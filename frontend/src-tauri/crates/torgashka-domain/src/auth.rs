//! Auth-порти (етап 6): авторизація, користувачі, RBAC, налаштування.
//!
//! Контракт між application і infrastructure — 1:1 з Python-еталоном (v1):
//!   - /api/v1/auth: login, login-pin, refresh, logout, verify, users-list
//!   - /api/v1/users: CRUD + permissions + hourly-rate + permissions/list (admin)
//!   - /api/v1/settings: GET all/by module, PUT batch/by key (admin)
//!
//! JWT: HS256, спільний секрет з Python (backend/.env → SECRET_KEY).
//! Claims: {sub, role, permissions, type: "access"|"refresh", iat, exp}.
//! Ролі: admin | cashier (Enum user_role) — роль "manager" у Python ВІДСУТНЯ.
//!
//! ВІДОМІ АНОМАЛІЇ Python (зафіксовано, docs/RUST_MIGRATION_EXECUTION.md):
//!   1. GET /auth/me відсутній у Python (404), але фронтенд його викликає —
//!      Rust реалізує робочий /auth/me (UserResponse поточного користувача).
//!   2. Rate limit 5/min на login/login-pin (slowapi) — інфраструктурна
//!      політика, НЕ бізнес-логіка: Rust не відтворює 429 (differential
//!      враховує: ≤5 login-запитів на Python за хвилину).

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Ролі
// ─────────────────────────────────────────────────────────────────────────────

/// Роль користувача (Enum user_role у БД). Python має ТІЛЬКИ admin|cashier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    Cashier,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            UserRole::Admin => "admin",
            UserRole::Cashier => "cashier",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(UserRole::Admin),
            "cashier" => Some(UserRole::Cashier),
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Права доступу (перелік 1:1 з Python Permission enum)
// ─────────────────────────────────────────────────────────────────────────────

pub const ALL_PERMISSIONS: &[&str] = &[
    "products:view",
    "products:create",
    "products:edit",
    "products:delete",
    "categories:view",
    "categories:create",
    "categories:edit",
    "categories:delete",
    "suppliers:view",
    "suppliers:create",
    "suppliers:edit",
    "suppliers:delete",
    "documents:view",
    "invoices:create",
    "invoices:edit",
    "invoices:delete",
    "invoices:confirm",
    "transfers:create",
    "transfers:edit",
    "transfers:delete",
    "transfers:confirm",
    "write-offs:create",
    "write-offs:edit",
    "write-offs:delete",
    "write-offs:confirm",
    "returns:create",
    "returns:edit",
    "returns:delete",
    "returns:confirm",
    "pos:access",
    "receipts:view",
    "receipts:cancel",
    "debtors:view",
    "debtors:create",
    "debtors:edit",
    "debtors:delete",
    "debtors:pay",
    "ledger:view",
    "ledger:create",
    "reports:view",
    "reports:stats",
    "users:view",
    "users:create",
    "users:edit",
    "users:delete",
    "users:manage-permissions",
];

/// Права за замовчуванням для касира (Python CASHIER_PERMISSIONS).
pub const CASHIER_PERMISSIONS: &[&str] = &[
    "products:view",
    "categories:view",
    "suppliers:view",
    "documents:view",
    "invoices:create",
    "pos:access",
    "receipts:view",
    "debtors:view",
    "debtors:create",
    "debtors:pay",
    "ledger:view",
    "reports:stats",
];

/// Українські назви прав (Python PERMISSION_LABELS) — для permissions/list.
pub fn permission_label(key: &str) -> &'static str {
    match key {
        "products:view" => "Перегляд товарів",
        "products:create" => "Створення товарів",
        "products:edit" => "Редагування товарів",
        "products:delete" => "Видалення товарів",
        "categories:view" => "Перегляд категорій",
        "categories:create" => "Створення категорій",
        "categories:edit" => "Редагування категорій",
        "categories:delete" => "Видалення категорій",
        "suppliers:view" => "Перегляд постачальників",
        "suppliers:create" => "Створення постачальників",
        "suppliers:edit" => "Редагування постачальників",
        "suppliers:delete" => "Видалення постачальників",
        "documents:view" => "Перегляд документів",
        "invoices:create" => "Створення накладних",
        "invoices:edit" => "Редагування накладних",
        "invoices:delete" => "Видалення накладних",
        "invoices:confirm" => "Підтвердження накладних",
        "transfers:create" => "Створення переміщень",
        "transfers:edit" => "Редагування переміщень",
        "transfers:delete" => "Видалення переміщень",
        "transfers:confirm" => "Підтвердження переміщень",
        "write-offs:create" => "Створення списань",
        "write-offs:edit" => "Редагування списань",
        "write-offs:delete" => "Видалення списань",
        "write-offs:confirm" => "Підтвердження списань",
        "returns:create" => "Створення повернень",
        "returns:edit" => "Редагування повернень",
        "returns:delete" => "Видалення повернень",
        "returns:confirm" => "Підтвердження повернень",
        "pos:access" => "Доступ до POS-каси",
        "receipts:view" => "Перегляд чеків",
        "receipts:cancel" => "Скасування чеків",
        "debtors:view" => "Перегляд боржників",
        "debtors:create" => "Створення боржників",
        "debtors:edit" => "Редагування боржників",
        "debtors:delete" => "Видалення боржників",
        "debtors:pay" => "Прийом оплати від боржників",
        "ledger:view" => "Перегляд взаєморозрахунків",
        "ledger:create" => "Створення записів у взаєморозрахунках",
        "reports:view" => "Перегляд звітів",
        "reports:stats" => "Перегляд статистики",
        "users:view" => "Перегляд користувачів",
        "users:create" => "Створення користувачів",
        "users:edit" => "Редагування користувачів",
        "users:delete" => "Видалення користувачів",
        "users:manage-permissions" => "Управління правами доступу",
        _ => "unknown",
    }
}

/// Групи прав для фронтенду (Python PERMISSION_GROUPS) — порядок 1:1.
pub const PERMISSION_GROUPS: &[(&str, &str, &[&str])] = &[
    (
        "Товари",
        "Package",
        &[
            "products:view",
            "products:create",
            "products:edit",
            "products:delete",
        ],
    ),
    (
        "Категорії",
        "Tags",
        &[
            "categories:view",
            "categories:create",
            "categories:edit",
            "categories:delete",
        ],
    ),
    (
        "Постачальники",
        "Truck",
        &[
            "suppliers:view",
            "suppliers:create",
            "suppliers:edit",
            "suppliers:delete",
        ],
    ),
    (
        "Прибуткові накладні",
        "FileText",
        &[
            "documents:view",
            "invoices:create",
            "invoices:edit",
            "invoices:delete",
            "invoices:confirm",
        ],
    ),
    (
        "Переміщення",
        "ArrowRightLeft",
        &[
            "transfers:create",
            "transfers:edit",
            "transfers:delete",
            "transfers:confirm",
        ],
    ),
    (
        "Списання",
        "Trash2",
        &[
            "write-offs:create",
            "write-offs:edit",
            "write-offs:delete",
            "write-offs:confirm",
        ],
    ),
    (
        "Повернення постачальнику",
        "Undo2",
        &[
            "returns:create",
            "returns:edit",
            "returns:delete",
            "returns:confirm",
        ],
    ),
    (
        "POS-каса",
        "ShoppingCart",
        &["pos:access", "receipts:view", "receipts:cancel"],
    ),
    (
        "Боржники",
        "Users",
        &[
            "debtors:view",
            "debtors:create",
            "debtors:edit",
            "debtors:delete",
            "debtors:pay",
        ],
    ),
    (
        "Взаєморозрахунки",
        "BookOpen",
        &["ledger:view", "ledger:create"],
    ),
    ("Звіти", "BarChart3", &["reports:view", "reports:stats"]),
    (
        "Користувачі",
        "UserCog",
        &[
            "users:view",
            "users:create",
            "users:edit",
            "users:delete",
            "users:manage-permissions",
        ],
    ),
];

/// Дефолтні права для ролі (Python get_default_permissions).
pub fn default_permissions(role: UserRole) -> Vec<String> {
    match role {
        UserRole::Admin => ALL_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
        UserRole::Cashier => CASHIER_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
    }
}

/// Транслітерація імені → логін (1:1 Python UserEntity.generate_login_from_name).
/// Кирилиця → латиниця, [^a-zA-Z0-9] → '_', lower, strip, злиття підкреслень.
pub fn generate_login_from_name(name: &str) -> String {
    let map: &[(&str, &str)] = &[
        ("а", "a"),
        ("б", "b"),
        ("в", "v"),
        ("г", "h"),
        ("ґ", "g"),
        ("д", "d"),
        ("е", "e"),
        ("є", "ie"),
        ("ж", "zh"),
        ("з", "z"),
        ("и", "y"),
        ("і", "i"),
        ("ї", "i"),
        ("й", "i"),
        ("к", "k"),
        ("л", "l"),
        ("м", "m"),
        ("н", "n"),
        ("о", "o"),
        ("п", "p"),
        ("р", "r"),
        ("с", "s"),
        ("т", "t"),
        ("у", "u"),
        ("ф", "f"),
        ("х", "kh"),
        ("ц", "ts"),
        ("ч", "ch"),
        ("ш", "sh"),
        ("щ", "shch"),
        ("ю", "iu"),
        ("я", "ia"),
        ("А", "a"),
        ("Б", "b"),
        ("В", "v"),
        ("Г", "h"),
        ("Ґ", "g"),
        ("Д", "d"),
        ("Е", "e"),
        ("Є", "ie"),
        ("Ж", "zh"),
        ("З", "z"),
        ("И", "y"),
        ("І", "i"),
        ("Ї", "i"),
        ("Й", "i"),
        ("К", "k"),
        ("Л", "l"),
        ("М", "m"),
        ("Н", "n"),
        ("О", "o"),
        ("П", "p"),
        ("Р", "r"),
        ("С", "s"),
        ("Т", "t"),
        ("У", "u"),
        ("Ф", "f"),
        ("Х", "kh"),
        ("Ц", "ts"),
        ("Ч", "ch"),
        ("Ш", "sh"),
        ("Щ", "shch"),
        ("Ю", "iu"),
        ("Я", "ia"),
    ];
    let mut result = String::new();
    for ch in name.chars() {
        let c = ch.to_string();
        let mut replaced = false;
        for (from, to) in map {
            if *from == c {
                result.push_str(to);
                replaced = true;
                break;
            }
        }
        if !replaced {
            result.push(ch);
        }
    }
    // [^a-zA-Z0-9] → '_'
    let mut with_us: String = result
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    // lower + strip('_')
    with_us = with_us.to_lowercase();
    let trimmed = with_us.trim_matches('_').to_string();
    // злиття послідовних '_'
    let mut out = String::new();
    let mut prev_underscore = false;
    for c in trimmed.chars() {
        if c == '_' {
            if !prev_underscore {
                out.push(c);
            }
            prev_underscore = true;
        } else {
            out.push(c);
            prev_underscore = false;
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// DTO
// ─────────────────────────────────────────────────────────────────────────────

/// Відповідь користувача (Python UserResponse — v1, без email/phone).
#[derive(Debug, Clone, Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub name: String,
    pub login: String,
    pub role: String,
    pub is_active: bool,
    pub onboarding_completed: bool,
    pub permissions: Option<Vec<String>>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl UserDto {
    /// Права для JWT-токена: з БД або дефолтні для ролі (як Python
    /// `_get_user_permissions` → `user.permissions or default_permissions(role)`).
    pub fn effective_permissions(&self) -> Vec<String> {
        self.permissions.clone().unwrap_or_else(|| {
            default_permissions(UserRole::parse(&self.role).unwrap_or(UserRole::Cashier))
        })
    }
}

/// Результат логіну (access + refresh + user) — Python UserTokenResponse.
#[derive(Debug, Clone, Serialize)]
pub struct LoginResult {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub user: UserDto,
}

/// Вхідні дані створення користувача (Python UserCreate).
#[derive(Debug, Clone)]
pub struct UserCreateInput {
    pub name: String,
    pub login: Option<String>,
    pub password: String,
    pub pin_code: Option<String>,
    pub role: UserRole,
    pub is_active: bool,
    pub permissions: Option<Vec<String>>,
}

/// Вхідні дані оновлення користувача (Python UserUpdate, всі поля optional).
#[derive(Debug, Clone, Default)]
pub struct UserUpdateInput {
    pub name: Option<String>,
    pub login: Option<String>,
    pub password: Option<String>,
    pub pin_code: Option<String>,
    pub role: Option<UserRole>,
    pub is_active: Option<bool>,
    pub onboarding_completed: Option<bool>,
    pub permissions: Option<Vec<String>>,
}

impl UserUpdateInput {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.login.is_none()
            && self.password.is_none()
            && self.pin_code.is_none()
            && self.role.is_none()
            && self.is_active.is_none()
            && self.onboarding_completed.is_none()
            && self.permissions.is_none()
    }
}

/// Права доступу для оновлення (Python UserPermissionsUpdate).
#[derive(Debug, Clone)]
pub struct PermissionsUpdateInput {
    pub permissions: Vec<String>,
}

/// Погодинна ставка (Python HourlyRateUpdate).
#[derive(Debug, Clone)]
pub struct HourlyRateInput {
    pub hourly_rate: f64,
}

/// Запит логіну за паролем (v1 UserLoginRequest — без мінімальних довжин).
#[derive(Debug, Clone)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

/// Запит логіну за PIN (v1 UserPinLoginRequest).
#[derive(Debug, Clone)]
pub struct LoginPinRequest {
    pub login: String,
    pub pin_code: String,
}

/// Список користувачів (Python list_users відповідь).
#[derive(Debug, Clone, Serialize)]
pub struct UserListDto {
    pub items: Vec<UserDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

/// Публічний список для сторінки логіну (Python get_users_list).
#[derive(Debug, Clone, Serialize)]
pub struct PublicUserDto {
    pub id: Uuid,
    pub name: String,
    pub login: String,
}

/// Результат verify (Python verify_token).
#[derive(Debug, Clone, Serialize)]
pub struct VerifyDto {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Налаштування системи (Python SystemSettingRead).
/// created_at/updated_at — timestamptz (БД) → RFC3339 з 'Z' (як Python).
#[derive(Debug, Clone, Serialize)]
pub struct SettingDto {
    pub id: Uuid,
    pub module: String,
    pub key: String,
    pub value: Option<String>,
    pub value_type: String,
    pub label: String,
    pub description: Option<String>,
    pub options: Option<String>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Відповідь GET /settings (Python SettingsModuleResponse).
#[derive(Debug, Clone, Serialize)]
pub struct SettingsModulesDto {
    pub modules: serde_json::Map<String, serde_json::Value>,
}

/// Оновлення налаштувань (Python SystemSettingBatchUpdate: {settings: {k: v}}).
#[derive(Debug, Clone)]
pub struct SettingsBatchInput {
    pub settings: Vec<(String, Option<String>)>,
}

/// Оновлення одного налаштування (Python SystemSettingUpdate: {value}).
#[derive(Debug, Clone)]
pub struct SettingUpdateInput {
    pub value: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Помилки → HTTP 1:1 з Python
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// 401 Unauthorized.
    #[error("{0}")]
    Unauthorized(String),
    /// 403 Forbidden.
    #[error("{0}")]
    Forbidden(String),
    /// 404 Not Found.
    #[error("{0}")]
    NotFound(String),
    /// 409 Conflict.
    #[error("{0}")]
    Conflict(String),
    /// 400 Bad Request.
    #[error("{0}")]
    BadRequest(String),
    /// 422 Pydantic-валідація (повний detail).
    #[error("{0}")]
    Validation(serde_json::Value),
    /// 500 Internal Server Error.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Контракт сервісу (blanket impl для Arc)
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl<T: AuthService + ?Sized> AuthService for std::sync::Arc<T> {
    async fn login(&self, input: &LoginRequest) -> Result<LoginResult, AuthError> {
        self.as_ref().login(input).await
    }
    async fn login_pin(&self, input: &LoginPinRequest) -> Result<LoginResult, AuthError> {
        self.as_ref().login_pin(input).await
    }
    async fn refresh(&self, user_id: Uuid) -> Result<LoginResult, AuthError> {
        self.as_ref().refresh(user_id).await
    }
    async fn logout(&self, user_id: Uuid) -> Result<(), AuthError> {
        self.as_ref().logout(user_id).await
    }
    async fn get_user_by_id(&self, user_id: Uuid) -> Result<UserDto, AuthError> {
        self.as_ref().get_user_by_id(user_id).await
    }
    async fn users_list_public(&self) -> Result<Vec<PublicUserDto>, AuthError> {
        self.as_ref().users_list_public().await
    }
    async fn list_users(&self, page: i64, size: i64) -> Result<UserListDto, AuthError> {
        self.as_ref().list_users(page, size).await
    }
    async fn create_user(&self, input: &UserCreateInput) -> Result<UserDto, AuthError> {
        self.as_ref().create_user(input).await
    }
    async fn update_user(
        &self,
        user_id: Uuid,
        input: &UserUpdateInput,
    ) -> Result<UserDto, AuthError> {
        self.as_ref().update_user(user_id, input).await
    }
    async fn update_permissions(
        &self,
        user_id: Uuid,
        permissions: &[String],
    ) -> Result<UserDto, AuthError> {
        self.as_ref().update_permissions(user_id, permissions).await
    }
    async fn update_hourly_rate(
        &self,
        user_id: Uuid,
        hourly_rate: f64,
    ) -> Result<serde_json::Value, AuthError> {
        self.as_ref().update_hourly_rate(user_id, hourly_rate).await
    }
    async fn delete_user(&self, user_id: Uuid, current_user_id: Uuid) -> Result<(), AuthError> {
        self.as_ref().delete_user(user_id, current_user_id).await
    }
    async fn settings_all(&self) -> Result<SettingsModulesDto, AuthError> {
        self.as_ref().settings_all().await
    }
    async fn settings_by_module(&self, module: &str) -> Result<Vec<SettingDto>, AuthError> {
        self.as_ref().settings_by_module(module).await
    }
    async fn settings_batch_update(
        &self,
        settings: &[(String, Option<String>)],
    ) -> Result<SettingsModulesDto, AuthError> {
        self.as_ref().settings_batch_update(settings).await
    }
    async fn settings_update_key(
        &self,
        key: &str,
        value: Option<String>,
    ) -> Result<SettingDto, AuthError> {
        self.as_ref().settings_update_key(key, value).await
    }
}

/// Контракт auth-операцій (етап 6) — 1:1 з Python v1.
#[async_trait::async_trait]
pub trait AuthService: Send + Sync {
    /// POST /api/v1/auth/login — логін за паролем + робоча сесія.
    async fn login(&self, input: &LoginRequest) -> Result<LoginResult, AuthError>;
    /// POST /api/v1/auth/login-pin — логін за PIN + робоча сесія.
    async fn login_pin(&self, input: &LoginPinRequest) -> Result<LoginResult, AuthError>;
    /// POST /api/v1/auth/refresh — нова пара токенів (API-шар декодував JWT
    /// і передає user_id; тут — перевірка активності 1:1 Python).
    async fn refresh(&self, user_id: Uuid) -> Result<LoginResult, AuthError>;
    /// POST /api/v1/auth/logout — закриває останню активну робочу сесію.
    async fn logout(&self, user_id: Uuid) -> Result<(), AuthError>;
    /// GET /api/v1/auth/users/me (аномалія Python) — поточний користувач.
    async fn get_user_by_id(&self, user_id: Uuid) -> Result<UserDto, AuthError>;
    /// GET /api/v1/auth/users-list — активні користувачі (id, name, login).
    async fn users_list_public(&self) -> Result<Vec<PublicUserDto>, AuthError>;
    /// GET /api/v1/users — список з пагінацією (admin).
    async fn list_users(&self, page: i64, size: i64) -> Result<UserListDto, AuthError>;
    /// POST /api/v1/users — створення (admin); авто-генерація логіну.
    async fn create_user(&self, input: &UserCreateInput) -> Result<UserDto, AuthError>;
    /// PUT /api/v1/users/{id} — часткове оновлення (admin).
    async fn update_user(
        &self,
        user_id: Uuid,
        input: &UserUpdateInput,
    ) -> Result<UserDto, AuthError>;
    /// PUT /api/v1/users/{id}/permissions — оновлення прав (admin).
    async fn update_permissions(
        &self,
        user_id: Uuid,
        permissions: &[String],
    ) -> Result<UserDto, AuthError>;
    /// PUT /api/v1/users/{id}/hourly-rate — погодинна ставка (admin).
    async fn update_hourly_rate(
        &self,
        user_id: Uuid,
        hourly_rate: f64,
    ) -> Result<serde_json::Value, AuthError>;
    /// DELETE /api/v1/users/{id} — видалення (admin); 409 при чеках/self.
    async fn delete_user(&self, user_id: Uuid, current_user_id: Uuid) -> Result<(), AuthError>;
    /// GET /api/v1/settings — всі налаштування за модулями.
    async fn settings_all(&self) -> Result<SettingsModulesDto, AuthError>;
    /// GET /api/v1/settings/{module} — налаштування модуля (404 якщо порожньо).
    async fn settings_by_module(&self, module: &str) -> Result<Vec<SettingDto>, AuthError>;
    /// PUT /api/v1/settings — масове оновлення (admin, валідація 422).
    async fn settings_batch_update(
        &self,
        settings: &[(String, Option<String>)],
    ) -> Result<SettingsModulesDto, AuthError>;
    /// PUT /api/v1/settings/{key} — upsert (admin, валідація 422).
    async fn settings_update_key(
        &self,
        key: &str,
        value: Option<String>,
    ) -> Result<SettingDto, AuthError>;
}
