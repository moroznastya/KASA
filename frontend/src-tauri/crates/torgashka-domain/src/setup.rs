// ─────────────────────────────────────────────────────────────────────────────
// setup — контракт першого встановлення (Частина 1 + Частина 2)
// ─────────────────────────────────────────────────────────────────────────────
// На свіжій БД (без користувачів) вхід неможливий: auth потребує існуючого
// користувача, а створити першого власника було нічим. Цей модуль дає
// публічний setup-ендпоінт:
//
//   GET  /api/v1/setup/status → {"status": "not_initialized" | "initialized"}
//   POST /api/v1/setup         → створити першого власника + точку + персональну
//                                БД (Частина 2) → LoginResult (одразу авторизує)
//
// Реалізація SQL — torgashka-infrastructure/repositories/setup.rs (SqlxSetupService).
// ─────────────────────────────────────────────────────────────────────────────

use serde::Serialize;
use uuid::Uuid;

use crate::auth::{LoginResult, UserDto};

/// Відповідь GET /api/v1/setup/status.
#[derive(Debug, Clone, Serialize)]
pub struct SetupStatusDto {
    /// "not_initialized" — у БД немає жодного користувача (треба /setup);
    /// "initialized" — система вже має користувачів.
    pub status: String,
}

/// Тіло POST /api/v1/setup.
#[derive(Debug, Clone)]
pub struct SetupRequest {
    /// Ім'я власника (обов'язкове).
    pub name: String,
    /// Логін авторизації (обов'язковий, унікальний).
    pub login: String,
    /// Пароль (мін. 6 символів).
    pub password: String,
    /// Назва першої торговельної точки (обов'язкова).
    pub store_name: String,
    /// Адреса точки (опційно).
    pub store_address: Option<String>,
    /// Телефон точки (опційно).
    pub store_phone: Option<String>,
}

/// Помилки setup-операцій.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Infrastructure(String),
}

/// Контракт setup-операцій (Частина 1 + Частина 2).
#[async_trait::async_trait]
pub trait SetupService: Send + Sync {
    /// GET /api/v1/setup/status — чи ініціалізована система.
    async fn status(&self) -> Result<SetupStatusDto, SetupError>;
    /// POST /api/v1/setup — створити першого власника, точку, user_stores
    /// (одна транзакція мета-БД) + персональну БД власника (Частина 2).
    /// Повертає LoginResult з порожніми токенами — API-шар заповнює JWT.
    async fn setup(&self, input: &SetupRequest) -> Result<LoginResult, SetupError>;
}

/// Хелпер: дефолтні права власника (всі права, як admin).
pub fn owner_permissions() -> Vec<String> {
    crate::auth::ALL_PERMISSIONS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// UserDto власника (для LoginResult setup).
pub fn owner_user_dto(
    id: Uuid,
    name: &str,
    login: &str,
    permissions: &[String],
    now: chrono::NaiveDateTime,
) -> UserDto {
    UserDto {
        id,
        name: name.to_string(),
        login: login.to_string(),
        role: "owner".to_string(),
        is_active: true,
        onboarding_completed: true,
        permissions: Some(permissions.to_vec()),
        created_at: now,
        updated_at: now,
    }
}
