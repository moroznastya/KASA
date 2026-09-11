//! Мультиточковість — торговельні точки (Етап 3).
//!
//! Контракт сервісу точок:
//!   - список точок користувача (через user_stores);
//!   - створення точки (owner) + автоприв'язка творця як owner;
//!   - призначення користувача на точку (owner);
//!   - міжточкова наявність (stock по всіх точках користувача).

use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

/// Помилки сервісу точок.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Conflict(String),
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

/// Торговельна точка (stores + роль у user_stores для поточного користувача).
#[derive(Debug, Clone, Serialize)]
pub struct StoreDto {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Юрособа/ФОП (для ПРРО-вкладки; Етап 1 адмін-панелі, Етапи 4-6 — ПРРО).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legal_name: Option<String>,
    /// Код ЄДРПОУ/ІПН (для ПРРО-вкладки; nullable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edrpou: Option<String>,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    /// Роль користувача НА ЦІЙ ТОЧЦІ (з user_stores).
    pub role: String,
    /// Чи є точка точкою за замовчуванням для користувача.
    pub is_default: bool,
}

/// Створення нової точки.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct StoreCreateInput {
    pub name: String,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    /// Юрособа/ФОП (для ПРРО-вкладки; Етапи 4-6).
    #[serde(default)]
    pub legal_name: Option<String>,
    /// Код ЄДРПОУ/ІПН (для ПРРО-вкладки).
    #[serde(default)]
    pub edrpou: Option<String>,
}

/// Призначення користувача на точку.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UserStoreAssignInput {
    pub user_id: Uuid,
    pub store_id: Uuid,
    #[serde(default = "default_store_role")]
    pub role: String,
    #[serde(default)]
    pub is_default: bool,
}

fn default_store_role() -> String {
    "cashier".to_string()
}

/// Наявність товару в точці.
#[derive(Debug, Clone, Serialize)]
pub struct StoreAvailabilityDto {
    pub store_id: Uuid,
    pub store_name: String,
    pub quantity: String,
    pub price: String,
}

/// Міжточкова наявність товару (по всіх точках користувача).
#[derive(Debug, Clone, Serialize)]
pub struct AvailabilityItemDto {
    pub product_id: Uuid,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub barcode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub stores: Vec<StoreAvailabilityDto>,
}

/// Контракт сервісу торговельних точок (Етап 3).
#[async_trait::async_trait]
pub trait StoreService: Send + Sync {
    /// Список точок користувача (RLS: тільки свої через user_stores).
    async fn list_stores(&self) -> Result<Vec<StoreDto>, StoreError>;
    /// Створити точку (owner) + автоприв'язка творця як owner.
    async fn create_store(&self, input: &StoreCreateInput) -> Result<StoreDto, StoreError>;
    /// Призначити користувача на точку (owner).
    async fn assign_user_store(&self, input: &UserStoreAssignInput)
        -> Result<StoreDto, StoreError>;
    /// Міжточкова наявність: залишки по всіх точках користувача.
    async fn availability(&self) -> Result<Vec<AvailabilityItemDto>, StoreError>;
}
