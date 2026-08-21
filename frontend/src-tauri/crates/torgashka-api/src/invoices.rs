// ─────────────────────────────────────────────────────────────────────────────
// invoices — Rust-гілка інвойсів (етап 8, група 3: v1+v2)
// ─────────────────────────────────────────────────────────────────────────────
// 1:1 з Python:
//   v1 (api/v1/invoices.py, 9 роутів): list, get, create, update, delete,
//     payment-info, confirm, print-items, price-changes — детальна відповідь.
//   v2 (api/v2/invoices.py, 10 роутів): list, get, create, confirm, update,
//     delete, payment-info, price-changes, print-items, cancel — компактна.
// АНОМАЛІЯ: v2 create/confirm/cancel у Python кидають 500 (entity/ORM);
//   Rust реалізує задуману робочу семантику.
// Авторизація: v1 — get_current_user (list/get/create/payment/price/print),
//   require_admin (update/delete/confirm) — як Python; v2 — глобальний
//   AuthMiddleware (будь-яка JWT-роль), require_admin не потрібен.
// Монтуються лише під TORGASHKA_RUST_INVOICES=1; інакше — fallback → 410 (дезактивація).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::NaiveDateTime;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use torgashka_domain::invoices::{
    InvoiceCreateV1Input, InvoiceCreateV2Input, InvoicePrintRequest, InvoiceUpdateV1Input,
    InvoiceUpdateV2Input, InvoicesError,
};

use crate::{auth::Claims, auth_routes::AuthRouteError, AppState};

/// Помилки хендлерів інвойсів → HTTP (1:1 з Python).
#[derive(Debug)]
pub enum InvErr {
    Service(InvoicesError),
    /// 422 Pydantic-валідація.
    Validation(Value),
    /// 401/403 — auth.
    Auth(AuthRouteError),
    /// 403 — Rust-гілка вимкнена.
    Forbidden(String),
}

impl From<InvoicesError> for InvErr {
    fn from(e: InvoicesError) -> Self {
        InvErr::Service(e)
    }
}

impl IntoResponse for InvErr {
    fn into_response(self) -> Response {
        match self {
            InvErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            InvErr::Service(InvoicesError::NotFound(msg)) => {
                (StatusCode::NOT_FOUND, Json(json!({"detail": msg}))).into_response()
            }
            InvErr::Service(InvoicesError::BadRequest(msg)) => {
                (StatusCode::BAD_REQUEST, Json(json!({"detail": msg}))).into_response()
            }
            InvErr::Service(InvoicesError::Infrastructure(msg)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"detail": msg})),
            )
                .into_response(),
            InvErr::Auth(e) => e.into_response(),
            InvErr::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(json!({"detail": msg}))).into_response()
            }
        }
    }
}

/// require_admin інвойсів (1:1 Python AuthService.require_admin), незалежний
/// від TORGASHKA_RUST_AUTH: перевіряє роль через invoices-пул.
async fn require_admin_inv(state: &AppState, claims: &Claims) -> Result<Uuid, InvErr> {
    let pool = state
        .invoices_pool
        .clone()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        InvErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Unauthorized(
                "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
            ),
        ))
    })?;
    let row = sqlx::query("SELECT role::text, is_active FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| InvErr::Service(InvoicesError::Infrastructure(e.to_string())))?;
    let Some(row) = row else {
        return Err(InvErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Unauthorized("Користувача не знайдено".to_string()),
        )));
    };
    let is_active: bool = row.get("is_active");
    if !is_active {
        return Err(InvErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Forbidden("Користувач деактивований".to_string()),
        )));
    }
    let role: String = row.get("role");
    if !matches!(role.as_str(), "admin" | "owner") {
        return Err(InvErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Forbidden(
                "Доступ заборонено: потрібна роль адміністратора".to_string(),
            ),
        )));
    }
    Ok(user_id)
}

// ─── v1 ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct V1ListQuery {
    pub supplier_id: Option<Uuid>,
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_size_v1")]
    pub size: i64,
}
fn default_page() -> i64 {
    1
}
fn default_size_v1() -> i64 {
    50
}

#[derive(Deserialize)]
pub struct V1ConfirmBody {
    pub status: String,
}

/// GET /api/v1/invoices (get_current_user).
pub async fn v1_list(
    State(state): State<AppState>,
    Query(q): Query<V1ListQuery>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    // Python Query(ge=1, le=1000) → 422 Pydantic.
    if q.page < 1 {
        return Err(InvErr::Validation(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["query", "page"],
            "msg": "Input should be greater than or equal to 1",
            "input": q.page,
        }]})));
    }
    if q.size < 1 {
        return Err(InvErr::Validation(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["query", "size"],
            "msg": "Input should be greater than or equal to 1",
            "input": q.size,
        }]})));
    }
    if q.size > 1000 {
        return Err(InvErr::Validation(json!({"detail": [{
            "type": "less_than_equal",
            "loc": ["query", "size"],
            "msg": "Input should be less than or equal to 1000",
            "input": q.size,
        }]})));
    }
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    let out = svc.list_v1(q.supplier_id, q.page, q.size).await?;
    Ok(Json(serde_json::to_value(out).unwrap()))
}

/// GET /api/v1/invoices/{id}.
pub async fn v1_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(serde_json::to_value(svc.get_v1(id).await?).unwrap()))
}

/// POST /api/v1/invoices → 201 (get_current_user).
pub async fn v1_create(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<InvoiceCreateV1Input>,
) -> Result<impl IntoResponse, InvErr> {
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        InvErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Unauthorized(
                "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
            ),
        ))
    })?;
    let out = svc.create_v1(&input, user_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(out).unwrap()),
    ))
}

/// PUT /api/v1/invoices/{id} (require_admin).
pub async fn v1_update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<InvoiceUpdateV1Input>,
) -> Result<Json<Value>, InvErr> {
    require_admin_inv(&state, &claims).await?;
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.update_v1(id, &input).await?).unwrap(),
    ))
}

/// DELETE /api/v1/invoices/{id} → 204 (require_admin).
pub async fn v1_delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, InvErr> {
    require_admin_inv(&state, &claims).await?;
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    svc.delete_v1(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/invoices/{id}/payment-info.
pub async fn v1_payment_info(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.payment_info_v1(id).await?).unwrap(),
    ))
}

/// POST /api/v1/invoices/{id}/confirm {status} (require_admin).
pub async fn v1_confirm(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<V1ConfirmBody>,
) -> Result<Json<Value>, InvErr> {
    require_admin_inv(&state, &claims).await?;
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.confirm_v1(id, &body.status).await?).unwrap(),
    ))
}

/// GET /api/v1/invoices/{id}/price-changes.
pub async fn v1_price_changes(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.price_changes(id).await?).unwrap(),
    ))
}

/// POST /api/v1/invoices/{id}/print-items (get_current_user).
pub async fn v1_print_items(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<InvoicePrintRequest>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v1
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.print_items(id, &input).await?).unwrap(),
    ))
}

// ─── v2 ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct V2ListQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_size_v2")]
    pub size: i64,
    pub search: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub status: Option<String>,
    pub date_from: Option<NaiveDateTime>,
    pub date_to: Option<NaiveDateTime>,
}
fn default_size_v2() -> i64 {
    20
}

#[derive(Deserialize)]
pub struct V2ConfirmBody {
    pub invoice_id: Uuid,
}

/// GET /api/v2/invoices (глобальний AuthMiddleware).
pub async fn v2_list(
    State(state): State<AppState>,
    Query(q): Query<V2ListQuery>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    let out = svc
        .list_v2(
            q.search,
            q.supplier_id,
            q.status,
            q.date_from,
            q.date_to,
            q.page,
            q.size,
        )
        .await?;
    Ok(Json(serde_json::to_value(out).unwrap()))
}

/// GET /api/v2/invoices/{id}.
pub async fn v2_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(serde_json::to_value(svc.get_v2(id).await?).unwrap()))
}

/// POST /api/v2/invoices → 201.
pub async fn v2_create(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<InvoiceCreateV2Input>,
) -> Result<impl IntoResponse, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    let out = svc.create_v2(&input).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(out).unwrap()),
    ))
}

/// POST /api/v2/invoices/confirm {invoice_id}.
pub async fn v2_confirm(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(body): Json<V2ConfirmBody>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.confirm_v2(body.invoice_id).await?).unwrap(),
    ))
}

/// PUT /api/v2/invoices/{id}.
pub async fn v2_update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<InvoiceUpdateV2Input>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.update_v2(id, &input).await?).unwrap(),
    ))
}

/// DELETE /api/v2/invoices/{id} → 204.
pub async fn v2_delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<impl IntoResponse, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    svc.delete_v2(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v2/invoices/{id}/payment-info.
pub async fn v2_payment_info(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.payment_info_v2(id).await?).unwrap(),
    ))
}

/// GET /api/v2/invoices/{id}/price-changes.
pub async fn v2_price_changes(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    let mut out = svc.price_changes_v2(id).await?;
    // Python v2: article = getattr(sku, "") or "" → "" для None.
    for item in out.iter_mut() {
        if item.article.is_none() {
            item.article = Some(String::new());
        }
    }
    Ok(Json(serde_json::to_value(out).unwrap()))
}

/// POST /api/v2/invoices/{id}/print-items.
pub async fn v2_print_items(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<InvoicePrintRequest>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.print_items_v2(id, &input).await?).unwrap(),
    ))
}

/// POST /api/v2/invoices/{id}/cancel.
pub async fn v2_cancel(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, InvErr> {
    let svc = state
        .invoices_v2
        .as_ref()
        .ok_or_else(|| InvErr::Forbidden("Rust-гілка інвойсів вимкнена".to_string()))?;
    Ok(Json(
        serde_json::to_value(svc.cancel_v2(id).await?).unwrap(),
    ))
}
