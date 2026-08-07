//! Повернення постачальнику (етап 8 — група 4): CRUD, статуси, зв'язок з
//! інвойсами, валідації.
//!
//! 1:1 з Python: backend/app/api/v1/return_invoices.py (7 роутів, 428 рядків) +
//! app/domain/services/document_service.py (confirm/cancel_return_invoice) +
//! app/schemas/return_invoice.py.
//!
//! Формат відповіді — Pydantic v2 (Decimal → JSON-рядок, datetime без Z,
//! supplier_name/null, exchange_invoice brief при обміні).

use chrono::NaiveDateTime;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Десеріалізація числа АБО рядка → рядок (Python Pydantic Decimal приймає обидва).
fn de_num_str<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Ok(other.to_string()),
    }
}

/// Option<число|рядок|null> → Option<String>.
fn de_opt_num_str<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        Value::Number(n) => Ok(Some(n.to_string())),
        other => Ok(Some(other.to_string())),
    }
}

/// Помилки повернень (1:1 з HTTP-статусами Python).
#[derive(Debug, thiserror::Error)]
pub enum ReturnInvoicesError {
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

// ─── DTO відповідей (Pydantic v1 ReturnInvoiceResponse/items/briefs) ───────

/// Скорочена інформація про товар (Python ProductBrief).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProductBriefDto {
    pub id: Uuid,
    pub title: String,
    pub barcode: Option<String>,
}

/// Позиція повернення (Python ReturnInvoiceItemResponse).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReturnInvoiceItemDto {
    pub id: Uuid,
    pub return_invoice_id: Uuid,
    pub product_id: Uuid,
    pub product: Option<ProductBriefDto>,
    pub quantity: String,
    pub price: String,
    pub cost_price: Option<String>,
    pub markup_percent: Option<String>,
    pub total: String,
    pub created_at: String,
}

/// Позиція прибуткової накладної при обміні (Python ExchangeInvoiceItemBrief).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExchangeInvoiceItemBriefDto {
    pub id: Uuid,
    pub product_id: Uuid,
    pub product: Option<ProductBriefDto>,
    pub quantity: String,
    pub price: String,
    pub total: String,
}

/// Прибуткова накладна при обміні (Python ExchangeInvoiceBrief).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExchangeInvoiceBriefDto {
    pub id: Uuid,
    pub number: String,
    pub total_amount: Option<String>,
    pub items: Vec<ExchangeInvoiceItemBriefDto>,
}

/// Повернення (Python ReturnInvoiceResponse).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReturnInvoiceDto {
    pub id: Uuid,
    pub number: String,
    pub supplier_id: Uuid,
    pub supplier_name: Option<String>,
    pub return_date: String,
    pub status: String,
    pub return_action: String,
    pub is_fiscal: bool,
    pub notes: Option<String>,
    pub total_amount: Option<String>,
    pub exchange_invoice_id: Option<Uuid>,
    pub exchange_invoice: Option<ExchangeInvoiceBriefDto>,
    pub source_invoice_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
    pub items: Vec<ReturnInvoiceItemDto>,
}

/// Список з пагінацією (Python list_return_invoices).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReturnInvoiceListDto {
    pub items: Vec<ReturnInvoiceDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

// ─── Вхідні DTO (Pydantic Create/Update/ConfirmRequest) ────────────────────

/// Позиція створення/оновлення (Python ReturnInvoiceItemCreate).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ReturnInvoiceItemCreateInput {
    pub product_id: Uuid,
    #[serde(deserialize_with = "de_num_str")]
    pub quantity: String,
    #[serde(deserialize_with = "de_num_str")]
    pub price: String,
    #[serde(deserialize_with = "de_num_str")]
    pub total: String,
    #[serde(default, deserialize_with = "de_opt_num_str")]
    pub cost_price: Option<String>,
}

/// Позиція обміну (Python ExchangeItemCreate).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ExchangeItemCreateInput {
    pub product_id: Uuid,
    #[serde(deserialize_with = "de_num_str")]
    pub quantity: String,
    #[serde(deserialize_with = "de_num_str")]
    pub price: String,
    #[serde(deserialize_with = "de_num_str")]
    pub total: String,
}

/// Створення (Python ReturnInvoiceCreate).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ReturnInvoiceCreateInput {
    pub number: Option<String>,
    pub supplier_id: Uuid,
    pub return_date: NaiveDateTime,
    #[serde(default = "default_return_action")]
    pub return_action: String,
    #[serde(default)]
    pub is_fiscal: bool,
    pub notes: Option<String>,
    pub total_amount: Option<String>,
    #[serde(default)]
    pub items: Vec<ReturnInvoiceItemCreateInput>,
    pub exchange_items: Option<Vec<ExchangeItemCreateInput>>,
    pub source_invoice_id: Option<Uuid>,
}

fn default_return_action() -> String {
    "deduct_from_debt".to_string()
}

/// Оновлення (Python ReturnInvoiceUpdate; exchange_items ігнорується Python).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ReturnInvoiceUpdateInput {
    pub number: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub return_date: Option<NaiveDateTime>,
    pub return_action: Option<String>,
    pub is_fiscal: Option<bool>,
    pub notes: Option<String>,
    pub total_amount: Option<String>,
    pub items: Option<Vec<ReturnInvoiceItemCreateInput>>,
    pub source_invoice_id: Option<Uuid>,
}

/// Підтвердження (Python ReturnInvoiceConfirmRequest).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ReturnInvoiceConfirmInput {
    pub status: String,
    pub exchange_items: Option<Vec<ExchangeItemCreateInput>>,
}

// ─── Контракт сервісу ───────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait ReturnInvoicesService: Send + Sync {
    async fn list(&self, page: i64, size: i64)
        -> Result<ReturnInvoiceListDto, ReturnInvoicesError>;
    async fn get(&self, id: Uuid) -> Result<ReturnInvoiceDto, ReturnInvoicesError>;
    async fn create(
        &self,
        input: &ReturnInvoiceCreateInput,
        user_id: Uuid,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError>;
    async fn update(
        &self,
        id: Uuid,
        input: &ReturnInvoiceUpdateInput,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError>;
    async fn delete(&self, id: Uuid) -> Result<(), ReturnInvoicesError>;
    async fn confirm(
        &self,
        id: Uuid,
        input: &ReturnInvoiceConfirmInput,
        user_id: Uuid,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError>;
    async fn cancel(&self, id: Uuid) -> Result<ReturnInvoiceDto, ReturnInvoicesError>;
}
