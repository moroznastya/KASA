//! Unit-тести синхронізації офлайн-черги (етап 7.3) — 1:1 Python sync.py.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};

use torgashka_prro::crypto::{PrroCryptoError, PrroSigner};
use torgashka_prro::prro::{
    check_type_code, InMemoryPrroRepository, MockChkSender, PrroOfflineQueue, PrroQueueStatus,
    PrroRepository, SyncOfflineQueueUseCase, CHECK_TYPE_CHK, CHECK_TYPE_SERVICECHK,
};

use common::{test_builder, MockSigner};

const XML: &str = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><P C="120" NM="Товар" PRC="100" Q="1" SM="10000" TX="0"></P><M T="0" SM="10000"></M><E N="1" SM="10000" TX="0" TXPR="20.00" TXSM="1667"></E></C><TS>20260807112601</TS></DAT>"#;

#[tokio::test]
async fn sync_empty_queue() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    let mut builder = test_builder();
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res.synced, 0);
    assert_eq!(res.failed, 0);
    assert_eq!(res.total, 0);
    assert_eq!(sender.calls_len(), 0);
}

#[tokio::test]
async fn sync_replays_pending_to_sent() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    // 2 чеки → сервер OK
    for i in 1..=2 {
        let item = PrroOfflineQueue::add_document(&repo, None, None, i, CHECK_TYPE_CHK, XML, None, None, None)
            .await
            .unwrap();
        assert_eq!(item.status, PrroQueueStatus::Pending);
    }
    sender.push_ok("chk-1");
    sender.push_ok("chk-2");

    let mut builder = test_builder();
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res.synced, 2);
    assert_eq!(res.failed, 0);
    assert_eq!(res.total, 2);
    assert_eq!(res.results[0].status, "sent");
    assert_eq!(res.results[1].status, "sent");
    assert_eq!(sender.calls_len(), 2);
    // документи позначено sent
    let items = PrroOfflineQueue::get_pending(&repo, 100).await.unwrap();
    assert!(items.is_empty());
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
    // повторна синхронізація — нічого не надсилає
    let res2 = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res2.total, 0);
    assert_eq!(sender.calls_len(), 2);
}

#[tokio::test]
async fn sync_server_rejects_marks_failed() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    // сервер відхиляє: ERROR_OFFLINE_168 (-11) — expired
    sender.push_fail("ERROR_OFFLINE_168", -11);

    let mut builder = test_builder();
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res.synced, 0);
    assert_eq!(res.failed, 1);
    assert_eq!(res.results[0].status, "failed");
    assert_eq!(res.results[0].error.as_deref(), Some("ERROR_OFFLINE_168"));
    // документ лишився у черзі (failed + error) — НЕ втрачений
    let items = PrroOfflineQueue::get_pending(&repo, 100).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].status, PrroQueueStatus::Failed);
    assert_eq!(items[0].error.as_deref(), Some("ERROR_OFFLINE_168"));
}

#[tokio::test]
async fn sync_grpc_error_marks_failed_keeps_document() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    let item = PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    sender
        .responses
        .lock()
        .unwrap()
        .push(Err(torgashka_prro::grpc::PrroGrpcError::Rpc {
            status: tonic::Status::unavailable("fiscal server offline"),
            max_retries: 1,
        }));

    let mut builder = test_builder();
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res.failed, 1);
    assert!(res.results[0]
        .error
        .as_deref()
        .unwrap_or("")
        .contains("fiscal server offline"));
    // відкат: документ НЕ втрачений — failed + error у черзі
    let item = repo.get_queue_item(item.id).await.unwrap().unwrap();
    assert_eq!(item.status, PrroQueueStatus::Failed);
    assert!(item
        .error
        .as_deref()
        .unwrap_or("")
        .contains("fiscal server offline"));
}

#[tokio::test]
async fn sync_respects_limit() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    for i in 0..5 {
        PrroOfflineQueue::add_document(&repo, None, None, i, CHECK_TYPE_CHK, XML, None, None, None)
            .await
            .unwrap();
    }
    let mut builder = test_builder();
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 2)
        .await
        .unwrap();
    assert_eq!(res.total, 2);
    assert_eq!(res.synced, 2);
    assert_eq!(sender.calls_len(), 2);
    // решта лишилась pending
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 3);
}

#[tokio::test]
async fn sync_failed_then_retry_succeeds() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    // перша спроба — помилка
    sender.push_fail("network", -2);
    let mut builder = test_builder();
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res.failed, 1);
    // повторна спроба (failed знову в черзі) — сервер OK
    sender.push_ok("chk-retry");
    let res2 = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res2.synced, 1);
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
}

#[test]
fn check_type_codes_match_python_map() {
    // 1:1 Python _PRRO_CHECK_TYPE_MAP
    assert_eq!(check_type_code("CHK"), 1);
    assert_eq!(check_type_code("ZREPORT"), 2);
    assert_eq!(check_type_code("SERVICECHK"), 3);
    assert_eq!(check_type_code("UNKNOWN"), 1);
}

#[tokio::test]
async fn sync_service_check_uses_local_number_0() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    // службовий чек у черзі (наприклад, T=108, local_number=0)
    let svc_xml = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="9" V="2.1.7"><C T="108"><E N="1"></E></C><TS>20260807113000</TS></DAT>"#;
    PrroOfflineQueue::add_document(&repo, None, None, 0, CHECK_TYPE_SERVICECHK, svc_xml, None, None, None)
        .await
        .unwrap();
    sender.push_ok("svc-ok");

    let mut builder = test_builder();
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &MockSigner, 100)
        .await
        .unwrap();
    assert_eq!(res.synced, 1);
    let call = &sender.calls.lock().unwrap()[0];
    assert_eq!(call.check_type, 3); // SERVICECHK
    assert_eq!(call.local_number, 0);
    let sign = String::from_utf8(call.check_sign.clone()).unwrap();
    assert!(sign.contains("T=\"108\""), "{sign}");
}

/// Лічильник викликів sign — прямий доказ "build_message/sign ≤ 1 разу на документ".
struct CountingSigner {
    calls: AtomicUsize,
}

impl PrroSigner for CountingSigner {
    fn sign(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(xml_bytes.to_vec())
    }
    fn verify(&self, _signed_xml: &[u8]) -> Result<bool, PrroCryptoError> {
        Ok(true)
    }
    fn get_serial_number(&self) -> Result<String, PrroCryptoError> {
        Ok("5E984D526F82F38F".to_string())
    }
    fn get_signer_name(&self) -> Result<String, PrroCryptoError> {
        Ok("ТЕСТОВИЙ ПІДПИСАНТ".to_string())
    }
}

/// B2: ідемпотентність sync — ПОВНИЙ підписаний check_sign зберігається у черзі
/// і відправляється as-is; sign/build_message викликаються РІВНО 1 раз на документ
/// (при первинному формуванні); повторна sync після failed → ІДЕНТИЧНИЙ check_sign.
#[tokio::test]
async fn sync_is_idempotent_check_sign_unchanged() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    let mut builder = test_builder();
    let signer = CountingSigner {
        calls: AtomicUsize::new(0),
    };

    // Документ, як формує fiscalize: dat_xml → build_message → sign = check_sign
    let dat_xml = XML;
    let message = builder.build_message(dat_xml, None, true).unwrap();
    let signed = MockSigner.sign(message.as_bytes()).unwrap();
    let signed_str = String::from_utf8_lossy(&signed).into_owned();
    assert_eq!(signer.calls.load(Ordering::SeqCst), 0, "signer ще не викликався");

    let item = PrroOfflineQueue::add_document(
        &repo,
        None,
        None,
        1,
        CHECK_TYPE_CHK,
        dat_xml,
        None,
        Some(signed_str.clone()), // B2: повний підписаний check_sign
        None, // B4: id_offline
    )
    .await
    .unwrap();

    // 1-ша спроба sync → успіх
    sender.push_ok("chk-1");
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &signer, 100)
        .await
        .unwrap();
    assert_eq!(res.synced, 1);
    assert_eq!(signer.calls.load(Ordering::SeqCst), 0, "sync НЕ переформовує (sign 0 викликів)");
    let first_sent = sender.calls.lock().unwrap()[0].check_sign.clone();
    assert_eq!(first_sent, signed_str.as_bytes(), "відправлено збережений check_sign as-is");

    // Обрив на сервері: документ повертається у failed
    PrroOfflineQueue::mark_failed(&repo, item.id, "net down".into())
        .await
        .unwrap();

    // 2-га спроба sync → ІДЕНТИЧНИЙ check_sign (NT/MAC/підпис не змінюються)
    sender.push_ok("chk-2");
    let res2 = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &signer, 100)
        .await
        .unwrap();
    assert_eq!(res2.synced, 1);
    assert_eq!(signer.calls.load(Ordering::SeqCst), 0, "повторна sync: sign 0 викликів");
    let second_sent = sender.calls.lock().unwrap()[1].check_sign.clone();
    assert_eq!(
        first_sent, second_sent,
        "B2: check_sign ідентичний між спробами (NT не змінюється)"
    );
}

/// B2: документ БЕЗ check_sign (доданий до B2) — формується РІВНО 1 раз при
/// першій sync, фіксується у черзі; повторна sync — as-is (sign більше не викликається).
#[tokio::test]
async fn sync_legacy_item_formats_once_and_persists_check_sign() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    let mut builder = test_builder();
    let signer = CountingSigner {
        calls: AtomicUsize::new(0),
    };

    // Legacy-документ: check_sign = None
    let item = PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();

    // 1-ша sync: формує і зберігає
    sender.push_ok("chk-1");
    let res = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &signer, 100)
        .await
        .unwrap();
    assert_eq!(res.synced, 1);
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1, "1-ша sync: sign 1 раз");
    let first_sent = sender.calls.lock().unwrap()[0].check_sign.clone();

    // check_sign зафіксовано у черзі
    let stored = repo.get_queue_item(item.id).await.unwrap().unwrap();
    assert!(stored.check_sign.is_some(), "check_sign збережено у черзі");
    assert_eq!(
        stored.check_sign.unwrap().as_bytes(),
        first_sent,
        "у черзі саме те, що відправлено"
    );

    // Повертаємо у failed і синхронізуємо вдруге → as-is, sign не викликається
    PrroOfflineQueue::mark_failed(&repo, item.id, "net down".into())
        .await
        .unwrap();
    sender.push_ok("chk-2");
    let res2 = SyncOfflineQueueUseCase::sync(&repo, &sender, &mut builder, &signer, 100)
        .await
        .unwrap();
    assert_eq!(res2.synced, 1);
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1, "повторна sync: sign НЕ викликається");
    let second_sent = sender.calls.lock().unwrap()[1].check_sign.clone();
    assert_eq!(first_sent, second_sent, "check_sign ідентичний");
}
