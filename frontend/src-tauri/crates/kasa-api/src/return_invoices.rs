//! Роути повернень постачальнику (етап 8 — група 4): 1:1 з Python
//! backend/app/api/v1/return_invoices.py (7 роутів, 428 рядків).
//!
//! require_admin — через окремий return_invoices_pool (незалежно від
//! KASA_RUST_AUTH), як у інвойсів (група 3).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use kasa_domain::return_invoices::{
    ReturnInvoiceConfirmInput, ReturnInvoiceCreateInput, ReturnInvoiceUpdateInput,
    ReturnInvoicesError,
};

use crate::{auth::Claims, auth_routes::AuthRouteError, AppState};

/// Помилки роутів повернень (1:1 HTTP-статуси Python).
pub enum RetErr {
    Service(ReturnInvoicesError),
    /// 422 Pydantic-валідація.
    Validation(Value),
    /// 401/403 — auth.
    Auth(AuthRouteError),
    /// 403 — Rust-гілка вимкнена.
    Forbidden(String),
}

impl From<ReturnInvoicesError> for RetErr {
    fn from(e: ReturnInvoicesError) -> Self {
        RetErr::Service(e)
    }
}

impl IntoResponse for RetErr {
    fn into_response(self) -> Response {
        match self {
            RetErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            RetErr::Service(ReturnInvoicesError::NotFound(msg)) => {
                (StatusCode::NOT_FOUND, Json(json!({"detail": msg}))).into_response()
            }
            RetErr::Service(ReturnInvoicesError::BadRequest(msg)) => {
                (StatusCode::BAD_REQUEST, Json(json!({"detail": msg}))).into_response()
            }
            RetErr::Service(ReturnInvoicesError::Infrastructure(msg)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"detail": msg})),
            )
                .into_response(),
            RetErr::Auth(e) => e.into_response(),
            RetErr::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(json!({"detail": msg}))).into_response()
            }
        }
    }
}

/// require_admin повернень (1:1 Python AuthService.require_admin) через
/// окремий пул — незалежно від KASA_RUST_AUTH.
async fn require_admin_ret(state: &AppState, claims: &Claims) -> Result<Uuid, RetErr> {
    let pool = state
        .return_invoices_pool
        .clone()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        RetErr::Auth(AuthRouteError::Plain(kasa_domain::AuthError::Unauthorized(
            "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
        )))
    })?;
    let row = sqlx::query("SELECT role::text, is_active FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| RetErr::Service(ReturnInvoicesError::Infrastructure(e.to_string())))?;
    let Some(row) = row else {
        return Err(RetErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Unauthorized("Користувача не знайдено".to_string()),
        )));
    };
    let is_active: bool = row.get("is_active");
    if !is_active {
        return Err(RetErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Forbidden("Користувач деактивований".to_string()),
        )));
    }
    let role: String = row.get("role");
    if role != "admin" {
        return Err(RetErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Forbidden(
                "Доступ заборонено: потрібна роль адміністратора".to_string(),
            ),
        )));
    }
    Ok(user_id)
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_size")]
    pub size: i64,
}
fn default_page() -> i64 {
    1
}
fn default_size() -> i64 {
    50
}

#[derive(Deserialize)]
pub struct ConfirmBody {
    pub status: String,
    pub exchange_items: Option<Vec<kasa_domain::return_invoices::ExchangeItemCreateInput>>,
}

/// GET /api/v1/return-invoices (get_current_user).
pub async fn list_returns(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, RetErr> {
    // Python Query(ge=1, le=1000) → 422 Pydantic.
    if q.page < 1 {
        return Err(RetErr::Validation(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["query", "page"],
            "msg": "Input should be greater than or equal to 1",
            "input": q.page,
        }]})));
    }
    if q.size < 1 {
        return Err(RetErr::Validation(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["query", "size"],
            "msg": "Input should be greater than or equal to 1",
            "input": q.size,
        }]})));
    }
    if q.size > 1000 {
        return Err(RetErr::Validation(json!({"detail": [{
            "type": "less_than_equal",
            "loc": ["query", "size"],
            "msg": "Input should be less than or equal to 1000",
            "input": q.size,
        }]})));
    }
    let svc = state
        .return_invoices
        .as_ref()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    let out = svc.list(q.page, q.size).await?;
    Ok(Json(serde_json::to_value(out).unwrap()))
}

/// GET /api/v1/return-invoices/{return_id}.
pub async fn get_return(
    State(state): State<AppState>,
    Path(return_id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, RetErr> {
    let svc = state
        .return_invoices
        .as_ref()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.get(return_id).await?).unwrap(),
    ))
}

/// POST /api/v1/return-invoices → 201 (require_admin).
pub async fn create_return(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<ReturnInvoiceCreateInput>,
) -> Result<impl IntoResponse, RetErr> {
    let svc = state
        .return_invoices
        .as_ref()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    let user_id = require_admin_ret(&state, &claims).await?;
    let out = svc.create(&input, user_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(out).unwrap()),
    ))
}

/// PUT /api/v1/return-invoices/{return_id} (require_admin).
pub async fn update_return(
    State(state): State<AppState>,
    Path(return_id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<ReturnInvoiceUpdateInput>,
) -> Result<Json<Value>, RetErr> {
    let svc = state
        .return_invoices
        .as_ref()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    require_admin_ret(&state, &claims).await?;
    Ok(Json(
        serde_json::to_value(svc.update(return_id, &input).await?).unwrap(),
    ))
}

/// DELETE /api/v1/return-invoices/{return_id} → 204 (require_admin).
pub async fn delete_return(
    State(state): State<AppState>,
    Path(return_id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, RetErr> {
    let svc = state
        .return_invoices
        .as_ref()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    require_admin_ret(&state, &claims).await?;
    svc.delete(return_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/return-invoices/{return_id}/confirm {status} (require_admin).
pub async fn confirm_return(
    State(state): State<AppState>,
    Path(return_id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ConfirmBody>,
) -> Result<Json<Value>, RetErr> {
    // Python: ReturnInvoiceStatus enum валідація → 422 Pydantic для невідомих.
    if body.status != "draft" && body.status != "confirmed" && body.status != "cancelled" {
        return Err(RetErr::Validation(json!({"detail": [{
            "type": "enum",
            "loc": ["body", "status"],
            "msg": "Input should be 'draft', 'confirmed' or 'cancelled'",
            "input": body.status,
        }]})));
    }
    let svc = state
        .return_invoices
        .as_ref()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    let user_id = require_admin_ret(&state, &claims).await?;
    let input = ReturnInvoiceConfirmInput {
        status: body.status,
        exchange_items: body.exchange_items,
    };
    Ok(Json(
        serde_json::to_value(svc.confirm(return_id, &input, user_id).await?).unwrap(),
    ))
}

/// POST /api/v1/return-invoices/{return_id}/cancel (require_admin).
pub async fn cancel_return(
    State(state): State<AppState>,
    Path(return_id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, RetErr> {
    let svc = state
        .return_invoices
        .as_ref()
        .ok_or_else(|| RetErr::Forbidden("Rust-гілка повернень вимкнена".to_string()))?;
    require_admin_ret(&state, &claims).await?;
    Ok(Json(
        serde_json::to_value(svc.cancel(return_id).await?).unwrap(),
    ))
}

/// Монтаж роутів (викликається з router_v1, якщо return_invoices увімкнено).
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/return-invoices",
            get(list_returns).post(create_return),
        )
        .route(
            "/api/v1/return-invoices/:return_id",
            get(get_return).put(update_return).delete(delete_return),
        )
        .route(
            "/api/v1/return-invoices/:return_id/confirm",
            post(confirm_return),
        )
    // Python НЕ має окремого /cancel — cancel через confirm {status:cancelled}.
}
