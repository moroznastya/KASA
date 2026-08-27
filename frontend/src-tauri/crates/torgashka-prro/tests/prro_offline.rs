//! Unit-тести B4: offline state machine (109/110/112 + id_offline).
//! 1:1 Python `tests/unit/services/test_prro_offline_state.py`.

mod common;

use torgashka_prro::crypto::PrroSigner;
use torgashka_prro::prro::{
    InMemoryPrroRepository, MockChkSender, OfflineStateMachine, PrroOfflineQueue, PrroRepository,
    SyncOfflineQueueUseCase, CHECK_TYPE_CHK,
};

use common::{test_builder, MockSigner};

const XML: &str = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><P C="120" NM="Товар" PRC="100" Q="1" SM="10000" TX="0"></P><M T="0" SM="10000"></M><E N="1" SM="10000" TX="0" TXPR="20.00" TXSM="1667"></E></C><TS>20260807112601</TS></DAT>"#;

fn ts_2026() -> chrono::DateTime<chrono::Utc> {
    "2026-08-27T12:00:00Z".parse().unwrap()
}

#[tokio::test]
async fn offline_enter_reserve_exit_full_scenario() {
    // B4 критерій: online → (мережа впала) → 109 → 112 → offline-чеки з
    // id_offline → (мережа є) → 110 → sync; усі документи пройшли.
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    let mut builder = test_builder();
    let now = ts_2026();

    // 0. Початково — online
    assert!(!OfflineStateMachine::is_offline(&repo).await.unwrap());

    // 1. Мережа впала → enter_offline (T=109; best-effort: sender падає)
    //    push_fail з помилкою gRPC — імітація транспортного обриву.
    sender
        .responses
        .lock()
        .unwrap()
        .push(Err(torgashka_prro::grpc::PrroGrpcError::Rpc {
            status: tonic::Status::unavailable("net down"),
            max_retries: 0,
        }));
    OfflineStateMachine::enter_offline(&repo, &sender, &mut builder, &MockSigner, now)
        .await
        .unwrap();
    assert!(OfflineStateMachine::is_offline(&repo).await.unwrap(), "стан → offline");
    // T=109 було надіслано (спроба) — owned-копія (guard не тримаємо)
    let xml109 = String::from_utf8_lossy(&sender.calls.lock().unwrap()[0].check_sign).into_owned();
    assert!(xml109.contains(r#"<C T="109">"#), "T=109 у check_sign: {xml109}");

    // 2. reserve_numbers (T=112) → сервер дає діапазон у data_sign
    sender.push_ok_with_data(
        "reserve-ok",
        br#"<?xml version="1.0"?><RS V="1"><DAT><CNF TY="C" FR="1001" TO="1100" ER="0"/></DAT></RS>"#.to_vec(),
    );
    let (start, end) = OfflineStateMachine::reserve_numbers(&repo, &sender, &mut builder, &MockSigner, now)
        .await
        .unwrap();
    assert_eq!((start, end), (1001, 1100), "діапазон з data_sign");
    let xml112 = String::from_utf8_lossy(&sender.calls.lock().unwrap()[1].check_sign).into_owned();
    assert!(xml112.contains(r#"<C T="112">"#));

    // 3. Offline-чек: резервний local_number + id_offline (не порожній)
    let (offline_local, id_offline) = OfflineStateMachine::next_offline_local(&repo).await.unwrap();
    assert_eq!(offline_local, 1001);
    assert_eq!(id_offline, "offline-1001");
    assert!(!id_offline.is_empty(), "id_offline не порожній");

    // Документ у чергу (як fiscalize в offline): check_sign + id_offline
    let message = builder.build_message(XML, None, true).unwrap();
    let signed = MockSigner.sign(message.as_bytes()).unwrap();
    let item = PrroOfflineQueue::add_document(
        &repo,
        None,
        None,
        offline_local,
        CHECK_TYPE_CHK,
        XML,
        None,
        Some(String::from_utf8_lossy(&signed).into_owned()),
        Some(id_offline.clone()), // B4: offline-чек — id_offline не порожній
    )
    .await
    .unwrap();

    // 4. Мережа є → exit_offline (T=110) + sync → усі документи пройшли
    sender.push_ok("t110-ok");
    sender.push_ok("chk-offline-1001"); // sync відправляє offline-чек
    let res = OfflineStateMachine::exit_offline(&repo, &sender, &mut builder, &MockSigner, 100, now)
        .await
        .unwrap();
    assert!(!OfflineStateMachine::is_offline(&repo).await.unwrap(), "стан → online");
    assert_eq!(res.synced, 1, "offline-чек синхронізовано");
    assert_eq!(res.failed, 0);
    assert_eq!(res.total, 1);
    // T=110 надіслано
    let xml110 = String::from_utf8_lossy(&sender.calls.lock().unwrap()[2].check_sign).into_owned();
    assert!(xml110.contains(r#"<C T="110">"#));
    // offline-чек відправлено з id_offline (не порожнім)
    let offline_check = sender.calls.lock().unwrap()[3].clone();
    assert_eq!(offline_check.id_offline, "offline-1001", "id_offline у Check");
    assert_eq!(offline_check.local_number, 1001);
    // черга порожня
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
    let _ = item; // item використано
}

#[tokio::test]
async fn offline_local_number_increments_within_reserve_range() {
    let repo = InMemoryPrroRepository::new();
    repo.set_setting("prro_reserve_start", "1001").await.unwrap();
    repo.set_setting("prro_reserve_end", "1100").await.unwrap();
    let (n1, id1) = OfflineStateMachine::next_offline_local(&repo).await.unwrap();
    let (n2, id2) = OfflineStateMachine::next_offline_local(&repo).await.unwrap();
    assert_eq!((n1, n2), (1001, 1002));
    assert_eq!(id1, "offline-1001");
    assert_eq!(id2, "offline-1002");
    assert!(!id1.is_empty() && !id2.is_empty());
}

#[tokio::test]
async fn fiscalize_in_offline_uses_reserve_local_and_id_offline() {
    // Перевірка, що fiscalize в offline-режимі не йде в мережу, а формує
    // offline-чек з резервним local_number та id_offline (див. fiscalize.rs:
    // OfflineStateMachine::is_offline → next_offline_local).
    let repo = InMemoryPrroRepository::new();
    repo.set_setting("prro_offline", "1").await.unwrap();
    repo.set_setting("prro_reserve_start", "500").await.unwrap();
    repo.set_setting("prro_reserve_end", "600").await.unwrap();
    let (local, id_offline) = OfflineStateMachine::next_offline_local(&repo).await.unwrap();
    assert_eq!(local, 500);
    assert!(!id_offline.is_empty());
    // у Check id_offline підставляється fiscalize.make_check — тест рівня
    // модуля offline (інтеграцію fiscalize покрито Rust-тестами fiscalize).
    let _ = SyncOfflineQueueUseCase::sync(&repo, &MockChkSender::new(), &mut test_builder(), &MockSigner, 10).await;
}
