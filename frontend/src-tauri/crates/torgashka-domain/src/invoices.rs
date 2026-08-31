//! Інвойси (етап 8 — група 3): CRUD, payment, confirm/cancel, price-changes,
//! print-items — v1 + v2.
//!
//! 1:1 з Python:
//!   - v1: backend/app/api/v1/invoices.py (9 роутів, 748 рядків) — детальна
//!     відповідь (supplier_name, product brief, Decimal-рядки)
//!   - v2: backend/app/api/v2/invoices.py (10 роутів, 362 рядки) — компактна
//!     відповідь (float, tax_rate, name) + use cases
//!
//! АНОМАЛІЯ PYTHON (зафіксовано 2026-08-07): v2 create/confirm/cancel у Python
//! кидають 500 (UnmappedInstanceError / AttributeError — use case змішує domain
//! entity і ORM-модель). Rust реалізує ЗАДУМАНУ робочу семантику цих роутів;
//! роути, що працюють у Python (list/get/update/delete/payment/price/print) —
//! 1:1.
//!
//! Грошові поля v1 — String (Decimal), як Python-еталон серіалізує Decimal у
//! JSON-рядок; v2 — f64 (Python-еталон віддає float).

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Помилки інвойсів (1:1 з HTTP-статусами Python).
#[derive(Debug, thiserror::Error)]
pub enum InvoicesError {
    /// 404.
    #[error("{0}")]
    NotFound(String),
    /// 400 — бізнес-валідація.
    #[error("{0}")]
    BadRequest(String),
    /// 500 — помилка БД.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

// ─── v2: компактні DTO (Python api/v2/invoices.py + invoice_use_cases) ──────

/// Позиція v2-відповіді (Python InvoiceItemResponse: quantity float,
/// price float, tax_rate int, name str).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvoiceItemV2Dto {
    pub product_id: Uuid,
    pub quantity: f64,
    pub price: f64,
    pub tax_rate: i32,
    pub name: String,
}

/// Накладна v2 (Python InvoiceResponse).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvoiceV2Dto {
    pub id: Uuid,
    pub number: String,
    pub supplier_id: Uuid,
    #[serde(default)]
    pub items: Vec<InvoiceItemV2Dto>,
    pub total: Option<f64>,
    #[serde(default = "default_status")]
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
    pub confirmed_at: Option<NaiveDateTime>,
    #[serde(default)]
    pub notes: String,
}

fn default_status() -> String {
    "draft".to_string()
}

/// Вхідні дані позиції v2 (Python InvoiceItemRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct InvoiceItemV2Input {
    pub product_id: Uuid,
    pub quantity: f64,
    pub price: f64,
    #[serde(default = "default_tax_rate")]
    pub tax_rate: i32,
    #[serde(default)]
    pub name: String,
}

fn default_tax_rate() -> i32 {
    20
}

/// Десеріалізація числа АБО рядка → рядок (Python Pydantic Decimal приймає обидва).
fn de_num_str<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Ok(other.to_string()),
    }
}

/// Option<число|рядок|null> → Option<String>.
fn de_opt_num_str<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => Ok(Some(s)),
        serde_json::Value::Number(n) => Ok(Some(n.to_string())),
        other => Ok(Some(other.to_string())),
    }
}

/// Створення v2 (Python CreateInvoiceRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct InvoiceCreateV2Input {
    pub number: String,
    pub supplier_id: Uuid,
    #[serde(default)]
    pub items: Vec<InvoiceItemV2Input>,
    #[serde(default)]
    pub notes: String,
}

/// Оновлення v2 (Python UpdateInvoiceRequest).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct InvoiceUpdateV2Input {
    pub number: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub notes: Option<String>,
    pub is_fiscal: Option<bool>,
    #[serde(default, deserialize_with = "crate::datetime::de_opt_naive_dt")]
    pub invoice_date: Option<NaiveDateTime>,
    /// Якщо передано — повністю замінює позиції.
    pub items: Option<Vec<InvoiceItemV2Input>>,
}

/// Список v2 (Python InvoiceListResponse {items, total, page, size}).
#[derive(Debug, Clone, Serialize)]
pub struct InvoiceV2ListDto {
    pub items: Vec<InvoiceV2Dto>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

/// Payment-info v2 (Python InvoicePaymentInfoResponse — float).
#[derive(Debug, Clone, Serialize)]
pub struct InvoicePaymentInfoV2Dto {
    pub invoice_id: Uuid,
    pub invoice_number: String,
    #[serde(default, deserialize_with = "crate::datetime::de_opt_naive_dt")]
    pub invoice_date: Option<NaiveDateTime>,
    pub total_amount: f64,
    pub paid_amount: f64,
    pub remaining: f64,
}

// ─── v1: детальні DTO (Python api/v1/invoices.py + schemas/invoice.py) ─────

/// Коротка інформація про товар у позиції (Python ProductBrief).
#[derive(Debug, Clone, Serialize)]
pub struct ProductBriefV1Dto {
    pub id: Uuid,
    pub title: String,
    pub barcode: Option<String>,
    pub price: Option<String>,
    pub markup: Option<String>,
    pub cost_price: Option<String>,
}

/// Позиція v1-відповіді (Python InvoiceItemResponse — Decimal-рядки).
#[derive(Debug, Clone, Serialize)]
pub struct InvoiceItemV1Dto {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub product_id: Uuid,
    pub product: Option<ProductBriefV1Dto>,
    pub quantity: String,
    pub price: String,
    pub total: String,
    pub cost_price: Option<String>,
    pub markup_percent: Option<String>,
    pub previous_price: Option<String>,
    pub created_at: NaiveDateTime,
}

/// Накладна v1 (Python InvoiceResponse).
#[derive(Debug, Clone, Serialize)]
pub struct InvoiceV1Dto {
    pub id: Uuid,
    pub number: String,
    pub supplier_id: Uuid,
    pub supplier_name: Option<String>,
    #[serde(deserialize_with = "crate::datetime::de_naive_dt")]
    pub invoice_date: NaiveDateTime,
    pub status: String,
    pub payment_method: Option<String>,
    pub is_fiscal: bool,
    pub notes: Option<String>,
    pub total_amount: Option<String>,
    /// Сплачена сума (supplier_ledger payment/return) — для картки накладної.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paid_amount: Option<String>,
    /// Залишок до оплати (total − paid).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    #[serde(default)]
    pub items: Vec<InvoiceItemV1Dto>,
}

/// Список v1 (Python: {items, total, page, page_size, pages}).
#[derive(Debug, Clone, Serialize)]
pub struct InvoiceV1ListDto {
    pub items: Vec<InvoiceV1Dto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

/// Позиція при створенні/оновленні v1 (Python InvoiceItemCreate).
#[derive(Debug, Clone, Deserialize)]
pub struct InvoiceItemV1Input {
    pub product_id: Uuid,
    #[serde(deserialize_with = "de_num_str")]
    pub quantity: String,
    #[serde(deserialize_with = "de_num_str")]
    pub price: String,
    #[serde(deserialize_with = "de_num_str")]
    pub total: String,
    #[serde(default, deserialize_with = "de_opt_num_str")]
    pub cost_price: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num_str")]
    pub markup_percent: Option<String>,
}

/// Створення v1 (Python InvoiceCreate).
#[derive(Debug, Clone, Deserialize)]
pub struct InvoiceCreateV1Input {
    pub number: Option<String>,
    pub supplier_id: Uuid,
    #[serde(deserialize_with = "crate::datetime::de_naive_dt")]
    pub invoice_date: NaiveDateTime,
    pub payment_method: Option<String>,
    #[serde(default)]
    pub is_fiscal: bool,
    pub notes: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num_str")]
    pub total_amount: Option<String>,
    #[serde(default)]
    pub items: Vec<InvoiceItemV1Input>,
}

/// Оновлення v1 (Python InvoiceUpdate).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct InvoiceUpdateV1Input {
    pub number: Option<String>,
    pub supplier_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::datetime::de_opt_naive_dt")]
    pub invoice_date: Option<NaiveDateTime>,
    pub payment_method: Option<String>,
    pub is_fiscal: Option<bool>,
    pub notes: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num_str")]
    pub total_amount: Option<String>,
    pub items: Option<Vec<InvoiceItemV1Input>>,
}

/// Payment-info v1 (Python InvoicePaymentInfo — Decimal-рядки).
#[derive(Debug, Clone, Serialize)]
pub struct InvoicePaymentInfoV1Dto {
    pub invoice_id: Uuid,
    pub invoice_number: String,
    #[serde(deserialize_with = "crate::datetime::de_naive_dt")]
    pub invoice_date: NaiveDateTime,
    pub total_amount: String,
    pub paid_amount: String,
    pub remaining: String,
}

// ─── Спільне: price-changes, print ──────────────────────────────────────────

/// Зміна ціни товару в накладній (Python PriceChangeItem — рядки).
#[derive(Debug, Clone, Serialize)]
pub struct PriceChangeItemDto {
    pub product_id: Uuid,
    pub title: String,
    pub barcode: Option<String>,
    pub article: Option<String>,
    pub invoice_price: String,
    pub current_price: String,
    pub changed: bool,
    pub difference: String,
}

/// Запит друку цінників/етикеток (Python InvoicePrintRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct InvoicePrintRequest {
    pub print_type: String,
    #[serde(default)]
    pub only_changed: bool,
    pub template_id: Uuid,
    #[serde(default = "default_width")]
    pub width_mm: f64,
    #[serde(default = "default_height")]
    pub height_mm: f64,
    #[serde(default = "default_gap")]
    pub gap_mm: f64,
    #[serde(default = "default_margin")]
    pub margin_mm: f64,
    #[serde(default = "default_barcode_type")]
    pub barcode_type: String,
    #[serde(default = "default_barcode_height")]
    pub barcode_height_mm: f64,
    #[serde(default = "default_print_mode")]
    pub print_mode: String,
}

fn default_width() -> f64 {
    40.0
}
fn default_height() -> f64 {
    25.0
}
fn default_gap() -> f64 {
    3.0
}
fn default_margin() -> f64 {
    10.0
}
fn default_barcode_type() -> String {
    "code128".to_string()
}
fn default_barcode_height() -> f64 {
    12.0
}
fn default_print_mode() -> String {
    "system".to_string()
}

/// Відповідь друку (Python InvoicePrintResponse).
#[derive(Debug, Clone, Serialize)]
pub struct InvoicePrintDto {
    pub html: String,
    pub total_labels: i64,
    pub total_pages: Option<i64>,
    pub changed_count: i64,
    pub total_count: i64,
}

// ─── Контракти сервісів ─────────────────────────────────────────────────────

/// v1-сервіс інвойсів (Python api/v1/invoices.py).
#[async_trait::async_trait]
pub trait InvoicesV1Service: Send + Sync {
    async fn list_v1(
        &self,
        supplier_id: Option<Uuid>,
        page: i64,
        size: i64,
    ) -> Result<InvoiceV1ListDto, InvoicesError>;
    async fn get_v1(&self, id: Uuid) -> Result<InvoiceV1Dto, InvoicesError>;
    async fn create_v1(
        &self,
        input: &InvoiceCreateV1Input,
        user_id: Uuid,
    ) -> Result<InvoiceV1Dto, InvoicesError>;
    async fn update_v1(
        &self,
        id: Uuid,
        input: &InvoiceUpdateV1Input,
    ) -> Result<InvoiceV1Dto, InvoicesError>;
    async fn delete_v1(&self, id: Uuid) -> Result<(), InvoicesError>;
    async fn payment_info_v1(&self, id: Uuid) -> Result<InvoicePaymentInfoV1Dto, InvoicesError>;
    /// confirm/cancel v1 (Python POST /{id}/confirm {status}).
    async fn confirm_v1(&self, id: Uuid, status: &str) -> Result<InvoiceV1Dto, InvoicesError>;
    async fn price_changes(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError>;
    async fn print_items(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError>;
}

/// v2-сервіс інвойсів (Python api/v2/invoices.py + InvoiceUseCases).
#[async_trait::async_trait]
#[allow(clippy::too_many_arguments)]
pub trait InvoicesV2Service: Send + Sync {
    async fn list_v2(
        &self,
        search: Option<String>,
        supplier_id: Option<Uuid>,
        status: Option<String>,
        date_from: Option<NaiveDateTime>,
        date_to: Option<NaiveDateTime>,
        page: i64,
        size: i64,
    ) -> Result<InvoiceV2ListDto, InvoicesError>;
    async fn get_v2(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError>;
    async fn create_v2(&self, input: &InvoiceCreateV2Input) -> Result<InvoiceV2Dto, InvoicesError>;
    async fn confirm_v2(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError>;
    async fn update_v2(
        &self,
        id: Uuid,
        input: &InvoiceUpdateV2Input,
    ) -> Result<InvoiceV2Dto, InvoicesError>;
    async fn delete_v2(&self, id: Uuid) -> Result<(), InvoicesError>;
    async fn payment_info_v2(&self, id: Uuid) -> Result<InvoicePaymentInfoV2Dto, InvoicesError>;
    async fn price_changes_v2(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError>;
    async fn print_items_v2(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError>;
    async fn cancel_v2(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError>;
}
