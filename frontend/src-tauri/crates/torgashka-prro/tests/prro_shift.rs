//! Unit-тести змін ПРРО (етап 7.3) — 1:1 Python shift_use_case.py.

mod common;

use chrono::{Duration, Utc};
use torgashka_prro::prro::{
    InMemoryPrroRepository, MockChkSender, PrroOfflineQueue, PrroQueueStatus, PrroRepository,
    PrroShiftUseCase, CHECK_TYPE_CHK, KEY_LAST_SHIFT_NUMBER,
};
use torgashka_prro::xml::parse_receipt_xml_totals;

use common::{test_builder, MockSigner};

/// Канонічний XML чеку продажу (T=0) — 1:1 format build_receipt_xml.
const SALE_XML: &str = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><P C="120" NM="Товар" PRC="100" Q="1" SM="10000" TX="0"></P><M T="0" SM="10000"></M><E N="1" SM="10000" TX="0" TXPR="20.00" TXSM="1667"></E></C><TS>20260807112601</TS></DAT>"#;
/// Канонічний XML чеку повернення (T=1).
const RETURN_XML: &str = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="2" V="2.1.7"><C T="1"><P C="120" NM="Товар" PRC="50" Q="1" SM="5000" TX="0"></P><M T="0" SM="5000"></M><E N="1" SM="5000" TX="0" TXPR="20.00" TXSM="833"></E></C><TS>20260807112701</TS></DAT>"#;

#[tokio::test]
async fn open_shift_creates_shift_and_queue() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    sender.push_ok("shift-108-id");

    let mut builder = test_builder();
    let now = Utc::now();
    let dto = PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(now))
        .await
        .expect("open_shift");

    // зміна створена: status=open, last_local_number=0
    assert_eq!(dto.shift_number, 1);
    assert_eq!(dto.status, "open");
    assert_eq!(dto.receipt_count, 0);
    let shift = repo.get_open_shift().await.unwrap().expect("open shift");
    assert_eq!(shift.last_local_number, 0);
    assert_eq!(shift.signer_serial.as_deref(), Some("5E984D526F82F38F"));
    assert_eq!(shift.signer_name.as_deref(), Some("ТЕСТОВИЙ ПІДПИСАНТ"));

    // last_shift_number збережено
    assert_eq!(
        repo.get_setting(KEY_LAST_SHIFT_NUMBER)
            .await
            .unwrap()
            .as_deref(),
        Some("1")
    );

    // службовий чек у черзі: SERVICECHK, local_number=0, sent
    let items = PrroOfflineQueue::list_by_shift(&repo, shift.id)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].check_type, "SERVICECHK");
    assert_eq!(items[0].local_number, 0);
    assert_eq!(items[0].status, PrroQueueStatus::Sent);
    assert!(
        items[0].xml_body.contains("T=\"108\""),
        "XML: {}",
        items[0].xml_body
    );
    assert!(items[0].xml_body.contains("<E N=\"1\"></E>"));

    // gRPC-виклик: SERVICECHK (enum=3), local_number=0, check_sign = RQ+MAC
    assert_eq!(sender.calls_len(), 1);
    let call = &sender.calls.lock().unwrap()[0];
    assert_eq!(call.check_type, 3);
    assert_eq!(call.local_number, 0);
    let sign = String::from_utf8(call.check_sign.clone()).unwrap();
    assert!(sign.starts_with("<RQ V=\"1\">"), "{sign}");
    assert!(sign.contains("<MAC "), "{sign}");
}

#[tokio::test]
async fn open_shift_twice_fails_already_open() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    sender.push_ok("1");
    let mut builder = test_builder();
    let now = Utc::now();
    PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(now))
        .await
        .expect("first open");

    let err = PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(now))
        .await
        .unwrap_err();
    assert_eq!(err.code, "SHIFT_ALREADY_OPEN");
    assert!(err.message.contains("вже відкрита"), "{}", err.message);
    // другий gRPC-виклик НЕ відбувся (перевірка до відправки)
    assert_eq!(sender.calls_len(), 1);
}

#[tokio::test]
async fn open_shift_server_rejects() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    sender.push_fail("статус сервера: ERROR_SAVE", -3);

    let mut builder = test_builder();
    let err =
        PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(Utc::now()))
            .await
            .unwrap_err();
    assert_eq!(err.code, "OPEN_SHIFT_FAILED");
    assert!(err.message.contains("ERROR_SAVE"));
    // зміна НЕ створена, черга порожня — нуль втрат/помилкового стану
    assert!(repo.get_open_shift().await.unwrap().is_none());
}

#[tokio::test]
async fn open_shift_grpc_error_no_state_change() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    // gRPC недоступний (відкат: чек не втрачається, стан не змінюється)
    sender
        .responses
        .lock()
        .unwrap()
        .push(Err(torgashka_prro::grpc::PrroGrpcError::Rpc {
            status: tonic::Status::unavailable("offline"),
            max_retries: 1,
        }));

    let mut builder = test_builder();
    let err =
        PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(Utc::now()))
            .await
            .unwrap_err();
    assert_eq!(err.code, "GRPC_ERROR");
    assert!(repo.get_open_shift().await.unwrap().is_none());
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
}

#[tokio::test]
async fn close_shift_no_open_shift() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    let mut builder = test_builder();
    let err = PrroShiftUseCase::close_shift(
        &repo,
        &sender,
        &mut builder,
        &MockSigner,
        None,
        Some(Utc::now()),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "NO_OPEN_SHIFT");
    assert_eq!(sender.calls_len(), 0);
}

#[tokio::test]
async fn close_shift_zreport_ok() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();

    // відкрити зміну
    sender.push_ok("shift-108");
    let mut builder = test_builder();
    let now = Utc::now();
    let shift = PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(now))
        .await
        .expect("open");
    let shift_id = shift.id;

    // 2 переданих чеки: продаж (100 грн) + повернення (50 грн)
    for (i, xml) in [SALE_XML, RETURN_XML].iter().enumerate() {
        let item = PrroOfflineQueue::add_document(
            &repo,
            None,
            Some(shift_id),
            (i + 1) as i64,
            CHECK_TYPE_CHK,
            xml,
            None,
        )
        .await
        .unwrap();
        PrroOfflineQueue::mark_sent(&repo, item.id, None)
            .await
            .unwrap();
    }

    // Z-звіт: сервер відповідає id="Z-20260807-001"
    sender.push_ok("Z-20260807-001");
    let closed = PrroShiftUseCase::close_shift(
        &repo,
        &sender,
        &mut builder,
        &MockSigner,
        Some("касир Іван".to_string()),
        Some(now + Duration::hours(2)),
    )
    .await
    .expect("close");

    assert_eq!(closed.status, "closed");
    assert_eq!(closed.zreport_number.as_deref(), Some("Z-20260807-001"));
    let shift = repo.get_shift(shift_id).await.unwrap().unwrap();
    assert_eq!(shift.status, torgashka_prro::prro::PrroShiftStatus::Closed);
    assert_eq!(shift.closed_by.as_deref(), Some("касир Іван"));
    assert!(shift.closed_at.is_some());

    // Z-звіт у черзі: ZREPORT, local_number=0, sent
    let items = PrroOfflineQueue::list_by_shift(&repo, shift_id)
        .await
        .unwrap();
    let z = items
        .iter()
        .find(|i| i.check_type == "ZREPORT")
        .expect("z item");
    assert_eq!(z.local_number, 0);
    assert_eq!(z.status, PrroQueueStatus::Sent);
    assert!(z.xml_body.contains("<Z NO=\"1\">"), "Z XML: {}", z.xml_body);
    // підсумки: 1 продаж + 1 повернення (з переданих чеків)
    assert!(
        z.xml_body.contains("<NC NI=\"1\" NO=\"1\"></NC>"),
        "{}",
        z.xml_body
    );

    // після закриття відкритої зміни немає
    assert!(repo.get_open_shift().await.unwrap().is_none());
}

#[tokio::test]
async fn close_shift_server_rejects_keeps_shift_open() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    sender.push_ok("open");
    let mut builder = test_builder();
    let now = Utc::now();
    PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(now))
        .await
        .expect("open");

    // сервер відхиляє Z-звіт
    sender.push_fail("ERROR_NOT_PREV_ZREPORT", -6);
    let err =
        PrroShiftUseCase::close_shift(&repo, &sender, &mut builder, &MockSigner, None, Some(now))
            .await
            .unwrap_err();
    assert_eq!(err.code, "CLOSE_SHIFT_FAILED");
    // зміна лишається open — відкат
    assert!(repo.get_open_shift().await.unwrap().is_some());
}

#[tokio::test]
async fn build_zreport_data_totals_1_1() {
    let repo = InMemoryPrroRepository::new();
    let sender = MockChkSender::new();
    sender.push_ok("open");
    let mut builder = test_builder();
    let now = Utc::now();
    let shift = PrroShiftUseCase::open_shift(&repo, &sender, &mut builder, &MockSigner, Some(now))
        .await
        .expect("open");

    for (i, xml) in [SALE_XML, RETURN_XML].iter().enumerate() {
        let item = PrroOfflineQueue::add_document(
            &repo,
            None,
            Some(shift.id),
            (i + 1) as i64,
            CHECK_TYPE_CHK,
            xml,
            None,
        )
        .await
        .unwrap();
        PrroOfflineQueue::mark_sent(&repo, item.id, None)
            .await
            .unwrap();
    }

    let shift = repo.get_open_shift().await.unwrap().unwrap();
    let data = PrroShiftUseCase::build_zreport_data(&repo, &shift)
        .await
        .unwrap();
    assert_eq!(data.sales_count, 1);
    assert_eq!(data.returns_count, 1);
    // оплати: готівка 100 - 50 = 50 (SMI=100, SMO=50)
    assert_eq!(data.payments.len(), 1);
    assert_eq!(data.payments[0].code, "0");
    assert_eq!(data.payments[0].name.as_deref(), Some("ГОТІВКА"));
    assert_eq!(data.payments[0].smi.as_deref(), Some("10000"));
    assert_eq!(data.payments[0].smo.as_deref(), Some("5000"));
    // податок: TX=0, TXI=16.67, TXO=8.33, SMI=150 (обіг)
    assert_eq!(data.taxes.len(), 1);
    assert_eq!(data.taxes[0].tax, "0");
    assert_eq!(data.taxes[0].tax_in.as_deref(), Some("1667"));
    assert_eq!(data.taxes[0].tax_out.as_deref(), Some("833"));
    assert_eq!(data.taxes[0].smi.as_deref(), Some("15000"));
    assert_eq!(data.taxes[0].tax_type.as_deref(), Some("0"));
    assert_eq!(data.taxes[0].tax_algorithm.as_deref(), Some("0"));
}

#[tokio::test]
async fn parse_totals_matches_python_semantics() {
    // прямий юніт парсера: 1:1 Python parse_receipt_xml_totals
    let p = parse_receipt_xml_totals(SALE_XML).unwrap();
    assert_eq!(p.check_type, "0");
    assert_eq!(p.total.to_string(), "100"); // Python: Decimal("10000")/100 = Decimal("100")
    assert_eq!(
        p.payments,
        vec![(
            "0".to_string(),
            rust_decimal::Decimal::from_str_exact("100.00").unwrap()
        )]
    );
    assert_eq!(p.taxes.len(), 1);
    assert_eq!(p.taxes[0].0, "0");
    assert_eq!(p.taxes[0].1.percent.to_string(), "20.00");
    assert_eq!(p.taxes[0].1.tax_total.to_string(), "16.67");
    assert_eq!(p.taxes[0].1.smi.to_string(), "100");

    let r = parse_receipt_xml_totals(RETURN_XML).unwrap();
    assert_eq!(r.check_type, "1");
    assert_eq!(r.total.to_string(), "50");
    assert_eq!(r.taxes[0].1.tax_total.to_string(), "8.33");
}
