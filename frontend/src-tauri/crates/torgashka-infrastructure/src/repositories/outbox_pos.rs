//! `OutboxPos` — POS-порт для **standby-вузла** (ADR-0007 §10, §11.1).
//!
//! Причина існування (баг до виправлення): на standby `state.pos` був
//! `SqlxPos(репліка)`, тому кожен запис POS-документа падав з PG-помилкою
//! `read-only transaction` → 500. §11.1 класифікує чеки/накладні/списання/
//! переміщення як **`LocalOutbox`**: документ каси не йде в PG локально, а
//! кладеться в SQLite-чергу (`offline/`) тією самою атомарною транзакцією, що
//! й локальний stock-ефект (§10, контур `sync_push.rs:112-219` /
//! `transactions.rs:153-204`). Доставку на primary робить наявний push-цикл.
//!
//! Правила адаптера:
//!   * **ЧИТАННЯ** — прямий делегат `inner` (репліка — дозволене джерело
//!     читань, §10). Не переписуються.
//!   * **ЗАПИС** — локальна черга SQLite. Жодного SQL у напрямку PG.
//!   * **`PgPool` відсутній у структурі** — адаптер фізично не має чим писати
//!     в PG (доказ — тест `no_pool_field_and_inner_write_not_called`).
//!   * Жодного per-request online-розгалуження (`check_online()`): на standby
//!     пишемо в чергу ЗАВЖДИ.
//!   * Метод без outbox-еквівалента і не читання → явна помилка з людським
//!     текстом (без паніки, без `unwrap`).
//!
//! Межа класу: `receipt`, `return_receipt`, `invoice`, `purchase_order`,
//! `inventory`, `transfer`, `write_off` (§11.1) — те, що вміє `outbox` +
//! приймач `/api/v1/sync/push` (`sync.rs:615-647`, `sync_receivers.rs`).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use torgashka_domain::{
    CashOperationCreateInput, CashOperationDto, CashOperationsListDto, DocItemInput, MySessionsDto,
    PosError, PosService, ProductRecentSalesDto, PrroShiftDto, ReceiptCreateInput, ReceiptDto,
    ReceiptItemDetailDto, ReceiptItemDto, ReceiptListDto, ReceiptListQuery, ReceiptSearchDto,
    ReceiptSearchQuery, ReceiptStatsDto, ReceiptV1CreateInput, ReceiptV1Dto, ReceiptV1ItemDto,
    ReceiptV1ListDto, ReceiptV1ListQuery, ReceiptV1SearchDto, ReturnableQtyDto, ShiftListDto,
    TransferCreateInput, TransferDto, TransferItemDto, TransferListDto, TransferUpdateInput,
    UserSessionsDto, WorkReportDto, WriteOffCreateInput, WriteOffDto, WriteOffItemDto,
    WriteOffListDto, WriteOffReasonItem, WriteOffReasonsListDto, WriteOffUpdateInput,
};

use crate::offline::{db::OfflineDatabase, snapshots, sync_push, transactions};

/// POS-порт standby-вузла: читання → `inner`, запис → SQLite-черга.
///
/// `inner` — та сама реалізація на пулі локальної репліки (`SqlxPos`), тому
/// читання на standby 1:1 з primary (репліка WAL-доганяє primary).
pub struct OutboxPos {
    inner: Arc<dyn PosService + Send + Sync>,
}

impl OutboxPos {
    pub fn new(inner: Arc<dyn PosService + Send + Sync>) -> Self {
        Self { inner }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Інфраструктурні помилки (D: технічний текст → лог, користувачу — стабільне)
// ─────────────────────────────────────────────────────────────────────────────

/// Технічний текст — у `torgashka.log`; користувачу — стабільне повідомлення
/// без імен таблиць/колонок/SQL (санація ADR-0007 §D контракту).
fn infra_err(op: &str, tech: impl std::fmt::Display) -> PosError {
    let tech = tech.to_string();
    crate::embedded_pg::pg_log("ERROR", &format!("[outbox_pos] {op}: {tech}"));
    PosError::Infrastructure(format!(
        "{op}: не вдалося зберегти документ у локальній черзі, спробуйте ще раз"
    ))
}

/// Операція, для якої в локальній черзі НЕМАЄ представлення (і приймач
/// `/api/v1/sync/push` її теж не знає) → явна відмова з людським текстом.
///
/// Варіант `BadRequest`, а не `Infrastructure`: санація D замінює текст
/// `Infrastructure` на стабільне «спробуйте ще раз», а тут повтор не допоможе —
/// потрібен головний сервер. Прецедент у цьому ж шарі — `open_shift`
/// (`repositories/pos.rs:3555` повертає `BadRequest` для недоступної гілки).
fn unavailable(op: &str) -> PosError {
    PosError::BadRequest(format!(
        "{op}: операція недоступна на цьому вузлі (потрібен головний сервер)"
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Локальна черга: відкриття SQLite, store_id, конверти payload
// ─────────────────────────────────────────────────────────────────────────────

/// Виконати блокуючу роботу з SQLite у `spawn_blocking` (rusqlite — блокуючий).
async fn with_conn<T, F>(op: &'static str, f: F) -> Result<T, PosError>
where
    F: FnOnce(&mut rusqlite::Connection) -> Result<T, PosError> + Send + 'static,
    T: Send + 'static,
{
    let join = tokio::task::spawn_blocking(move || -> Result<T, PosError> {
        let path = OfflineDatabase::default_db_path().map_err(|e| infra_err(op, e))?;
        // Каталог даних каси може бути ще не створений (headless standby:
        // фасад стартував без десктопного застосунку, який його ініціалізує).
        // `sync_push::open_connection` створює файл і доганяє міграції —
        // потрібен лише наявний каталог.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                infra_err(op, format!("каталог даних каси {}: {e}", parent.display()))
            })?;
        }
        let mut conn = sync_push::open_connection(&path).map_err(|e| infra_err(op, e))?;
        f(&mut conn)
    })
    .await;
    match join {
        Ok(res) => res,
        Err(e) => Err(infra_err(op, format!("локальний таск: {e}"))),
    }
}

/// `store_id` точки каси: SQLite `settings.store_id` → env `TORGASHKA_STORE_ID`.
///
/// Порожньо в обох джерелах → `Validation` (422), НЕ 500: каса не налаштована.
fn resolve_store_id(conn: &rusqlite::Connection) -> Result<String, PosError> {
    let from_sqlite = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'store_id'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok();
    let candidate = from_sqlite.filter(|s| !s.trim().is_empty()).or_else(|| {
        std::env::var("TORGASHKA_STORE_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
    });
    candidate
        .map(|s| s.trim().to_string())
        .ok_or_else(|| PosError::Validation("точку продажу не налаштовано".to_string()))
}

/// Число з рядка домену → JSON-число (fallback — рядок як є).
fn num(s: &str) -> Value {
    match s.trim().parse::<f64>() {
        Ok(f) if f.is_finite() => json!(f),
        _ => Value::String(s.to_string()),
    }
}

/// `f64` з рядка домену (для DTO-відповіді); некоректне → 0.
fn f64_of(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// Добуток Decimal-рядків (total позиції) як рядок.
fn dec_mul(a: &str, b: &str) -> String {
    use std::str::FromStr;
    match (
        bigdecimal::BigDecimal::from_str(a.trim()),
        bigdecimal::BigDecimal::from_str(b.trim()),
    ) {
        (Ok(x), Ok(y)) => (x * y).to_string(),
        _ => "0".to_string(),
    }
}

fn uuid_of(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or_else(|_| Uuid::nil())
}

/// JSON чека каси (форма `/api/v2/receipts/sale|return` + `receipt_type`).
///
/// `receipt_type` — не для приймача (він визначає тип із `PushEnvelope.kind`),
/// а для локального `sync_push::outbox_type_of` (мапа JSON → тип агрегата).
fn receipt_json(input: &ReceiptCreateInput, receipt_type: &str, client_uuid: &Uuid) -> Value {
    let items: Vec<Value> = input
        .items
        .iter()
        .map(|it| {
            json!({
                "product_id": it.product_id,
                "name": it.name,
                "quantity": num(&it.quantity),
                "price": num(&it.price),
                "tax_rate": it.tax_rate,
            })
        })
        .collect();
    let mut v = json!({
        "receipt_type": receipt_type,
        "items": items,
        "payment_method": input.payment_method,
        "notes": input.notes,
        "is_fiscal": input.is_fiscal,
        "client_uuid": client_uuid,
    });
    let obj = v.as_object_mut().expect("object");
    if let Some(x) = &input.cash_amount {
        obj.insert("cash_amount".into(), num(x));
    }
    if let Some(x) = &input.card_amount {
        obj.insert("card_amount".into(), num(x));
    }
    if let Some(x) = input.customer_id {
        obj.insert("customer_id".into(), json!(x));
    }
    if let Some(x) = input.split_group_id {
        obj.insert("split_group_id".into(), json!(x));
    }
    for (key, val) in [
        ("terminal_rrn", &input.terminal_rrn),
        ("terminal_approval_code", &input.terminal_approval_code),
        ("terminal_invoice_number", &input.terminal_invoice_number),
        ("terminal_transaction_id", &input.terminal_transaction_id),
        ("terminal_response_code", &input.terminal_response_code),
        ("terminal_status", &input.terminal_status),
        ("terminal_receipt", &input.terminal_receipt),
        ("terminal_card_pan", &input.terminal_card_pan),
        ("terminal_payment_system", &input.terminal_payment_system),
        ("terminal_merchant", &input.terminal_merchant),
    ] {
        if let Some(x) = val {
            obj.insert(key.into(), Value::String(x.clone()));
        }
    }
    if let Some(ts) = input.terminal_created_at {
        obj.insert(
            "terminal_created_at".into(),
            Value::String(ts.format("%Y-%m-%dT%H:%M:%S").to_string()),
        );
    }
    v
}

/// Синтетичний `ReceiptDto` локальної черги: НІЯКОГО натяку на успішну
/// фіскалізацію (`fiscal_status = "queued"`), `number` — локальний id.
fn queued_receipt_dto(input: &ReceiptCreateInput, client_uuid: &Uuid, local_id: i64) -> ReceiptDto {
    let items: Vec<ReceiptItemDto> = input
        .items
        .iter()
        .map(|it| ReceiptItemDto {
            product_id: it.product_id,
            name: it.name.clone(),
            quantity: f64_of(&it.quantity),
            price: f64_of(&it.price),
            tax_rate: it.tax_rate,
        })
        .collect();
    let total: f64 = items.iter().map(|it| it.quantity * it.price).sum();
    ReceiptDto {
        id: *client_uuid,
        number: local_id.to_string(),
        items,
        total: Some(total),
        payment_method: input.payment_method.clone(),
        created_at: Some(Utc::now().to_rfc3339()),
        cash_amount: input.cash_amount.as_deref().map(f64_of),
        card_amount: input.card_amount.as_deref().map(f64_of),
        change_amount: None,
        customer_id: input.customer_id,
        notes: input.notes.clone(),
        is_fiscal: input.is_fiscal,
        fiscal_status: "queued".to_string(),
        fiscal_number: None,
        fiscal_serial: None,
        fiscal_sent_at: None,
        fiscal_error: None,
        split_group_id: input.split_group_id,
        terminal_rrn: input.terminal_rrn.clone(),
        terminal_approval_code: input.terminal_approval_code.clone(),
        terminal_invoice_number: input.terminal_invoice_number.clone(),
        terminal_transaction_id: input.terminal_transaction_id.clone(),
        terminal_response_code: input.terminal_response_code.clone(),
        terminal_status: input.terminal_status.clone(),
        terminal_receipt: input.terminal_receipt.clone(),
        terminal_card_pan: input.terminal_card_pan.clone(),
        terminal_payment_system: input.terminal_payment_system.clone(),
        terminal_merchant: input.terminal_merchant.clone(),
        terminal_created_at: input
            .terminal_created_at
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
        fiscal_check_url: None,
    }
}

/// Позиції документа → JSON payload черги (`items[]` як читає
/// `stock::parse_items` і приймач `sync_receivers::parse_items`).
fn doc_items_json(items: &[DocItemInput]) -> Vec<Value> {
    items
        .iter()
        .map(|it| {
            let mut o = json!({
                "product_id": it.product_id,
                "quantity": num(&it.quantity),
            });
            if let Some(x) = &it.cost_price {
                o["cost_price"] = num(x);
            }
            if let Some(x) = &it.price {
                o["price"] = num(x);
            }
            o
        })
        .collect()
}

/// Спільна реалізація LocalOutbox-запису агрегата
/// (`transactions::enqueue_transaction`: агрегат + outbox + stock — атомарно).
async fn enqueue_doc(
    op: &'static str,
    kind: &'static str,
    payload: Value,
) -> Result<transactions::EnqueuedTransaction, PosError> {
    let payload_json = payload.to_string();
    with_conn(op, move |conn| {
        let store_id = resolve_store_id(conn)?;
        transactions::enqueue_transaction(conn, kind, &payload_json, &store_id)
            .map_err(|e| infra_err(op, e))
    })
    .await
}

// ─────────────────────────────────────────────────────────────────────────────
// PosService
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl PosService for OutboxPos {
    // ─── Чеки v2: LocalOutbox `receipt` / `return_receipt` ──────────────────

    async fn create_sale_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        self.enqueue_receipt("чек продажу", input, "sale").await
    }

    async fn create_return_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        self.enqueue_receipt("чек повернення", input, "return")
            .await
    }

    /// v1-чек (боргова семантика). Черга відтворює чек і stock-ефект
    /// (`receipt`+`items[]`), але НЕ боргову частину: приймач `/api/v1/sync/push`
    /// парсить payload тим самим `parse_receipt_create`, що v2 (`sync.rs:670`), і
    /// про `debt_payment`/`debtor_id` не знає, а сутностей `debtors`/
    /// `debtor_payments` у §11.1 немає (АНОМАЛІЯ у звіті). Тому чек ІЗ боргом
    /// не «втрачає борг тихо» — відмова з людським текстом.
    async fn create_receipt_v1(
        &self,
        input: &ReceiptV1CreateInput,
    ) -> Result<ReceiptV1Dto, PosError> {
        if input.debt_payment.is_some() || input.debtor_id.is_some() {
            return Err(unavailable(
                "чек із борговою семантикою (debt_payment/debtor_id)",
            ));
        }
        let client_uuid = Uuid::new_v4();
        let receipt_type = if input.is_return { "return" } else { "sale" };
        let items: Vec<Value> = input
            .items
            .iter()
            .map(|it| {
                json!({
                    "product_id": it.product_id,
                    "quantity": num(&it.quantity),
                    "price": num(&it.price),
                })
            })
            .collect();
        let payment_method = input
            .payment_method
            .clone()
            .unwrap_or_else(|| "cash".into());
        let payload = json!({
            "receipt_type": receipt_type,
            "items": items,
            "payment_method": payment_method,
            "notes": input.notes.clone(),
            "is_fiscal": false,
            "client_uuid": client_uuid,
        });
        let json = payload.to_string();
        let uuid_s = client_uuid.to_string();
        let enq = with_conn("чек v1", move |conn| {
            let store_id = resolve_store_id(conn)?;
            sync_push::enqueue_receipt_with_uuid(conn, &json, Some(&store_id), &uuid_s)
                .map_err(|e| infra_err("чек v1", e))
        })
        .await?;
        let now = Utc::now().to_rfc3339();
        let dto_items: Vec<ReceiptV1ItemDto> = input
            .items
            .iter()
            .map(|it| ReceiptV1ItemDto {
                id: Uuid::new_v4(),
                receipt_id: client_uuid,
                product_id: it.product_id,
                product_name: String::new(),
                product_barcode: None,
                quantity: it.quantity.clone(),
                price: it.price.clone(),
                total: it
                    .total
                    .clone()
                    .unwrap_or_else(|| dec_mul(&it.quantity, &it.price)),
                purchase_price: None,
                profit: None,
                vat_amount: None,
                created_at: now.clone(),
            })
            .collect();
        Ok(ReceiptV1Dto {
            id: client_uuid,
            receipt_number: enq.receipt_id.to_string(),
            receipt_type: receipt_type.to_string(),
            cashier_id: input.cashier_id.unwrap_or_else(Uuid::nil),
            total_amount: input.total_amount.clone(),
            paid_amount: input.paid_amount.clone(),
            change_amount: None,
            debtor_id: None,
            is_return: input.is_return,
            notes: input.notes.clone(),
            created_at: now,
            items: dto_items,
            total_profit: Value::Null,
            vat_amount: Value::Null,
            cashier_name: String::new(),
            payment_method: input.payment_method.clone(),
        })
    }

    // ─── Читання чеків — репліка (§10) ──────────────────────────────────────

    async fn get_receipt(&self, id: Uuid) -> Result<ReceiptDto, PosError> {
        self.inner.get_receipt(id).await
    }

    async fn list_receipts(&self, q: &ReceiptListQuery) -> Result<ReceiptListDto, PosError> {
        self.inner.list_receipts(q).await
    }

    async fn today_stats(&self) -> Result<ReceiptStatsDto, PosError> {
        self.inner.today_stats().await
    }

    async fn search_receipts(&self, q: &ReceiptSearchQuery) -> Result<ReceiptSearchDto, PosError> {
        self.inner.search_receipts(q).await
    }

    async fn recent_sales_by_product(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ProductRecentSalesDto>, PosError> {
        self.inner.recent_sales_by_product(query, limit).await
    }

    async fn returnable_quantity(&self, product_id: Uuid) -> Result<ReturnableQtyDto, PosError> {
        self.inner.returnable_quantity(product_id).await
    }

    async fn receipt_items(&self, receipt_id: Uuid) -> Result<Vec<ReceiptItemDetailDto>, PosError> {
        self.inner.receipt_items(receipt_id).await
    }

    async fn list_receipts_v1(&self, q: &ReceiptV1ListQuery) -> Result<ReceiptV1ListDto, PosError> {
        self.inner.list_receipts_v1(q).await
    }

    async fn get_receipt_v1(&self, id: Uuid) -> Result<ReceiptV1Dto, PosError> {
        self.inner.get_receipt_v1(id).await
    }

    async fn receipt_items_v1(&self, receipt_id: Uuid) -> Result<Vec<ReceiptV1ItemDto>, PosError> {
        self.inner.receipt_items_v1(receipt_id).await
    }

    async fn search_receipts_v1(
        &self,
        q: &ReceiptSearchQuery,
    ) -> Result<ReceiptV1SearchDto, PosError> {
        self.inner.search_receipts_v1(q).await
    }

    // ─── Робочі сесії — читання ─────────────────────────────────────────────

    async fn my_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<MySessionsDto, PosError> {
        self.inner.my_sessions(user_id, month, year).await
    }

    async fn work_report(&self, month: i64, year: i64) -> Result<WorkReportDto, PosError> {
        self.inner.work_report(month, year).await
    }

    async fn user_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<UserSessionsDto, PosError> {
        self.inner.user_sessions(user_id, month, year).await
    }

    // ─── Списання: LocalOutbox `write_off` ──────────────────────────────────

    async fn list_write_offs(&self, page: i64, size: i64) -> Result<WriteOffListDto, PosError> {
        self.inner.list_write_offs(page, size).await
    }

    async fn get_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError> {
        self.inner.get_write_off(id).await
    }

    async fn create_write_off(&self, input: &WriteOffCreateInput) -> Result<WriteOffDto, PosError> {
        let payload = json!({
            "reason": input.reason,
            "write_off_date": input.write_off_date.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "notes": input.notes,
            "items": doc_items_json(&input.items),
        });
        let out = enqueue_doc("списання", transactions::TYPE_WRITE_OFF, payload).await?;
        let now = Utc::now().to_rfc3339();
        let doc_id = uuid_of(&out.client_uuid);
        let items: Vec<WriteOffItemDto> = input
            .items
            .iter()
            .map(|it| {
                let cost = it.cost_price.clone().unwrap_or_else(|| "0".into());
                let price = it.price.clone().unwrap_or_else(|| "0".into());
                WriteOffItemDto {
                    id: Uuid::new_v4(),
                    write_off_id: doc_id,
                    product_id: it.product_id,
                    product_name: String::new(),
                    quantity: it.quantity.clone(),
                    cost_price: cost.clone(),
                    price: price.clone(),
                    total: dec_mul(&it.quantity, &price),
                    created_at: now.clone(),
                }
            })
            .collect();
        Ok(WriteOffDto {
            id: doc_id,
            number: input.number.clone().unwrap_or_else(|| out.id.to_string()),
            reason: input.reason.clone(),
            write_off_date: input.write_off_date.to_string(),
            notes: input.notes.clone(),
            status: QUEUED_STATUS.to_string(),
            total_amount: Some(
                items
                    .iter()
                    .fold(bigdecimal::BigDecimal::from(0), |acc, it| {
                        use std::str::FromStr;
                        acc + bigdecimal::BigDecimal::from_str(&it.total)
                            .unwrap_or_else(|_| bigdecimal::BigDecimal::from(0))
                    })
                    .to_string(),
            ),
            created_at: now.clone(),
            updated_at: now,
            items,
        })
    }

    /// Оновлення вже створеного документа: у `transactions.rs` немає типу
    /// «update», а приймач `sync.rs:860-900` знає лише `accept_*` (INSERT).
    /// Вигадувати «update через новий INSERT» = дубль документа на primary
    /// (подвійний stock-ефект) → явна відмова, АНОМАЛІЯ у звіті.
    async fn update_write_off(
        &self,
        _id: Uuid,
        _input: &WriteOffUpdateInput,
    ) -> Result<WriteOffDto, PosError> {
        Err(unavailable("редагування списання"))
    }

    async fn delete_write_off(&self, _id: Uuid) -> Result<(), PosError> {
        Err(unavailable("видалення списання"))
    }

    async fn confirm_write_off(&self, _id: Uuid) -> Result<WriteOffDto, PosError> {
        Err(unavailable("проведення списання"))
    }

    async fn list_write_off_reasons(&self) -> Result<WriteOffReasonsListDto, PosError> {
        self.inner.list_write_off_reasons().await
    }

    /// Сутність `write_off_reasons` відсутня в §11.1 (АНОМАЛІЯ) → відмова.
    async fn create_write_off_reason(&self, _name: &str) -> Result<WriteOffReasonItem, PosError> {
        Err(unavailable("створення причини списання"))
    }

    // ─── Переміщення: LocalOutbox `transfer` ────────────────────────────────

    async fn list_transfers(&self, page: i64, size: i64) -> Result<TransferListDto, PosError> {
        self.inner.list_transfers(page, size).await
    }

    async fn get_transfer(&self, id: Uuid) -> Result<TransferDto, PosError> {
        self.inner.get_transfer(id).await
    }

    async fn create_transfer(&self, input: &TransferCreateInput) -> Result<TransferDto, PosError> {
        // Ключі саме `from_store_id`/`to_store_id`: саме їх читає і локальний
        // stock-ефект (`transactions::transfer_side`), і приймач
        // (`sync_receivers::accept_transfer`). from_location/to_location —
        // ті самі uuid кас рядком (`sync_receivers.rs:22`).
        let payload = json!({
            "from_store_id": input.from_location,
            "to_store_id": input.to_location,
            "transfer_date": input.transfer_date.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "notes": input.notes,
            "items": doc_items_json(&input.items),
        });
        let out = enqueue_doc("переміщення", transactions::TYPE_TRANSFER, payload).await?;
        let now = Utc::now().to_rfc3339();
        let doc_id = uuid_of(&out.client_uuid);
        let items: Vec<TransferItemDto> = input
            .items
            .iter()
            .map(|it| TransferItemDto {
                id: Uuid::new_v4(),
                transfer_id: doc_id,
                product_id: it.product_id,
                quantity: it.quantity.clone(),
                cost_price: it.cost_price.clone().unwrap_or_else(|| "0".into()),
                price: it.price.clone().unwrap_or_else(|| "0".into()),
                created_at: now.clone(),
            })
            .collect();
        Ok(TransferDto {
            id: doc_id,
            number: input.number.clone().unwrap_or_else(|| out.id.to_string()),
            from_location: input.from_location.clone(),
            to_location: input.to_location.clone(),
            transfer_date: input.transfer_date.to_string(),
            status: QUEUED_STATUS.to_string(),
            notes: input.notes.clone(),
            created_at: now.clone(),
            updated_at: now,
            items,
        })
    }

    async fn update_transfer(
        &self,
        _id: Uuid,
        _input: &TransferUpdateInput,
    ) -> Result<TransferDto, PosError> {
        Err(unavailable("редагування переміщення"))
    }

    async fn delete_transfer(&self, _id: Uuid) -> Result<(), PosError> {
        Err(unavailable("видалення переміщення"))
    }

    async fn confirm_transfer(&self, _id: Uuid, _status: &str) -> Result<TransferDto, PosError> {
        Err(unavailable("проведення/скасування переміщення"))
    }

    // ─── Зміни ПРРО ─────────────────────────────────────────────────────────
    // Немає жодної write-точки в PG (`repositories/pos.rs:3555` — гілка ПРРО
    // не підключена, `close_shift` лише читає) → читання/паритет з primary.

    async fn list_shifts(&self, page: i64, size: i64) -> Result<ShiftListDto, PosError> {
        self.inner.list_shifts(page, size).await
    }

    async fn open_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        self.inner.open_shift(comment).await
    }

    async fn close_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        self.inner.close_shift(comment).await
    }

    // ─── Готівкові операції ─────────────────────────────────────────────────

    /// Касова операція (внесення/інкасація) — клас `LocalOutbox`
    /// (ADR-0007 §11.1, §11.6): агрегат `cash_ledger` (0006) + outbox-запис
    /// (`cash_operation`) + локальний баланс каси (`cash::apply_cash_delta`)
    /// в ОДНІЙ SQLite-транзакції. Порожній `user_name`: таблиці `users` на
    /// вузлі немає (імʼя резолвить primary у своїй відповіді / пулі).
    async fn create_cash_operation(
        &self,
        store_id: Uuid,
        user_id: Uuid,
        input: &CashOperationCreateInput,
    ) -> Result<CashOperationDto, PosError> {
        let payload = json!({
            "store_id": store_id.to_string(),
            "user_id": user_id.to_string(),
            "operation_type": input.operation_type.as_str(),
            "cash_type": input.cash_type.as_str(),
            "amount": input.amount.to_string(),
            "comment": input.comment,
        });
        let out = enqueue_doc(
            "касова операція",
            transactions::TYPE_CASH_OPERATION,
            payload,
        )
        .await?;
        Ok(CashOperationDto {
            id: uuid_of(&out.client_uuid),
            store_id,
            user_id,
            user_name: String::new(),
            operation_type: input.operation_type,
            cash_type: input.cash_type,
            amount: input.amount.clone(),
            comment: input.comment.clone(),
            created_at: Utc::now().naive_utc(),
        })
    }

    async fn list_cash_operations(
        &self,
        store_id: Uuid,
    ) -> Result<CashOperationsListDto, PosError> {
        self.inner.list_cash_operations(store_id).await
    }
}

/// Маркер «документ у черзі, primary ще не створив» (202 Accepted у хендлері).
pub const QUEUED_STATUS: &str = "queued";

impl OutboxPos {
    /// Спільний шлях чеків v2: JSON → атомарний запис SQLite (чек+outbox+stock).
    async fn enqueue_receipt(
        &self,
        op: &'static str,
        input: &ReceiptCreateInput,
        receipt_type: &str,
    ) -> Result<ReceiptDto, PosError> {
        let client_uuid = Uuid::new_v4();
        let json = receipt_json(input, receipt_type, &client_uuid).to_string();
        let uuid_s = client_uuid.to_string();
        let enq = with_conn(op, move |conn| {
            let store_id = resolve_store_id(conn)?;
            // Той самий контур, що офлайн-команда каси
            // (`commands.rs::save_receipt_offline`): заборона продажу товару,
            // позначеного видаленим у локальному довіднику (дизайн 6.2), +
            // снапшоти назв/цін на момент продажу. Без цих двох викликів
            // локальний агрегат у черзі відрізнявся б від агрегата
            // Tauri-шляху (та сама черга — та сама семантика, §11.5).
            snapshots::validate_sale_allowed(conn, &json).map_err(PosError::BadRequest)?;
            let enriched = snapshots::enrich_receipt_with_snapshots(conn, &json)
                .map_err(|e| infra_err(op, e))?;
            sync_push::enqueue_receipt_with_uuid(conn, &enriched, Some(&store_id), &uuid_s)
                .map_err(|e| infra_err(op, e))
        })
        .await?;
        Ok(queued_receipt_dto(input, &client_uuid, enq.receipt_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `receipt_json` мусить бути придатним І локальній черзі
    /// (`sync_push::outbox_type_of` + `stock::parse_items`), І приймачу
    /// primary (`sync.rs:670` парсить тим самим `parse_receipt_create`).
    #[test]
    fn receipt_json_carries_type_items_and_numbers() {
        let v = receipt_json(&input(), "sale", &Uuid::nil());
        assert_eq!(v["receipt_type"], "sale");
        assert_eq!(v["items"][0]["quantity"], 2.0);
        assert_eq!(v["items"][0]["price"], 10.5);
        assert_eq!(v["items"][0]["tax_rate"], 20);
        assert_eq!(v["items"][0]["name"], "Товар");
        assert_eq!(v["cash_amount"], 21.0);
        assert_eq!(v["is_fiscal"], true);
        assert_eq!(sync_push::outbox_type_of(&v.to_string()), "receipt");

        let ret = receipt_json(&input(), "return", &Uuid::nil());
        assert_eq!(
            sync_push::outbox_type_of(&ret.to_string()),
            "return_receipt"
        );
        // Локальний stock-ефект бачить позицію (product_id + quantity > 0).
        assert_eq!(crate::offline::stock::parse_items(&ret).len(), 1);
    }

    /// Синтетичний DTO черги НЕ видає себе за успішно фіскалізований чек.
    #[test]
    fn queued_dto_never_claims_fiscal_success() {
        let uuid = Uuid::new_v4();
        let dto = queued_receipt_dto(&input(), &uuid, 17);
        assert_eq!(dto.id, uuid);
        assert_eq!(dto.number, "17");
        assert_eq!(dto.fiscal_status, QUEUED_STATUS);
        assert!(dto.fiscal_number.is_none());
        assert!(dto.fiscal_serial.is_none());
        assert!(dto.fiscal_check_url.is_none());
        assert_eq!(dto.total, Some(21.0));
        assert_eq!(dto.items.len(), 1);
    }

    /// store_id: ні SQLite-налаштування, ні env → `Validation` (422), НЕ 500.
    #[test]
    fn missing_store_id_is_validation_not_infrastructure() {
        let conn = rusqlite::Connection::open_in_memory().expect("sqlite");
        conn.execute(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .expect("settings");
        std::env::remove_var("TORGASHKA_STORE_ID");
        let err = resolve_store_id(&conn).expect_err("мусить бути помилка");
        assert!(matches!(err, PosError::Validation(_)), "{err:?}");
        std::env::set_var("TORGASHKA_STORE_ID", "store-from-env");
        assert_eq!(resolve_store_id(&conn).expect("env"), "store-from-env");
        std::env::remove_var("TORGASHKA_STORE_ID");
    }

    fn input() -> ReceiptCreateInput {
        ReceiptCreateInput {
            items: vec![torgashka_domain::ReceiptItemInput {
                product_id: Uuid::new_v4(),
                name: "Товар".into(),
                quantity: "2".into(),
                price: "10.5".into(),
                tax_rate: 20,
            }],
            payment_method: "cash".into(),
            cash_amount: Some("21".into()),
            card_amount: None,
            customer_id: None,
            cashier_id: None,
            notes: "тест".into(),
            terminal_rrn: None,
            terminal_approval_code: None,
            terminal_invoice_number: None,
            terminal_transaction_id: None,
            terminal_response_code: None,
            terminal_status: None,
            terminal_receipt: None,
            terminal_card_pan: None,
            terminal_payment_system: None,
            terminal_merchant: None,
            terminal_created_at: None,
            is_fiscal: true,
            split_group_id: None,
            client_uuid: None,
            created_at: None,
        }
    }
}
