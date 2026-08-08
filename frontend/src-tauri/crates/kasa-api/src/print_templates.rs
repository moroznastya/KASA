// ─────────────────────────────────────────────────────────────────────────────
// print_templates — Rust-гілка друку (етап 8, група 6)
// ─────────────────────────────────────────────────────────────────────────────
// 1:1 з Python:
//   - api/v1/print.py (4 роути, 719 рядків): POST /print/price-tags/render,
//     POST /print/labels/render, GET /print/printers (CUPS lpstat -e,
//     ПУБЛІЧНИЙ — без auth), POST /print/test (receipt/price_tag/label)
//   - api/v1/print_templates.py (9 роутів, 315 рядків): list active (пагінація
//     pages=max(1,ceil)), all (admin), default (is_default → перший активний),
//     get, create (201; is_default знімає з інших типу), update (exclude_unset;
//     is_default → зняти з інших), delete (soft, 204), set-default, render
//     (replace {{var}} + font).
// Авторизація: list/get/default/render — get_current_user; all/create/update/
//   delete/set-default — require_admin; printers — публічний (Python без
//   Depends; додано у PUBLIC_PATHS фасаду).
// Монтуються лише під KASA_RUST_PRINT=1; інакше — fallback на Python :8001.
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

use kasa_domain::print::{
    LabelRenderInput, PriceTagRenderInput, PrintError, PrintTemplateCreateInput,
    PrintTemplateUpdateInput, TestPrintInput,
};

use crate::{auth::Claims, auth_routes::AuthRouteError, AppState};

/// Помилки хендлерів друку → HTTP (1:1 з Python).
#[derive(Debug)]
pub enum PrintErr {
    Service(PrintError),
    /// 422 Pydantic-валідація.
    Validation(Value),
    /// 401/403 — auth.
    Auth(AuthRouteError),
    /// 403 — Rust-гілка вимкнена.
    Forbidden(String),
}

impl From<PrintError> for PrintErr {
    fn from(e: PrintError) -> Self {
        PrintErr::Service(e)
    }
}

impl IntoResponse for PrintErr {
    fn into_response(self) -> Response {
        match self {
            PrintErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            PrintErr::Service(PrintError::NotFound(msg)) => {
                (StatusCode::NOT_FOUND, Json(json!({"detail": msg}))).into_response()
            }
            PrintErr::Service(PrintError::BadRequest(msg)) => {
                (StatusCode::BAD_REQUEST, Json(json!({"detail": msg}))).into_response()
            }
            PrintErr::Service(PrintError::Infrastructure(msg)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"detail": msg})),
            )
                .into_response(),
            PrintErr::Auth(e) => e.into_response(),
            PrintErr::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(json!({"detail": msg}))).into_response()
            }
        }
    }
}

/// require_admin друку (1:1 Python AuthService.require_admin), незалежний від
/// KASA_RUST_AUTH: перевіряє роль через print-пул.
async fn require_admin_print(state: &AppState, claims: &Claims) -> Result<Uuid, PrintErr> {
    let pool = state
        .print_pool
        .clone()
        .ok_or_else(|| PrintErr::Forbidden("Rust-гілка друку вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        PrintErr::Auth(AuthRouteError::Plain(kasa_domain::AuthError::Unauthorized(
            "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
        )))
    })?;
    let row = sqlx::query("SELECT role::text, is_active FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| PrintErr::Service(PrintError::Infrastructure(e.to_string())))?;
    let Some(row) = row else {
        return Err(PrintErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Unauthorized("Користувача не знайдено".to_string()),
        )));
    };
    let is_active: bool = row.get("is_active");
    if !is_active {
        return Err(PrintErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Forbidden("Користувач деактивований".to_string()),
        )));
    }
    let role: String = row.get("role");
    if role != "admin" {
        return Err(PrintErr::Auth(AuthRouteError::Plain(
            kasa_domain::AuthError::Forbidden(
                "Доступ заборонено: потрібна роль адміністратора".to_string(),
            ),
        )));
    }
    Ok(user_id)
}

fn svc(state: &AppState) -> Result<&dyn kasa_domain::print::PrintTemplatesService, PrintErr> {
    match state.print_templates.as_ref() {
        Some(s) => Ok(s.as_ref()),
        None => Err(PrintErr::Forbidden("Rust-гілка друку вимкнена".to_string())),
    }
}

// ─── Pydantic 422-хелпери (1:1 з FastAPI/Pydantic v2) ───────────────────────

/// Python repr числа: 5.0 → 5, 10.25 → 10.25 (Pydantic input).
fn py_num(v: f64) -> Value {
    if v.fract() == 0.0 {
        json!((v as i64))
    } else {
        json!(v)
    }
}

/// Pydantic v2 msg форматує ge/le як int, якщо значення ціле: "to 10".
fn py_bound(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

fn py_ge(body: &str, value: f64, ge: f64) -> Option<PrintErr> {
    (value < ge).then(|| {
        PrintErr::Validation(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["body", body],
            "msg": format!("Input should be greater than or equal to {}", py_bound(ge)),
            "input": py_num(value),
            "ctx": {"ge": ge},
        }]}))
    })
}

fn py_le(body: &str, value: f64, le: f64) -> Option<PrintErr> {
    (value > le).then(|| {
        PrintErr::Validation(json!({"detail": [{
            "type": "less_than_equal",
            "loc": ["body", body],
            "msg": format!("Input should be less than or equal to {}", py_bound(le)),
            "input": py_num(value),
            "ctx": {"le": le},
        }]}))
    })
}

fn py_enum(body: &str, value: &str, expected: &str) -> PrintErr {
    // Python ctx.expected містить лапки: "'code128' or 'qr'".
    PrintErr::Validation(json!({"detail": [{
        "type": "literal_error",
        "loc": ["body", body],
        "msg": format!("Input should be '{expected}'"),
        "input": value,
        "ctx": {"expected": format!("'{expected}'")},
    }]}))
}

// ─── Роути /print/* (Python api/v1/print.py) ────────────────────────────────

/// POST /api/v1/print/price-tags/render (get_current_user).
pub async fn price_tags_render(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(body): Json<PriceTagRenderInput>,
) -> Result<Json<Value>, PrintErr> {
    if body.barcode_type != "code128" && body.barcode_type != "qr" {
        return Err(py_enum(
            "barcode_type",
            &body.barcode_type,
            "code128' or 'qr",
        ));
    }
    if let Some(e) =
        py_ge("width_mm", body.width_mm, 10.0).or_else(|| py_le("width_mm", body.width_mm, 200.0))
    {
        return Err(e);
    }
    if let Some(e) = py_ge("height_mm", body.height_mm, 10.0)
        .or_else(|| py_le("height_mm", body.height_mm, 200.0))
    {
        return Err(e);
    }
    if let Some(e) =
        py_ge("gap_mm", body.gap_mm, 0.0).or_else(|| py_le("gap_mm", body.gap_mm, 20.0))
    {
        return Err(e);
    }
    if let Some(e) =
        py_ge("margin_mm", body.margin_mm, 0.0).or_else(|| py_le("margin_mm", body.margin_mm, 50.0))
    {
        return Err(e);
    }
    if let Some(e) = py_ge("barcode_height_mm", body.barcode_height_mm, 4.0)
        .or_else(|| py_le("barcode_height_mm", body.barcode_height_mm, 40.0))
    {
        return Err(e);
    }
    if body.products.is_empty() {
        return Err(PrintErr::Validation(json!({"detail": [{
            "type": "too_short",
            "loc": ["body", "products"],
            "msg": "List should have at least 1 item after validation, not 0",
            "input": [],
            "ctx": {"field_type": "List", "min_length": 1, "actual_length": 0},
        }]})));
    }
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.render_price_tags(&body).await?).unwrap(),
    ))
}

/// POST /api/v1/print/labels/render (get_current_user).
pub async fn labels_render(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(body): Json<LabelRenderInput>,
) -> Result<Json<Value>, PrintErr> {
    if body.barcode_type != "code128" && body.barcode_type != "qr" {
        return Err(py_enum(
            "barcode_type",
            &body.barcode_type,
            "code128' or 'qr",
        ));
    }
    if body.print_mode != "system" && body.print_mode != "escpos" {
        return Err(py_enum(
            "print_mode",
            &body.print_mode,
            "system' or 'escpos",
        ));
    }
    if let Some(e) =
        py_ge("width_mm", body.width_mm, 20.0).or_else(|| py_le("width_mm", body.width_mm, 120.0))
    {
        return Err(e);
    }
    if let Some(e) = py_ge("height_mm", body.height_mm, 10.0)
        .or_else(|| py_le("height_mm", body.height_mm, 200.0))
    {
        return Err(e);
    }
    if let Some(e) =
        py_ge("gap_mm", body.gap_mm, 0.0).or_else(|| py_le("gap_mm", body.gap_mm, 20.0))
    {
        return Err(e);
    }
    if let Some(e) = py_ge("barcode_height_mm", body.barcode_height_mm, 4.0)
        .or_else(|| py_le("barcode_height_mm", body.barcode_height_mm, 40.0))
    {
        return Err(e);
    }
    if body.products.is_empty() {
        return Err(PrintErr::Validation(json!({"detail": [{
            "type": "too_short",
            "loc": ["body", "products"],
            "msg": "List should have at least 1 item after validation, not 0",
            "input": [],
            "ctx": {"field_type": "List", "min_length": 1, "actual_length": 0},
        }]})));
    }
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.render_labels(&body).await?).unwrap(),
    ))
}

/// GET /api/v1/print/printers — ПУБЛІЧНИЙ (Python без Depends).
pub async fn printers() -> Result<Json<Value>, PrintErr> {
    let printers = crate::print_templates::list_printers_public().await;
    Ok(Json(json!({"printers": printers})))
}

async fn list_printers_public() -> Vec<String> {
    kasa_infrastructure::repositories::print_templates::list_printers().await
}

/// POST /api/v1/print/test (get_current_user).
pub async fn test_print(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(body): Json<TestPrintInput>,
) -> Result<Json<Value>, PrintErr> {
    if body.print_type != "receipt" && body.print_type != "price_tag" && body.print_type != "label"
    {
        return Err(py_enum(
            "print_type",
            &body.print_type,
            "receipt', 'price_tag' or 'label",
        ));
    }
    if body.barcode_type != "code128" && body.barcode_type != "qr" {
        return Err(py_enum(
            "barcode_type",
            &body.barcode_type,
            "code128' or 'qr",
        ));
    }
    for (name, v) in [("width_mm", body.width_mm), ("height_mm", body.height_mm)] {
        if let Some(v) = v {
            if let Some(e) = py_ge(name, v, 10.0).or_else(|| py_le(name, v, 200.0)) {
                return Err(e);
            }
        }
    }
    if let Some(v) = body.gap_mm {
        if let Some(e) = py_ge("gap_mm", v, 0.0).or_else(|| py_le("gap_mm", v, 20.0)) {
            return Err(e);
        }
    }
    if let Some(v) = body.margin_mm {
        if let Some(e) = py_ge("margin_mm", v, 0.0).or_else(|| py_le("margin_mm", v, 50.0)) {
            return Err(e);
        }
    }
    if let Some(e) = py_ge("barcode_height_mm", body.barcode_height_mm, 4.0)
        .or_else(|| py_le("barcode_height_mm", body.barcode_height_mm, 40.0))
    {
        return Err(e);
    }
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.test_print(&body).await?).unwrap(),
    ))
}

// ─── Роути /print-templates/* (Python api/v1/print_templates.py) ────────────

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
}

/// GET /api/v1/print-templates (get_current_user; пагінація).
pub async fn list_templates(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, PrintErr> {
    let page = q.page.unwrap_or(1);
    let size = q.size.unwrap_or(50);
    if page < 1 {
        return Err(PrintErr::Validation(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["query", "page"],
            "msg": "Input should be greater than or equal to 1",
            "input": page.to_string(),
            "ctx": {"ge": 1},
        }]})));
    }
    if !(1..=1000).contains(&size) {
        return Err(PrintErr::Validation(json!({"detail": [{
            "type": if size < 1 { "greater_than_equal" } else { "less_than_equal" },
            "loc": ["query", "size"],
            "msg": if size < 1 {
                "Input should be greater than or equal to 1"
            } else {
                "Input should be less than or equal to 1000"
            },
            "input": size.to_string(),
            "ctx": if size < 1 { json!({"ge": 1}) } else { json!({"le": 1000}) },
        }]})));
    }
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.list_active(page, size).await?).unwrap(),
    ))
}

/// GET /api/v1/print-templates/all (admin).
pub async fn list_all(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, PrintErr> {
    require_admin_print(&state, &claims).await?;
    let s = svc(&state)?;
    Ok(Json(serde_json::to_value(s.list_all().await?).unwrap()))
}

#[derive(Debug, Deserialize)]
pub struct DefaultQuery {
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

/// GET /api/v1/print-templates/default?type= (get_current_user).
pub async fn get_default(
    State(state): State<AppState>,
    Query(q): Query<DefaultQuery>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, PrintErr> {
    let Some(type_) = q.type_ else {
        return Err(PrintErr::Validation(json!({"detail": [{
            "type": "missing",
            "loc": ["query", "type"],
            "msg": "Field required",
            "input": null,
        }]})));
    };
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.get_default(&type_).await?).unwrap(),
    ))
}

/// GET /api/v1/print-templates/{id} (get_current_user).
pub async fn get_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, PrintErr> {
    let s = svc(&state)?;
    Ok(Json(serde_json::to_value(s.get(id).await?).unwrap()))
}

/// POST /api/v1/print-templates (admin) → 201.
pub async fn create_template(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PrintTemplateCreateInput>,
) -> Result<Response, PrintErr> {
    require_admin_print(&state, &claims).await?;
    if body.name.is_empty() {
        return Err(PrintErr::Validation(json!({"detail": [{
            "type": "string_too_short",
            "loc": ["body", "name"],
            "msg": "String should have at least 1 character",
            "input": "",
            "ctx": {"min_length": 1},
        }]})));
    }
    if body.name.chars().count() > 255 {
        return Err(PrintErr::Validation(json!({"detail": [{
            "type": "string_too_long",
            "loc": ["body", "name"],
            "msg": "String should have at most 255 characters",
            "input": body.name,
            "ctx": {"max_length": 255},
        }]})));
    }
    let s = svc(&state)?;
    let dto = s.create(&body).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(dto).unwrap()),
    )
        .into_response())
}

/// PUT /api/v1/print-templates/{id} (admin).
pub async fn update_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PrintTemplateUpdateInput>,
) -> Result<Json<Value>, PrintErr> {
    require_admin_print(&state, &claims).await?;
    if let Some(name) = &body.name {
        if name.is_empty() {
            return Err(PrintErr::Validation(json!({"detail": [{
                "type": "string_too_short",
                "loc": ["body", "name"],
                "msg": "String should have at least 1 character",
                "input": "",
                "ctx": {"min_length": 1},
            }]})));
        }
        if name.chars().count() > 255 {
            return Err(PrintErr::Validation(json!({"detail": [{
                "type": "string_too_long",
                "loc": ["body", "name"],
                "msg": "String should have at most 255 characters",
                "input": name,
                "ctx": {"max_length": 255},
            }]})));
        }
    }
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.update(id, &body).await?).unwrap(),
    ))
}

/// DELETE /api/v1/print-templates/{id} (admin, soft delete) → 204.
pub async fn delete_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
) -> Result<StatusCode, PrintErr> {
    require_admin_print(&state, &claims).await?;
    let s = svc(&state)?;
    s.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/print-templates/{id}/set-default (admin).
pub async fn set_default(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, PrintErr> {
    require_admin_print(&state, &claims).await?;
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.set_default(id).await?).unwrap(),
    ))
}

/// POST /api/v1/print-templates/{id}/render (get_current_user).
pub async fn render_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(_claims): Extension<Claims>,
    Json(body): Json<kasa_domain::print::TemplateRenderInput>,
) -> Result<Json<Value>, PrintErr> {
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.render_template(id, &body.data).await?).unwrap(),
    ))
}
