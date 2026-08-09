//! Боржники (етап 8 — група 1): DTO, помилки, контракт репозиторію.
//!
//! 1:1 з Python v1/debtors.py + моделі Debtor/DebtorPayment:
//!   - GET  /debtors/search?query=&limit=      — пошук за ім'ям (ilike)
//!   - GET  /debtors?page=&size=               — список з пагінацією (sort total_debt DESC)
//!   - POST /debtors                           — створення (201)
//!   - GET  /debtors/{id}                      — деталі
//!   - PUT  /debtors/{id}                      — оновлення
//!   - POST /debtors/{id}/pay                  — погашення боргу (при 0 → видалення боржника)
//!   - GET  /debtors/{id}/receipts             — чеки боржника (v1 ReceiptResponse)
//!   - GET  /debtors/{id}/payments             — історія оплат
//!
//! Грошові поля — String (Python Pydantic Decimal → JSON-рядок зі scale БД:
//! `total_debt: "68.00"`). Суми порівнюються як scaled2 i64 (копійки).

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Помилки боржників (1:1 з HTTP-статусами Python).
#[derive(Debug, thiserror::Error)]
pub enum DebtorError {
    /// 404 — боржника не знайдено.
    #[error("Боржника з ID '{0}' не знайдено")]
    NotFound(Uuid),
    /// 400 — бізнес-валідація (сума <= 0, немає боргу, перевищення суми).
    #[error("{0}")]
    BadRequest(String),
    /// 422 — Pydantic-валідація (name порожнє, amount > 2 знаки тощо).
    #[error("валідація: {0}")]
    Validation(serde_json::Value),
    /// 500 — помилка БД.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

/// Створення боржника.
#[derive(Debug, Clone, Deserialize)]
pub struct DebtorCreateInput {
    pub name: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
}

/// Оновлення боржника (всі поля опційні).
#[derive(Debug, Clone, Deserialize)]
pub struct DebtorUpdateInput {
    pub name: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
}

/// Запит на погашення боргу.
#[derive(Debug, Clone, Deserialize)]
pub struct DebtorPayInput {
    /// Decimal-рядок (scale <= 2), як приходить у JSON.
    pub amount: String,
    pub payment_method: Option<String>,
}

/// Параметри пошуку боржників.
#[derive(Debug, Clone)]
pub struct DebtorSearchQuery {
    pub query: String,
    pub limit: i64,
}

/// Відповідь боржника (1:1 Python DebtorResponse).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DebtorDto {
    pub id: Uuid,
    pub name: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
    /// Decimal-рядок зі scale БД: "68.00".
    pub total_debt: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Список боржників (1:1 Python GET /debtors).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DebtorListDto {
    pub items: Vec<DebtorDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

/// Оплата боргу (1:1 Python DebtorPaymentResponse).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DebtorPaymentDto {
    pub id: Uuid,
    pub debtor_id: Uuid,
    /// Decimal-рядок: "10.00".
    pub amount: String,
    pub payment_method: Option<String>,
    pub created_at: NaiveDateTime,
}

/// Позиція чеку боржника (1:1 Python ReceiptItemResponse v1).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DebtorReceiptItemDto {
    pub id: Uuid,
    pub receipt_id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    /// Python v1 завжди повертає None (модель не має такого атрибута).
    pub product_barcode: Option<String>,
    /// Decimal-рядок (scale 3): "1.000".
    pub quantity: String,
    /// Decimal-рядок (scale 2): "40.00".
    pub price: String,
    pub total: String,
    pub purchase_price: Option<String>,
    /// Python v1 — завжди None (колонки немає).
    pub profit: Option<String>,
    pub vat_amount: Option<String>,
    pub created_at: NaiveDateTime,
}

/// Чек боржника (1:1 Python ReceiptResponse v1).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DebtorReceiptDto {
    pub id: Uuid,
    pub receipt_number: String,
    /// lowercase: "sale" | "return".
    pub receipt_type: String,
    pub cashier_id: Uuid,
    pub total_amount: String,
    pub paid_amount: Option<String>,
    pub change_amount: Option<String>,
    pub debtor_id: Option<Uuid>,
    pub is_return: bool,
    pub notes: Option<String>,
    pub created_at: NaiveDateTime,
    pub items: Vec<DebtorReceiptItemDto>,
    pub total_profit: Option<String>,
    pub vat_amount: Option<String>,
    /// Python v1 завжди порожній у цьому ендпоінті.
    pub cashier_name: String,
    pub payment_method: Option<String>,
}

/// Контракт репозиторію боржників.
#[async_trait::async_trait]
pub trait DebtorService: Send + Sync {
    async fn search(&self, q: &DebtorSearchQuery) -> Result<Vec<DebtorDto>, DebtorError>;
    async fn list(&self, page: i64, size: i64) -> Result<DebtorListDto, DebtorError>;
    async fn create(&self, input: &DebtorCreateInput) -> Result<DebtorDto, DebtorError>;
    async fn get(&self, id: Uuid) -> Result<DebtorDto, DebtorError>;
    async fn update(&self, id: Uuid, input: &DebtorUpdateInput) -> Result<DebtorDto, DebtorError>;
    async fn pay(&self, id: Uuid, input: &DebtorPayInput) -> Result<DebtorDto, DebtorError>;
    async fn receipts(&self, id: Uuid) -> Result<Vec<DebtorReceiptDto>, DebtorError>;
    async fn payments(&self, id: Uuid) -> Result<Vec<DebtorPaymentDto>, DebtorError>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Blanket impl: Arc<dyn DebtorService> також є DebtorService (як для PosService).
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl<T: DebtorService + ?Sized> DebtorService for std::sync::Arc<T> {
    async fn search(&self, q: &DebtorSearchQuery) -> Result<Vec<DebtorDto>, DebtorError> {
        self.as_ref().search(q).await
    }
    async fn list(&self, page: i64, size: i64) -> Result<DebtorListDto, DebtorError> {
        self.as_ref().list(page, size).await
    }
    async fn create(&self, input: &DebtorCreateInput) -> Result<DebtorDto, DebtorError> {
        self.as_ref().create(input).await
    }
    async fn get(&self, id: Uuid) -> Result<DebtorDto, DebtorError> {
        self.as_ref().get(id).await
    }
    async fn update(&self, id: Uuid, input: &DebtorUpdateInput) -> Result<DebtorDto, DebtorError> {
        self.as_ref().update(id, input).await
    }
    async fn pay(&self, id: Uuid, input: &DebtorPayInput) -> Result<DebtorDto, DebtorError> {
        self.as_ref().pay(id, input).await
    }
    async fn receipts(&self, id: Uuid) -> Result<Vec<DebtorReceiptDto>, DebtorError> {
        self.as_ref().receipts(id).await
    }
    async fn payments(&self, id: Uuid) -> Result<Vec<DebtorPaymentDto>, DebtorError> {
        self.as_ref().payments(id).await
    }
}
