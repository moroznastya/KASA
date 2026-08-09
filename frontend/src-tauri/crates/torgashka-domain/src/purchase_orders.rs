//! Замовлення постачальнику (етап 8 — група 5): CRUD, confirm/cancel.
//!
//! 1:1 з Python: backend/app/api/v1/purchase_orders.py (6 роутів, 416 рядків)
//! + schemas/purchase_order.py + models/purchase_order.py.
//!
//! Статуси: draft / confirmed / cancelled. При підтвердженні (confirmed)
//! автоматично створюється прибуткова накладна (Invoice DRAFT) з копією
//! позицій і зв'язком invoice_id; при скасуванні — просто статус CANCELLED.
//!
//! Грошові поля — String (Decimal), як Python-еталон серіалізує Decimal у
//! JSON-рядок.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Помилки замовлень (1:1 з HTTP-статусами Python).
#[derive(Debug, thiserror::Error)]
pub enum PurchaseOrdersError {
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

// ─── DTO відповіді (Python schemas/purchase_order.py) ──────────────────────

/// Коротка інформація про товар у позиції (Python ProductBrief).
#[derive(Debug, Clone, Serialize)]
pub struct ProductBriefDto {
    pub id: Uuid,
    pub title: String,
    pub barcode: Option<String>,
}

/// Позиція замовлення (Python PurchaseOrderItemResponse — Decimal-рядки).
#[derive(Debug, Clone, Serialize)]
pub struct PurchaseOrderItemDto {
    pub id: Uuid,
    pub purchase_order_id: Uuid,
    pub product_id: Uuid,
    pub product: Option<ProductBriefDto>,
    pub quantity: String,
    pub price: String,
    pub total: String,
    pub created_at: NaiveDateTime,
}

/// Прибуткова накладна, створена при підтвердженні (Python InvoiceBrief).
#[derive(Debug, Clone, Serialize)]
pub struct InvoiceBriefDto {
    pub id: Uuid,
    pub number: String,
    pub total_amount: Option<String>,
}

/// Замовлення (Python PurchaseOrderResponse).
#[derive(Debug, Clone, Serialize)]
pub struct PurchaseOrderDto {
    pub id: Uuid,
    pub number: String,
    pub supplier_id: Uuid,
    pub supplier_name: Option<String>,
    #[serde(deserialize_with = "crate::datetime::de_naive_dt")]
    pub order_date: NaiveDateTime,
    #[serde(default, deserialize_with = "crate::datetime::de_opt_naive_dt")]
    pub expected_date: Option<NaiveDateTime>,
    pub status: String,
    pub is_fiscal: bool,
    pub notes: Option<String>,
    pub total_amount: Option<String>,
    pub invoice_id: Option<Uuid>,
    pub invoice: Option<InvoiceBriefDto>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    #[serde(default)]
    pub items: Vec<PurchaseOrderItemDto>,
}

/// Список (Python: {items, total, page, page_size, pages}).
#[derive(Debug, Clone, Serialize)]
pub struct PurchaseOrderListDto {
    pub items: Vec<PurchaseOrderDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

// ─── Вхідні DTO (Python schemas/purchase_order.py) ─────────────────────────

/// Позиція при створенні/оновленні (Python PurchaseOrderItemCreate).
#[derive(Debug, Clone, Deserialize)]
pub struct PurchaseOrderItemInput {
    pub product_id: Uuid,
    #[serde(deserialize_with = "de_num_str")]
    pub quantity: String,
    #[serde(deserialize_with = "de_num_str")]
    pub price: String,
    #[serde(deserialize_with = "de_num_str")]
    pub total: String,
}

/// Створення (Python PurchaseOrderCreate).
#[derive(Debug, Clone, Deserialize)]
pub struct PurchaseOrderCreateInput {
    pub number: Option<String>,
    pub supplier_id: Uuid,
    #[serde(deserialize_with = "crate::datetime::de_naive_dt")]
    pub order_date: NaiveDateTime,
    #[serde(default, deserialize_with = "crate::datetime::de_opt_naive_dt")]
    pub expected_date: Option<NaiveDateTime>,
    #[serde(default)]
    pub is_fiscal: bool,
    pub notes: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num_str")]
    pub total_amount: Option<String>,
    #[serde(default)]
    pub items: Vec<PurchaseOrderItemInput>,
}

/// Оновлення (Python PurchaseOrderUpdate — всі поля опціональні).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PurchaseOrderUpdateInput {
    pub number: Option<String>,
    pub supplier_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::datetime::de_opt_naive_dt")]
    pub order_date: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "crate::datetime::de_opt_naive_dt")]
    pub expected_date: Option<NaiveDateTime>,
    pub is_fiscal: Option<bool>,
    pub notes: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num_str")]
    pub total_amount: Option<String>,
    pub items: Option<Vec<PurchaseOrderItemInput>>,
}

/// Підтвердження/скасування (Python PurchaseOrderConfirmRequest.status).
#[derive(Debug, Clone, Deserialize)]
pub struct PurchaseOrderConfirmInput {
    pub status: String,
}

// ─── Контракт сервісу (Python api/v1/purchase_orders.py) ───────────────────

/// Сервіс замовлень постачальнику.
#[async_trait::async_trait]
pub trait PurchaseOrdersService: Send + Sync {
    async fn list(&self, page: i64, size: i64)
        -> Result<PurchaseOrderListDto, PurchaseOrdersError>;
    async fn get(&self, id: Uuid) -> Result<PurchaseOrderDto, PurchaseOrdersError>;
    async fn create(
        &self,
        input: &PurchaseOrderCreateInput,
        user_id: Uuid,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError>;
    async fn update(
        &self,
        id: Uuid,
        input: &PurchaseOrderUpdateInput,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError>;
    async fn delete(&self, id: Uuid) -> Result<(), PurchaseOrdersError>;
    async fn confirm(
        &self,
        id: Uuid,
        status: &str,
        user_id: Uuid,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError>;
}
