// ─────────────────────────────────────────────────────────────────────────────
// auth — JWT-валідація (jsonwebtoken, HS256)
// ─────────────────────────────────────────────────────────────────────────────
// Секрет береться з env TORGASHKA_JWT_SECRET; fallback — backend/.env (SECRET_KEY,
// спільний із Python-бекендом). Хардкодити секрет у коді ЗАБОРОНЕНО.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Помилки JWT-валідації та резолву секрету.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("JWT-секрет не знайдено: задайте TORGASHKA_JWT_SECRET або SECRET_KEY у backend/.env")]
    MissingSecret,
    #[error("JWT-валідація не пройшла: {0}")]
    InvalidToken(#[from] jsonwebtoken::errors::Error),
    #[error("помилка читання конфігурації: {0}")]
    Io(#[from] std::io::Error),
}

/// Claims JWT-токена (1:1 Python AuthService.create_access_token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Ідентифікатор суб'єкта (user id).
    pub sub: String,
    /// Роль користувача (admin|cashier).
    pub role: String,
    /// Список прав доступу (з БД або дефолтні для ролі).
    /// Option: Python refresh-токен не містить поля permissions взагалі →
    /// serde(default) для декодування + skip_serializing_if для генерації 1:1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,
    /// Тип токена: "access" | "refresh" (Python to_encode["type"]).
    #[serde(rename = "type")]
    pub token_type: String,
    /// Issued at (Unix timestamp).
    pub iat: usize,
    /// Expiration (Unix timestamp).
    pub exp: usize,
}

/// Резолв JWT-секрету: env TORGASHKA_JWT_SECRET → backend/.env (SECRET_KEY) → Err.
pub fn resolve_jwt_secret() -> Result<String, AuthError> {
    if let Ok(s) = std::env::var("TORGASHKA_JWT_SECRET") {
        if !s.trim().is_empty() {
            return Ok(s);
        }
    }
    // Fallback: спільний секрет Python-бекенду (backend/.env → SECRET_KEY).
    for candidate in secret_file_candidates() {
        if let Ok(content) = std::fs::read_to_string(&candidate) {
            if let Some(v) = parse_env_value(&content, "SECRET_KEY") {
                return Ok(v);
            }
        }
    }
    Err(AuthError::MissingSecret)
}

/// Кандидати шляхів до backend/.env (залежно від робочої директорії запуску).
fn secret_file_candidates() -> Vec<std::path::PathBuf> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        std::path::PathBuf::from("backend/.env"),
        std::path::PathBuf::from("../../backend/.env"),
        // Від кореня пакета (CWD cargo test): crates/torgashka-api → kasa (4 рівні вгору).
        manifest.join("../../../../backend/.env"),
        manifest.join("../../../backend/.env"),
    ]
}

/// Примітивний парсер значення з .env-файлу (без зовнішніх залежностей).
fn parse_env_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        if k.trim() == key {
            Some(v.trim().trim_matches('"').trim_matches('\'').to_string())
        } else {
            None
        }
    })
}

/// Перевірка JWT-токена (HS256). Повертає claims у разі успіху.
pub fn validate_jwt(token: &str, secret: &str) -> Result<Claims, AuthError> {
    let key = jsonwebtoken::DecodingKey::from_secret(secret.as_bytes());
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    // Строга валідація: жодного leeway (дефолт jsonwebtoken — 60s).
    validation.leeway = 0;
    let data = jsonwebtoken::decode::<Claims>(token, &key, &validation)?;
    Ok(data.claims)
}

/// Створює access-токен (1:1 Python `create_access_token`):
/// HS256, claims {sub, role, permissions, type=access, iat, exp=+480 хв}.
pub fn create_access_token(
    user_id: &str,
    role: &str,
    permissions: &[String],
    secret: &str,
) -> Result<String, AuthError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            AuthError::InvalidToken(jsonwebtoken::errors::ErrorKind::ImmatureSignature.into())
        })?
        .as_secs() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        role: role.to_string(),
        permissions: Some(permissions.to_vec()),
        token_type: "access".to_string(),
        iat: now,
        exp: now + 480 * 60,
    };
    let key = jsonwebtoken::EncodingKey::from_secret(secret.as_bytes());
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &key,
    )
    .map_err(AuthError::from)
}

/// Створює refresh-токен (Python `create_refresh_token`): exp = +10080 хв.
pub fn create_refresh_token(user_id: &str, role: &str, secret: &str) -> Result<String, AuthError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            AuthError::InvalidToken(jsonwebtoken::errors::ErrorKind::ImmatureSignature.into())
        })?
        .as_secs() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        role: role.to_string(),
        permissions: None, // Python refresh-токен без поля permissions
        token_type: "refresh".to_string(),
        iat: now,
        exp: now + 10080 * 60,
    };
    let key = jsonwebtoken::EncodingKey::from_secret(secret.as_bytes());
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &key,
    )
    .map_err(AuthError::from)
}

/// Декодує JWT без перевірки типу (для refresh-ендпойнта).
pub fn decode_token(token: &str, secret: &str) -> Result<Claims, AuthError> {
    validate_jwt(token, secret)
}

/// Публічні шляхи (1:1 Python AuthMiddleware.PUBLIC_PATHS).
fn is_public_path(path: &str) -> bool {
    if path == "/api/v1/health" {
        return true;
    }
    const PUBLIC: &[&str] = &[
        "/api/v1/setup/status",
        "/api/v1/setup",
        "/api/v1/print/printers",
        "/api/v1/auth/login",
        "/api/v1/auth/login-pin",
        "/api/v1/auth/refresh",
        "/api/v1/auth/users-list",
        "/api/v1/auth/verify",
    ];
    if PUBLIC.contains(&path) {
        return true;
    }
    if path.starts_with("/docs") || path.starts_with("/redoc") {
        return true;
    }
    if path.starts_with("/openapi.json") {
        return true;
    }
    if path.contains("/auth/login") {
        return true;
    }
    if path.starts_with("/uploads/") {
        return true;
    }
    // Print документа відкривається в новій вкладці з ?token= (Python
    // get_current_user_optional) — auth перевіряється в самому хендлері.
    if path.starts_with("/api/v1/documents/") && path.ends_with("/print") {
        return true;
    }
    // /print-items інвойсів — вимагає JWT (Python get_current_user);
    // публічний тільки /documents/{id}/print (?token=).
    // Rust-гілка друку (етап 8, група 6): /print/* та /print-templates/*
    // ВИМАГАЮТЬ auth (Python get_current_user/require_admin) — крім
    // /print/printers (Python без Depends, уже в PUBLIC).
    if path.contains("/print") && !path.contains("/print-items") && !is_rust_print_route(path) {
        return true;
    }
    false
}

/// Шляхи Rust-гілки друку (етап 8, група 6) — auth обов'язковий.
fn is_rust_print_route(path: &str) -> bool {
    path == "/api/v1/print/price-tags/render"
        || path == "/api/v1/print/labels/render"
        || path == "/api/v1/print/test"
        || path == "/api/v1/print-templates"
        || path.starts_with("/api/v1/print-templates/")
}

/// JSON 403-відповідь (тіло 1:1 Python middleware).
fn forbidden_json(msg: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({"detail": msg})),
    )
        .into_response()
}

/// JSON 401-відповідь (тіло 1:1 Python middleware).
fn unauthorized_json(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({"detail": msg})),
    )
        .into_response()
}

/// Контекст device-каси (Частина 4): auth_middleware кладе в extensions,
/// store_context будує StoreCtx точки БЕЗ X-Store-Id (точка — з DeviceCtx,
/// захист від підміни X-Store-Id касою).
#[derive(Debug, Clone)]
pub struct DeviceCtx {
    pub device_id: uuid::Uuid,
    pub store_id: uuid::Uuid,
}

/// Шляхи, на яких каса може автентифікуватись device_token (Bearer)
/// замість JWT касира. Інші шляхи device-авторизації НЕ дають.
fn is_device_sync_path(path: &str) -> bool {
    path == "/api/v1/sync/master" || path == "/api/v1/sync/push"
}

enum DeviceAuthError {
    /// Пристрій знайдено, але статус не дозволяє sync.
    Forbidden(&'static str),
    /// БД недоступна або запит упав — автентифікувати неможливо.
    Unavailable,
}

/// SHA-256 hex — той самий формат, що network.rs зберігає в
/// devices.device_token_hash (токен віддається один раз, зберігається хеш).
fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(s.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Device fallback: sha256(Bearer token) → devices. Таблиця БЕЗ RLS
/// (мережевий рівень, NETWORK_DDL) — звичайний SELECT через store_pool.
/// Активний пристрій → Claims(role="device") + DeviceCtx.
async fn authenticate_device(
    state: &AppState,
    token: &str,
) -> Result<Option<(Claims, DeviceCtx)>, DeviceAuthError> {
    let store_pool = state
        .store_pool
        .clone()
        .ok_or(DeviceAuthError::Unavailable)?;
    let hash = sha256_hex(token);
    let row: Option<(uuid::Uuid, uuid::Uuid, String)> = sqlx::query_as(
        "SELECT id, store_id, status::text FROM devices WHERE device_token_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(&store_pool)
    .await
    .map_err(|e| {
        eprintln!("[torgashka-api] device auth: запит devices не виконано: {e}");
        DeviceAuthError::Unavailable
    })?;
    let Some((device_id, store_id, status)) = row else {
        return Ok(None);
    };
    match status.as_str() {
        "active" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as usize)
                .unwrap_or(0);
            let claims = Claims {
                sub: device_id.to_string(),
                role: "device".to_string(),
                permissions: None,
                token_type: "access".to_string(),
                iat: now,
                exp: now + 480 * 60,
            };
            Ok(Some((claims, DeviceCtx { device_id, store_id })))
        }
        "blocked" | "deleted" => Err(DeviceAuthError::Forbidden(
            "Пристрій заблоковано або видалено",
        )),
        _ => Err(DeviceAuthError::Forbidden("Пристрій не активовано")),
    }
}

/// Middleware JWT-валідації (1:1 Python AuthMiddleware).
/// Middleware JWT-валідації (1:1 Python AuthMiddleware).
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if is_public_path(path) {
        return next.run(req).await;
    }
    if req.method() == axum::http::Method::OPTIONS {
        return next.run(req).await;
    }
    let Some(auth_header) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return unauthorized_json("Відсутній заголовок авторизації");
    };
    let Some(token) = auth_header.strip_prefix("Bearer ") else {
        return unauthorized_json("Невірний формат токена. Використовуйте Bearer");
    };
    let claims = match validate_jwt(token, &state.jwt_secret) {
        Ok(c) => c,
        Err(e) => {
            // Device fallback (Частина 4): на sync-шляхах каса може бути
            // автентифікована device_token (Bearer) замість JWT касира.
            // Спрацьовує ЛИШЕ коли JWT невалідний — старі каси без змін.
            if is_device_sync_path(path) {
                return match authenticate_device(&state, token).await {
                    Ok(Some((device_claims, dctx))) => {
                        req.extensions_mut().insert(device_claims);
                        req.extensions_mut().insert(dctx);
                        next.run(req).await
                    }
                    Ok(None) => unauthorized_json("Недійсний токен пристрою"),
                    Err(DeviceAuthError::Forbidden(msg)) => forbidden_json(msg),
                    Err(DeviceAuthError::Unavailable) => {
                        eprintln!("[torgashka-api] device auth: БД недоступна");
                        unauthorized_json("Недійсний або прострочений токен")
                    }
                };
            }
            eprintln!("[torgashka-api] JWT відхилено: {e}");
            return unauthorized_json("Недійсний або прострочений токен");
        }
    };
    if claims.sub.is_empty() {
        return unauthorized_json("Недійсний токен");
    }
    // Зберігаємо claims у extensions — хендлери дістають sub через
    // `Extension<Claims>` для перевірки ролі (require_admin).
    req.extensions_mut().insert(claims);
    next.run(req).await
}

// ── Тести ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_value_parsed_from_dotenv() {
        let content = "DB_HOST=localhost\nSECRET_KEY=abc123\n";
        assert_eq!(
            parse_env_value(content, "SECRET_KEY"),
            Some("abc123".into())
        );
    }

    #[test]
    fn env_value_with_quotes_stripped() {
        let content = "SECRET_KEY=\"quoted-secret\"\n";
        assert_eq!(
            parse_env_value(content, "SECRET_KEY"),
            Some("quoted-secret".into())
        );
    }

    #[test]
    fn missing_key_returns_none() {
        let content = "OTHER=1\n";
        assert_eq!(parse_env_value(content, "SECRET_KEY"), None);
    }

    fn test_claims() -> Claims {
        Claims {
            sub: "user-1".into(),
            role: "admin".into(),
            permissions: Some(vec!["admin".into()]),
            token_type: "access".into(),
            iat: chrono_now() as usize,
            exp: (chrono_now() + 3600) as usize,
        }
    }

    #[test]
    fn jwt_roundtrip_valid_token_passes() {
        let secret = "test-secret-для-юніт-тесту";
        let claims = test_claims();
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("токен має кодуватись");
        let decoded = validate_jwt(&token, secret).expect("токен має валідуватись");
        assert_eq!(decoded.sub, "user-1");
    }

    #[test]
    fn jwt_expired_token_rejected() {
        let secret = "test-secret-2";
        let mut claims = test_claims();
        claims.exp = (chrono_now() - 10) as usize;
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("токен має кодуватись");
        assert!(validate_jwt(&token, secret).is_err());
    }

    #[test]
    fn jwt_wrong_secret_rejected() {
        let claims = test_claims();
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret("правильний-секрет".as_bytes()),
        )
        .expect("токен має кодуватись");
        assert!(validate_jwt(&token, "inshyj-secret").is_err());
    }

    /// Поточний Unix-час (без залежності від chrono в основних deps).
    fn chrono_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("системний час")
            .as_secs()
    }
}
