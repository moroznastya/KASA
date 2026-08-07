// ─────────────────────────────────────────────────────────────────────────────
// auth — JWT-валідація (jsonwebtoken, HS256)
// ─────────────────────────────────────────────────────────────────────────────
// Секрет береться з env KASA_JWT_SECRET; fallback — backend/.env (SECRET_KEY,
// спільний із Python-бекендом). Хардкодити секрет у коді ЗАБОРОНЕНО.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Помилки JWT-валідації та резолву секрету.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("JWT-секрет не знайдено: задайте KASA_JWT_SECRET або SECRET_KEY у backend/.env")]
    MissingSecret,
    #[error("JWT-валідація не пройшла: {0}")]
    InvalidToken(#[from] jsonwebtoken::errors::Error),
    #[error("помилка читання конфігурації: {0}")]
    Io(#[from] std::io::Error),
}

/// Claims JWT-токена (мінімальний набір: sub + exp).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Ідентифікатор суб'єкта (наприклад, user id).
    pub sub: String,
    /// Expiration (Unix timestamp).
    pub exp: usize,
}

/// Резолв JWT-секрету: env KASA_JWT_SECRET → backend/.env (SECRET_KEY) → Err.
pub fn resolve_jwt_secret() -> Result<String, AuthError> {
    if let Ok(s) = std::env::var("KASA_JWT_SECRET") {
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

/// Middleware JWT-валідації. /api/v1/health пропускається без токена.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if req.uri().path() == "/api/v1/health" {
        return Ok(next.run(req).await);
    }
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = validate_jwt(token, &state.jwt_secret).map_err(|e| {
        eprintln!("[kasa-api] JWT відхилено: {e}");
        StatusCode::UNAUTHORIZED
    })?;
    // Зберігаємо claims у extensions — CRUD-хендлери (етап 2) дістають sub
    // через `Extension<Claims>` для перевірки ролі (require_admin).
    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
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

    #[test]
    fn jwt_roundtrip_valid_token_passes() {
        let secret = "test-secret-для-юніт-тесту";
        let claims = Claims {
            sub: "user-1".into(),
            exp: (chrono_now() + 3600) as usize,
        };
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
        let claims = Claims {
            sub: "user-1".into(),
            exp: (chrono_now() - 10) as usize,
        };
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
        let claims = Claims {
            sub: "user-1".into(),
            exp: (chrono_now() + 3600) as usize,
        };
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
