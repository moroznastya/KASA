// ─────────────────────────────────────────────────────────────────────────────
// stores — хендлери торговельних точок (Етап 3 мультиточковості)
// ─────────────────────────────────────────────────────────────────────────────
//   GET  /api/v1/stores                → список точок користувача
//   POST /api/v1/stores                → створити точку (owner) + автоприв'язка
//   POST /api/v1/user-stores           → призначити користувача на точку (owner)
//   GET  /api/v1/inventory/availability → міжточкова наявність (через user_stores)
//
// JWT-валідація — middleware; роль/точка — з StoreCtx (task-local).
// RLS-контур: усі запити йдуть через StorePool (set_config на кожен запит).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use torgashka_domain::{
    AvailabilityItemDto, StoreCreateInput, StoreDto, StoreError, StoreService,
    UserStoreAssignInput,
};

use crate::AppState;

/// Помилки точок → HTTP (як решта модулів фасаду).
#[derive(Debug)]
pub enum StoreErr {
    Service(StoreError),
}

impl From<StoreError> for StoreErr {
    fn from(e: StoreError) -> Self {
        StoreErr::Service(e)
    }
}

impl IntoResponse for StoreErr {
    fn into_response(self) -> Response {
        match self {
            StoreErr::Service(e) => match e {
                StoreError::NotFound(msg) => {
                    (StatusCode::NOT_FOUND, Json(serde_json::json!({"detail": msg})))
                        .into_response()
                }
                StoreError::BadRequest(msg) => {
                    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"detail": msg})))
                        .into_response()
                }
                StoreError::Forbidden(msg) => {
                    (StatusCode::FORBIDDEN, Json(serde_json::json!({"detail": msg})))
                        .into_response()
                }
                StoreError::Conflict(msg) => {
                    (StatusCode::CONFLICT, Json(serde_json::json!({"detail": msg})))
                        .into_response()
                }
                StoreError::Infrastructure(msg) => {
                    eprintln!("[torgashka-api] stores: {msg}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"detail": "Внутрішня помилка сервера"})),
                    )
                        .into_response()
                }
            },
        }
    }
}

fn store_svc(state: &AppState) -> Result<std::sync::Arc<dyn StoreService + Send + Sync>, StoreErr> {
    state
        .stores
        .clone()
        .ok_or_else(|| StoreErr::Service(StoreError::BadRequest(
            "Rust-гілка точок вимкнена".to_string(),
        )))
}

/// GET /api/v1/stores → 200 [StoreDto]
pub async fn list_stores(
    State(state): State<AppState>,
) -> Result<Json<Vec<StoreDto>>, StoreErr> {
    let svc = store_svc(&state)?;
    Ok(Json(svc.list_stores().await?))
}

/// POST /api/v1/stores → 201 StoreDto (owner)
pub async fn create_store(
    State(state): State<AppState>,
    Json(body): Json<StoreCreateInput>,
) -> Result<(StatusCode, Json<StoreDto>), StoreErr> {
    let svc = store_svc(&state)?;
    Ok((StatusCode::CREATED, Json(svc.create_store(&body).await?)))
}

/// POST /api/v1/user-stores → 201 StoreDto (owner)
pub async fn assign_user_store(
    State(state): State<AppState>,
    Json(body): Json<UserStoreAssignInput>,
) -> Result<(StatusCode, Json<StoreDto>), StoreErr> {
    let svc = store_svc(&state)?;
    Ok((
        StatusCode::CREATED,
        Json(svc.assign_user_store(&body).await?),
    ))
}

/// GET /api/v1/inventory/availability → 200 [AvailabilityItemDto]
pub async fn availability(
    State(state): State<AppState>,
) -> Result<Json<Vec<AvailabilityItemDto>>, StoreErr> {
    let svc = store_svc(&state)?;
    Ok(Json(svc.availability().await?))
}
