// ─────────────────────────────────────────────────────────────────────────────
// setup — хендлери першого встановлення (Частина 1 + Частина 2)
// ─────────────────────────────────────────────────────────────────────────────
//   GET  /api/v1/setup/status (публічний) → {"status": "not_initialized"|"initialized"}
//   POST /api/v1/setup         (публічний) → LoginResult (одразу авторизує)
//
// Сценарій: fresh-БД без користувачів → LoginPage редиректить на /setup →
// майстер створює першого власника + точку + персональну БД (Частина 2).
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use torgashka_domain::{LoginResult, SetupError, SetupRequest, SetupService, SetupStatusDto};

use crate::auth_routes::{self, AuthRouteError};
use crate::AppState;

/// Тіло POST /api/v1/setup (1:1 з фронтенд-майстром SetupPage).
#[derive(Debug, Deserialize)]
pub struct SetupBody {
    pub name: String,
    pub login: String,
    pub password: String,
    pub store_name: String,
    #[serde(default)]
    pub store_address: Option<String>,
    #[serde(default)]
    pub store_phone: Option<String>,
}

/// Помилки setup → HTTP.
#[derive(Debug)]
pub enum SetupHttpError {
    Service(SetupError),
}

impl From<SetupError> for SetupHttpError {
    fn from(e: SetupError) -> Self {
        SetupHttpError::Service(e)
    }
}

impl From<AuthRouteError> for SetupHttpError {
    fn from(e: AuthRouteError) -> Self {
        match e {
            AuthRouteError::Plain(err) => {
                use torgashka_domain::AuthError;
                match err {
                    AuthError::BadRequest(m) => SetupHttpError::Service(SetupError::BadRequest(m)),
                    AuthError::Conflict(m) => SetupHttpError::Service(SetupError::Conflict(m)),
                    _ => SetupHttpError::Service(SetupError::Infrastructure(
                        "Помилка генерації токенів".to_string(),
                    )),
                }
            }
            AuthRouteError::Validation(_) => {
                SetupHttpError::Service(SetupError::BadRequest("Невірні дані запиту".to_string()))
            }
        }
    }
}

impl IntoResponse for SetupHttpError {
    fn into_response(self) -> Response {
        match self {
            SetupHttpError::Service(e) => match e {
                SetupError::BadRequest(msg) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                SetupError::Conflict(msg) => (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                SetupError::Infrastructure(msg) => {
                    eprintln!("[torgashka-api] setup infrastructure error: {msg}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"detail": msg})),
                    )
                        .into_response()
                }
            },
        }
    }
}

fn setup_svc(state: &AppState) -> Result<Arc<dyn SetupService + Send + Sync>, SetupHttpError> {
    state.setup.clone().ok_or_else(|| {
        SetupHttpError::Service(SetupError::Infrastructure(
            "Rust-гілка setup не ініціалізована (БД недоступна?)".to_string(),
        ))
    })
}

/// GET /api/v1/setup/status — публічний, без JWT.
pub async fn status(State(state): State<AppState>) -> Result<Json<SetupStatusDto>, SetupHttpError> {
    let svc = setup_svc(&state)?;
    Ok(Json(svc.status().await?))
}

/// POST /api/v1/setup — створити першого власника + точку + персональну БД.
/// Повертає LoginResult з токенами — користувач одразу авторизований.
pub async fn setup(
    State(state): State<AppState>,
    Json(body): Json<SetupBody>,
) -> Result<Json<LoginResult>, SetupHttpError> {
    let svc = setup_svc(&state)?;
    let input = SetupRequest {
        name: body.name,
        login: body.login,
        password: body.password,
        store_name: body.store_name,
        store_address: body.store_address,
        store_phone: body.store_phone,
    };
    let mut result = svc.setup(&input).await?;
    // JWT — той самий секрет/формат, що auth/login (спільний з Python).
    let (access, refresh) = auth_routes::issue_tokens(&state, &result.user).await?;
    result.access_token = access;
    result.refresh_token = refresh;
    Ok(Json(result))
}
