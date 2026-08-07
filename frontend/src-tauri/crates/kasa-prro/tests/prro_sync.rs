//! Unit-тести синхронізації офлайн-черги (етап 7.3) — 1:1 Python sync.py.

mod common;

use kasa_prro::prro::{
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
        let item = PrroOfflineQueue::add_document(&repo, None, None, i, CHECK_TYPE_CHK, XML, None)
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
    PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None)
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
    let item = PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None)
        .await
        .unwrap();
    sender
        .responses
        .lock()
        .unwrap()
        .push(Err(kasa_prro::grpc::PrroGrpcError::Rpc {
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
        PrroOfflineQueue::add_document(&repo, None, None, i, CHECK_TYPE_CHK, XML, None)
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
    PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None)
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
    PrroOfflineQueue::add_document(&repo, None, None, 0, CHECK_TYPE_SERVICECHK, svc_xml, None)
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
