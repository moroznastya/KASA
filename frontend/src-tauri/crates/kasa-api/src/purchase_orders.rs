// ─────────────────────────────────────────────────────────────────────────────
// purchase_orders — Rust-гілка замовлень постачальнику (етап 8, група 5)
// ─────────────────────────────────────────────────────────────────────────────
// 1:1 з Python api/v1/purchase_orders.py (6 роутів, 416 рядків):
//   list, get, create, update, delete, confirm (confirmed → Invoice DRAFT
//   з копією позицій; cancelled → статус CANCELLED).
// Авторизація: list/get — get_current_user (будь-яка JWT-роль);
//   create/update/delete/confirm — require_admin (як Python).
// Монтуються лише під KASA_RUST_PURCHASE_ORDERS=1; інакше — fallback на
// Python :8001.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use kasa_domain::purchase_orders::{
    PurchaseOrderCreateInput, PurchaseOrderUpdateInput, PurchaseOrdersError,
};

use crate::{auth::Claims, auth_routes::AuthRouteError, AppState};

/// Помилки хендлерів замовлень → HTTP (1:1 з Python).
#[derive(Debug)]
pub enum PoErr {
    Service(PurchaseOrdersError),
    /// 422 Pydantic-валідація.
    Validation(Value),
    /// 401/403 — auth.
    Auth(AuthRouteError),
    /// 403 — Rust-гілка вимкнена.
    Forbidden(String),
}

impl From<PurchaseOrdersError> for PoErr {
    fn from(e: PurchaseOrdersError) -> Self {
        PoErr::Service(e)
    }
}

impl IntoResponse for PoErr {
    fn into_response(self) -> Response {
        match self {
            PoErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            PoErr::Service(PurchaseOrdersError::NotFound(msg)) => {
                (StatusCode::NOT_FOUND, Json(json!({"detail": msg}))).into_response()
            }
            PoErr::Service(PurchaseOrdersError::BadRequest(msg)) => {
                (StatusCode::BAD_REQUEST, Json(json!({"detail": msg}))).into_response()
            }
            PoErr::Service(PurchaseOrdersError::Infrastructure(msg)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"detail": msg})),
            )
                .into_response(),
            PoErr::Auth(e) => e.into_response(),
            PoErr::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(json!({"detail": msg}))).into_response()
            }
        }
    }
}

/// require_admin замовлень (1:1 Python AuthService.require_admin), незалежний
/// від KASA_RUST_AUTH: перевіряє роль через purchase_orders-пул.
async fn require_admin_po(state: &AppState, claims: &Claims) -> Result<Uuid, PoErr> {
    let pool = state
        .purchase_orders_pool
        .clone()
        .ok_or_else(|| PoErr::Forbidden("Rust-гілка замовлень вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        PoErr::Auth(AuthRouteError::Plain(kasa_domain::AuthError::Unauthorized(
            "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
        )))
    })?;
    let row = sqlx::query("SELECT role::text, is_active FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| PoErr::Service(PurchaseOrdersError::Infrastructure(e.to_string())))?;
    let Some(row) = row else {
        return Err(PoErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Unauthorized("Користувача не знайдено".to_string()),
        )));
    };
    let is_active: bool = row.get("is_active");
    if !is_active {
        return Err(PoErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Forbidden("Користувач деактивований".to_string()),
        )));
    }
    let role: String = row.get("role");
    if role != "admin" {
        return Err(PoErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Forbidden(
                "Доступ заборонено: потрібна роль адміністратора".to_string(),
            ),
        )));
    }
    Ok(user_id)
}

#[derive(Deserialize)]
pub struct PoListQuery {
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

/// GET /api/v1/purchase-orders (get_current_user).
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<PoListQuery>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, PoErr> {
    // Python Query(ge=1, le=1000) → 422 Pydantic.
    if q.page < 1 {
        return Err(PoErr::Validation(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["query", "page"],
            "msg": "Input should be greater than or equal to 1",
            "input": q.page,
        }]})));
    }
    if q.size < 1 || q.size > 1000 {
        return Err(PoErr::Validation(json!({"detail": [{
            "type": if q.size < 1 { "greater_than_equal" } else { "less_than_equal" },
            "loc": ["query", "size"],
            "msg": if q.size < 1 {
                "Input should be greater than or equal to 1"
            } else {
                "Input should be less than or equal to 1000"
            },
            "input": q.size,
        }]})));
    }
    let svc = state
        .purchase_orders
        .as_ref()
        .ok_or_else(|| PoErr::Forbidden("Rust-гілка замовлень вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.list(q.page, q.size).await?).unwrap(),
    ))
}

/// GET /api/v1/purchase-orders/{order_id} (get_current_user).
pub async fn get(
    State(state): State<AppState>,
    Path(order_id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, PoErr> {
    let svc = state
        .purchase_orders
        .as_ref()
        .ok_or_else(|| PoErr::Forbidden("Rust-гілка замовлень вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.get(order_id).await?).unwrap(),
    ))
}

/// POST /api/v1/purchase-orders (require_admin) → 201.
pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PurchaseOrderCreateInput>,
) -> Result<(StatusCode, Json<Value>), PoErr> {
    let svc = state
        .purchase_orders
        .as_ref()
        .ok_or_else(|| PoErr::Forbidden("Rust-гілка замовлень вимкнена".to_string()))?;
    let user_id = require_admin_po(&state, &claims).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(svc.create(&body, user_id).await?).unwrap()),
    ))
}

/// PUT /api/v1/purchase-orders/{order_id} (require_admin).
pub async fn update(
    State(state): State<AppState>,
    Path(order_id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PurchaseOrderUpdateInput>,
) -> Result<Json<Value>, PoErr> {
    let svc = state
        .purchase_orders
        .as_ref()
        .ok_or_else(|| PoErr::Forbidden("Rust-гілка замовлень вимкнена".to_string()))?;
    let user_id = require_admin_po(&state, &claims).await?;
    let _ = user_id;
    Ok(Json(
        serde_json::to_value(svc.update(order_id, &body).await?).unwrap(),
    ))
}

/// DELETE /api/v1/purchase-orders/{order_id} (require_admin) → 204.
pub async fn delete(
    State(state): State<AppState>,
    Path(order_id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
) -> Result<StatusCode, PoErr> {
    let svc = state
        .purchase_orders
        .as_ref()
        .ok_or_else(|| PoErr::Forbidden("Rust-гілка замовлень вимкнена".to_string()))?;
    let user_id = require_admin_po(&state, &claims).await?;
    let _ = user_id;
    svc.delete(order_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PoConfirmBody {
    pub status: String,
}

/// POST /api/v1/purchase-orders/{order_id}/confirm (require_admin).
pub async fn confirm(
    State(state): State<AppState>,
    Path(order_id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PoConfirmBody>,
) -> Result<Json<Value>, PoErr> {
    // Python: PurchaseOrderStatus enum валідація → 422 Pydantic для невідомих.
    if body.status != "draft" && body.status != "confirmed" && body.status != "cancelled" {
        return Err(PoErr::Validation(json!({"detail": [{
            "type": "enum",
            "loc": ["body", "status"],
            "msg": "Input should be 'draft', 'confirmed' or 'cancelled'",
            "input": body.status,
        }]})));
    }
    let svc = state
        .purchase_orders
        .as_ref()
        .ok_or_else(|| PoErr::Forbidden("Rust-гілка замовлень вимкнена".to_string()))?;
    let user_id = require_admin_po(&state, &claims).await?;
    Ok(Json(
        serde_json::to_value(svc.confirm(order_id, &body.status, user_id).await?).unwrap(),
    ))
}
