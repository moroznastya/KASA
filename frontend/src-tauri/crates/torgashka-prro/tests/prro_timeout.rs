//! Unit-тести H1 (timeout recovery) + V1 (QR mac = MAC чека).
//! 1:1 Python `tests/unit/use_cases/test_prro_timeout_recovery.py`.

mod common;

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use torgashka_prro::grpc::PrroGrpcError;
use torgashka_prro::proto::{Check, CheckResponse};
use torgashka_prro::prro::{
    build_fiscal_check_url, ChkSender, FiscalizeReceiptUseCase, InMemoryPrroRepository,
    ProductFiscalRow, PrroKeyStore, PrroRepository, PrroShift, ReceiptFiscalRow,
    ReceiptItemFiscalRow, KEY_PRRO_FN,
};
use torgashka_prro::xml::{compute_mac, extract_check_no};

use common::{test_builder, MockSigner};

/// Керований надсилач: send_chk з черги відповідей, lastChk — окремо.
struct H1Sender {
    send_responses: Mutex<VecDeque<Result<CheckResponse, PrroGrpcError>>>,
    send_calls: Mutex<Vec<Check>>,
    last_chk_response: Mutex<Option<Result<CheckResponse, PrroGrpcError>>>,
    last_chk_calls: Mutex<usize>,
}

impl H1Sender {
    fn new() -> Self {
        Self {
            send_responses: Mutex::new(VecDeque::new()),
            send_calls: Mutex::new(Vec::new()),
            last_chk_response: Mutex::new(None),
            last_chk_calls: Mutex::new(0),
        }
    }

    fn push_transport_error(&self) {
        self.send_responses
            .lock()
            .unwrap()
            .push_back(Err(PrroGrpcError::Rpc {
                status: tonic::Status::unavailable("deadline exceeded"),
                max_retries: 1,
            }));
    }

    fn push_ok(&self, id: &str, data_sign: Vec<u8>) {
        self.send_responses
            .lock()
            .unwrap()
            .push_back(Ok(CheckResponse {
                id: id.to_string(),
                status: 1,
                id_sign: format!("id-sign-{id}").into_bytes(),
                data_sign,
                error_message: String::new(),
            }));
    }

    fn set_last_chk(&self, resp: CheckResponse) {
        *self.last_chk_response.lock().unwrap() = Some(Ok(resp));
    }

    fn send_calls_len(&self) -> usize {
        self.send_calls.lock().unwrap().len()
    }
}

#[async_trait]
impl ChkSender for H1Sender {
    async fn send_chk(&self, check: Check) -> Result<CheckResponse, PrroGrpcError> {
        self.send_calls.lock().unwrap().push(check);
        let mut q = self.send_responses.lock().unwrap();
        match q.pop_front() {
            Some(r) => r,
            None => Ok(CheckResponse {
                id: "mock-id".into(),
                status: 1,
                id_sign: vec![],
                data_sign: vec![],
                error_message: String::new(),
            }),
        }
    }

    async fn last_chk(&self) -> Result<CheckResponse, PrroGrpcError> {
        *self.last_chk_calls.lock().unwrap() += 1;
        self.last_chk_response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Err(PrroGrpcError::Rpc {
                status: tonic::Status::unimplemented("no lastChk mock"),
                max_retries: 0,
            }))
    }
}

// ─── Сіди ────────────────────────────────────────────────────────────────────

fn tmp_keystore() -> (PrroKeyStore, std::path::PathBuf) {
    let key = fernet::Fernet::generate_key();
    let path = std::env::temp_dir().join(format!("prro_ks_h1_{}.json", uuid::Uuid::new_v4()));
    let ks = PrroKeyStore::new(Some(&key), Some(path.to_str().unwrap()));
    ks.save_key_path("/tmp/mock-key.pfx", Some("pfx")).unwrap();
    ks.save_password_encrypted("secret").unwrap();
    (ks, path)
}

async fn seed_open_shift(repo: &InMemoryPrroRepository) -> uuid::Uuid {
    let shift = PrroShift::new(1, chrono::Utc::now());
    let id = shift.id;
    repo.create_shift(shift).await.unwrap();
    id
}

fn seed_sale_receipt(repo: &InMemoryPrroRepository) -> uuid::Uuid {
    let rid = uuid::Uuid::new_v4();
    let pid = uuid::Uuid::new_v4();
    repo.seed_setting(KEY_PRRO_FN, "4000000001");
    repo.seed_product(ProductFiscalRow {
        id: pid,
        title: Some("Тестовий товар".to_string()),
        fiscal_stock: rust_decimal::Decimal::new(10, 0),
        tax_rate: Some(rust_decimal::Decimal::new(20, 0)),
    });
    repo.seed_receipt(ReceiptFiscalRow {
        id: rid,
        receipt_number: "T-H1".to_string(),
        cashier_id: uuid::Uuid::new_v4(),
        total_amount: rust_decimal::Decimal::new(2500, 2),
        paid_amount: Some(rust_decimal::Decimal::new(2500, 2)),
        change_amount: Some(rust_decimal::Decimal::ZERO),
        debtor_id: None,
        is_return: false,
        notes: None,
        payment_method: Some("cash".to_string()),
        cash_amount: Some(rust_decimal::Decimal::new(2500, 2)),
        card_amount: Some(rust_decimal::Decimal::ZERO),
        original_receipt_id: None,
        return_reason: None,
        split_group_id: None,
        fiscal_status: "pending".to_string(),
        fiscal_number: None,
        fiscal_serial: None,
        fiscal_sent_at: None,
        fiscal_error: None,
        is_fiscal: false,
        items: vec![ReceiptItemFiscalRow {
            id: uuid::Uuid::new_v4(),
            product_id: pid,
            quantity: rust_decimal::Decimal::new(2, 0),
            price: rust_decimal::Decimal::new(1250, 2),
            total: rust_decimal::Decimal::new(2500, 2),
            purchase_price: None,
            fiscal_quantity: rust_decimal::Decimal::new(2, 0),
        }],
    });
    rid
}

fn xml_with_no(no: i64) -> Vec<u8> {
    format!(
        r#"<DAT FN="4000000001" TN="4000000001" ZN="4000000001" DI="1" V="2.1.7"><C T="0"><E N="1" NO="{no}" SM="2500" TX="0"></E></C><TS>20260827120000</TS></DAT>"#
    )
    .into_bytes()
}

// ─── H1: timeout recovery ────────────────────────────────────────────────────

/// Критерій H1 (сценарій 1): після timeout lastChk знаходить наш чек
/// (NO == local_number) → SENT; send_chk викликано РІВНО 1 раз (без дубліката).
#[tokio::test]
async fn timeout_lastchk_finds_check_marks_sent_no_duplicate() {
    let repo = InMemoryPrroRepository::new();
    seed_open_shift(&repo).await;
    let rid = seed_sale_receipt(&repo);
    let (ks, _p) = tmp_keystore();
    let sender = H1Sender::new();

    // 1-й send → транспортний таймаут
    sender.push_transport_error();
    // lastChk → сервер зберіг чек: XML з NO=1 (наш local_number)
    sender.set_last_chk(CheckResponse {
        id: "FISCAL-TIMEOUT-1".into(),
        status: 1,
        id_sign: b"id-sign-timeout".to_vec(),
        data_sign: xml_with_no(1),
        error_message: String::new(),
    });

    let mut builder = test_builder();
    let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
        &repo,
        &ks,
        &sender,
        &mut builder,
        &MockSigner,
        rid,
        true,
    )
    .await
    .unwrap();

    assert_eq!(resp.fiscal_status, "sent", "lastChk знайшов чек → SENT");
    assert_eq!(resp.fiscal_number.as_deref(), Some("FISCAL-TIMEOUT-1"));
    // send_chk викликано рівно 1 раз → жодного дубліката на сервері
    assert_eq!(sender.send_calls_len(), 1, "жодного повторного send");
    // lastChk викликано 1 раз
    assert_eq!(*sender.last_chk_calls.lock().unwrap(), 1);
    // чек у черзі позначено sent
    let r = repo.load_receipt_with_items(rid).await.unwrap().unwrap();
    assert_eq!(r.fiscal_status, "sent");
}

/// Критерій H1 (сценарій 2): lastChk НЕ знаходить наш чек (NO != наш) →
/// один контрольований повторний send → SENT.
#[tokio::test]
async fn timeout_lastchk_not_found_then_retry_succeeds() {
    let repo = InMemoryPrroRepository::new();
    seed_open_shift(&repo).await;
    let rid = seed_sale_receipt(&repo);
    let (ks, _p) = tmp_keystore();
    let sender = H1Sender::new();

    // 1-й send → таймаут
    sender.push_transport_error();
    // lastChk → останній чек ЧУЖИЙ (NO=99)
    sender.set_last_chk(CheckResponse {
        id: "OTHER-CHK".into(),
        status: 1,
        id_sign: vec![],
        data_sign: xml_with_no(99),
        error_message: String::new(),
    });
    // 2-й send (retry) → успіх
    sender.push_ok("FISCAL-RETRY-1", vec![]);

    let mut builder = test_builder();
    let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
        &repo,
        &ks,
        &sender,
        &mut builder,
        &MockSigner,
        rid,
        true,
    )
    .await
    .unwrap();

    assert_eq!(resp.fiscal_status, "sent");
    assert_eq!(resp.fiscal_number.as_deref(), Some("FISCAL-RETRY-1"));
    assert_eq!(sender.send_calls_len(), 2, "рівно 1 повторний send");
    assert_eq!(*sender.last_chk_calls.lock().unwrap(), 1);
}

/// Критерій H1 (сценарій 3): lastChk не знаходить, повторний send теж
/// транспортна помилка → документ у черзі (failed), ПРРО → offline.
#[tokio::test]
async fn timeout_retry_fails_document_queued_and_offline() {
    let repo = InMemoryPrroRepository::new();
    seed_open_shift(&repo).await;
    let rid = seed_sale_receipt(&repo);
    let (ks, _p) = tmp_keystore();
    let sender = H1Sender::new();

    sender.push_transport_error(); // 1-й send → таймаут
    sender.set_last_chk(CheckResponse {
        id: "OTHER-CHK".into(),
        status: 1,
        id_sign: vec![],
        data_sign: xml_with_no(99),
        error_message: String::new(),
    });
    sender.push_transport_error(); // 2-й send (retry) → теж таймаут

    let mut builder = test_builder();
    let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
        &repo,
        &ks,
        &sender,
        &mut builder,
        &MockSigner,
        rid,
        true,
    )
    .await
    .unwrap();

    assert_eq!(resp.fiscal_status, "failed", "документ у черзі (failed)");
    assert!(resp.error.is_some(), "помилка зафіксована");
    // документ НЕ втрачений: лишився в черзі
    let q = torgashka_prro::prro::PrroOfflineQueue::get_pending(&repo, 100)
        .await
        .unwrap();
    assert_eq!(q.len(), 1, "документ у черзі");
    assert_eq!(q[0].local_number, 1);
    // ПРРО перейшов в офлайн (B4)
    assert!(torgashka_prro::prro::OfflineStateMachine::is_offline(&repo)
        .await
        .unwrap());
    // 2 спроби НАШОГО чека (первинний + контрольований retry), далі — службові
    // T=109 (enter_offline) і T=112 (reserve_numbers): разом 4 виклики.
    let calls = sender.send_calls.lock().unwrap();
    assert_eq!(calls.len(), 4, "2 спроби чека + T=109 + T=112");
    assert_eq!(calls[0].local_number, 1, "перша спроба — наш чек");
    assert_eq!(
        calls[1].local_number, 1,
        "retry — той самий чек (без дубліката номера)"
    );
}

// ─── V1: QR mac = MAC чека ───────────────────────────────────────────────────

#[test]
fn extract_check_no_parses_no_from_dat() {
    assert_eq!(
        extract_check_no(&String::from_utf8_lossy(&xml_with_no(7))),
        Some(7)
    );
    assert_eq!(
        extract_check_no("<DAT><C T=\"0\"><P N=\"1\"></P></C></DAT>"),
        None
    );
}

#[test]
fn v1_qr_uses_check_mac_not_fallback_sha1() {
    let mac = compute_mac(
        r#"<DAT FN="4000000001"><C T="0"><E N="1" NO="1" SM="2500" TX="0"></E></C></DAT>"#,
        None,
    );
    let url = build_fiscal_check_url(
        "45",
        rust_decimal::Decimal::new(78000, 2),
        "3000898168",
        chrono::DateTime::parse_from_rfc3339("2022-09-04T11:30:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
        Some(&mac),
    )
    .expect("URL є");
    // Rust URL-енкодить параметри як Python urlencode (quote_plus): + / =
    // у base64 MAC → %2B %2F %3D
    let encoded_mac = mac
        .replace('+', "%2B")
        .replace('/', "%2F")
        .replace('=', "%3D");
    assert!(
        url.contains(&format!("mac={encoded_mac}")),
        "QR mac = MAC чека (URL-encoded): {url}"
    );
    assert!(!url.contains("id_sign"), "id_sign не потрапляє в QR");
    // ДПС §5: ?mac=...&date=20220904&time=1130&id=45&sm=780.00&fn=3000898168
    assert!(url.contains("date=20220904"), "{url}");
    assert!(url.contains("time=1130"), "{url}");
    assert!(url.contains("id=45"), "{url}");
    assert!(url.contains("sm=780.00"), "{url}");
    assert!(url.contains("fn=3000898168"), "{url}");
}

/// V1: fiscalize on_success будує QR з MAC чека (не id_sign).
#[tokio::test]
async fn v1_fiscalize_qr_contains_check_mac() {
    let repo = InMemoryPrroRepository::new();
    seed_open_shift(&repo).await;
    let rid = seed_sale_receipt(&repo);
    let (ks, _p) = tmp_keystore();
    let sender = H1Sender::new();
    // успішний send: data_sign = XML останнього чека з NO=1
    sender.push_ok("FISCAL-OK-1", xml_with_no(1));

    let mut builder = test_builder();
    let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
        &repo,
        &ks,
        &sender,
        &mut builder,
        &MockSigner,
        rid,
        true,
    )
    .await
    .unwrap();

    let url = resp.fiscal_check_url.expect("QR URL є");
    assert!(
        url.starts_with("https://cabinet.tax.gov.ua/cashregs/check?"),
        "{url}"
    );
    // mac у QR == MAC чека (SHA-256 base64), а НЕ id_sign.
    // Якби баг повернувся (serial=id_sign), тут було б "mac=id-sign-...".
    let mac_param = url
        .split("mac=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .expect("mac параметр є");
    assert!(
        !mac_param.contains("id-sign"),
        "id_sign не має потрапляти в QR: {url}"
    );
    // URL-encode (як Python urlencode): %2B %2F %3D → + / =
    let mac_unquoted = mac_param
        .replace("%2B", "+")
        .replace("%2F", "/")
        .replace("%3D", "=");
    // валідний base64 (SHA-256 → 32 байти)
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(mac_unquoted)
        .expect("mac — валідний base64");
    assert_eq!(decoded.len(), 32, "SHA-256 = 32 байти");
}
