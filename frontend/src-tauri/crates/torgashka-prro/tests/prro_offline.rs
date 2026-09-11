//! Unit-тести offline state machine (109/110/112 + id_offline, спека F/I).
//! 1:1 Python `tests/unit/services/test_prro_offline_state.py` (2026-09-08).

mod common;

use torgashka_prro::prro::{
    InMemoryPrroRepository, MockChkSender, OfflineStateMachine, PrroOfflineQueue, PrroRepository,
    SyncOfflineQueueUseCase, CHECK_TYPE_CHK,
};
use torgashka_prro::PrroSigner;

use common::{test_builder, MockSigner};

const XML: &str = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><P C="120" NM="Товар" PRC="100" Q="1" SM="10000" TX="0"></P><M T="0" SM="10000"></M><E N="1" SM="10000" TX="0" TXPR="20.00" TXSM="1667"></E></C><TS>20260807112601</TS></DAT>"#;

fn ts_2026() -> chrono::DateTime<chrono::Utc> {
    "2026-08-27T12:00:00Z".parse().unwrap()
}

fn reserve_data(ids: &[i64]) -> Vec<u8> {
    let mut xml = String::from("<?xml version=\"1.0\"?><RS V=\"1\"><C T=\"112\">");
    for id in ids {
        xml.push_str(&format!("<ID>{id}</ID>"));
    }
    xml.push_str("</C></RS>");
    xml.into_bytes()
}

#[tokio::test]
async fn offline_enter_reserve_exit_full_scenario() {
    // online → (мережа впала) → 109 (у черзі) → 112 (діапазон <ID>) →
    // offline-чек з id_offline=номер з діапазону → (мережа є) → 110 → sync.
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    let mut builder = test_builder();
    let now = ts_2026();

    // 0. Початково — online
    assert!(!OfflineStateMachine::is_offline(&repo).await.unwrap());

    // 1. Мережа впала → enter_offline (T=109; best-effort: sender падає)
    sender
        .responses
        .lock()
        .unwrap()
        .push(Err(torgashka_prro::grpc::PrroGrpcError::Rpc {
            status: tonic::Status::unavailable("net down"),
            max_retries: 0,
        }));
    OfflineStateMachine::enter_offline(&repo, &sender, &mut builder, &MockSigner, now, None)
        .await
        .unwrap();
    assert!(
        OfflineStateMachine::is_offline(&repo).await.unwrap(),
        "стан → offline"
    );
    // T=109 було надіслано (спроба)
    let xml109 = String::from_utf8_lossy(&sender.calls.lock().unwrap()[0].check_sign).into_owned();
    assert!(
        xml109.contains(r#"<C T="109">"#),
        "T=109 у check_sign: {xml109}"
    );
    // 109 НЕ втрачається: лежить у черзі (pending — доставка не вдалась)
    let q109 = PrroOfflineQueue::get_pending(&repo, 100).await.unwrap();
    assert!(
        q109.iter().any(|i| i.xml_body.contains(r#"T="109""#)),
        "109 у черзі для синку ланцюга"
    );

    // 2. reserve_numbers (T=112) → сервер дає перелік <ID> (спека F)
    sender.push_ok_with_data("reserve-ok", reserve_data(&[1001, 1002, 1003]));
    let (start, end) = OfflineStateMachine::reserve_numbers(
        &repo,
        &sender,
        &mut builder,
        &MockSigner,
        now,
        "",
        150,
    )
    .await
    .unwrap();
    assert_eq!((start, end), (1001, 1003), "діапазон з <ID> data_sign");
    let xml112 = String::from_utf8_lossy(&sender.calls.lock().unwrap()[1].check_sign).into_owned();
    assert!(xml112.contains(r#"<C T="112">"#));
    assert!(
        xml112.contains(r#"<H SIZE="150">"#),
        "112: <H SIZE=\"150\">: {xml112}"
    );

    // 3. Наступний резервний номер → id_offline (не рядок "offline-{n}")
    let offline_id = OfflineStateMachine::next_offline_number(&repo)
        .await
        .unwrap();
    assert_eq!(offline_id, 1001);
    let id_offline = offline_id.to_string();

    // Документ у чергу (як fiscalize в offline): check_sign + id_offline
    let message = builder
        .build_message(XML, Some(""), &id_offline, true)
        .unwrap();
    let bytes = torgashka_prro::xml::cp1251_bytes(&message).unwrap();
    let signed = MockSigner.sign(&bytes).unwrap();
    let item = PrroOfflineQueue::add_document(
        &repo,
        None,
        None,
        1,
        CHECK_TYPE_CHK,
        XML,
        Some(String::new()), // mac цього чека (doc_mac)
        Some(torgashka_prro::xml::signed_bytes_to_text(&signed)),
        Some(id_offline.clone()), // F: id_offline = резервний фіскальний номер
    )
    .await
    .unwrap();

    // 4. Мережа є → exit_offline (T=110) + sync → усі документи пройшли
    sender.push_ok("t110-ok");
    sender.push_ok("sync-109"); // 109 з черги (pending)
    sender.push_ok("chk-offline-1001"); // sync відправляє offline-чек
    let res =
        OfflineStateMachine::exit_offline(&repo, &sender, &mut builder, &MockSigner, now, "", 100)
            .await
            .unwrap();
    assert!(
        !OfflineStateMachine::is_offline(&repo).await.unwrap(),
        "стан → online"
    );
    assert_eq!(res.synced, 2, "109 + offline-чек синхронізовано");
    assert_eq!(res.failed, 0);
    assert_eq!(res.total, 2);
    // T=110 надіслано (call 2: 0=109, 1=112, 2=110)
    let xml110 = String::from_utf8_lossy(&sender.calls.lock().unwrap()[2].check_sign).into_owned();
    assert!(xml110.contains(r#"<C T="110">"#));
    // offline-чек відправлено з id_offline = номер з діапазону (call 4)
    let offline_check = sender.calls.lock().unwrap()[4].clone();
    assert_eq!(
        offline_check.id_offline, "1001",
        "id_offline = фіскальний номер з діапазону"
    );
    // черга порожня
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
    let _ = item; // item використано
}

#[tokio::test]
async fn offline_reserve_number_increments_and_exhausts() {
    let repo = InMemoryPrroRepository::new();
    repo.set_setting("prro_reserve_start", "1001")
        .await
        .unwrap();
    repo.set_setting("prro_reserve_end", "1100").await.unwrap();
    let n1 = OfflineStateMachine::next_offline_number(&repo)
        .await
        .unwrap();
    let n2 = OfflineStateMachine::next_offline_number(&repo)
        .await
        .unwrap();
    assert_eq!((n1, n2), (1001, 1002));
    // без діапазону → зрозуміла помилка (без фейкового дефолту, спека F)
    let repo2 = InMemoryPrroRepository::new();
    let err = OfflineStateMachine::next_offline_number(&repo2)
        .await
        .unwrap_err();
    assert!(err.message.contains("T=112"), "{err}");
}

#[tokio::test]
async fn fiscalize_offline_state_helpers() {
    // Перевірка допоміжних методів offline-режиму: is_offline та
    // next_offline_number з резервного діапазону.
    let repo = InMemoryPrroRepository::new();
    repo.set_setting("prro_offline", "1").await.unwrap();
    repo.set_setting("prro_reserve_start", "500").await.unwrap();
    repo.set_setting("prro_reserve_end", "600").await.unwrap();
    assert!(OfflineStateMachine::is_offline(&repo).await.unwrap());
    let id_offline = OfflineStateMachine::next_offline_number(&repo)
        .await
        .unwrap();
    assert_eq!(id_offline, 500);
    // sync порожньої черги — Ok(0)
    let res = SyncOfflineQueueUseCase::sync(
        &repo,
        &MockChkSender::new(),
        &mut test_builder(),
        &MockSigner,
        10,
    )
    .await
    .unwrap();
    assert_eq!(res.total, 0);
}
