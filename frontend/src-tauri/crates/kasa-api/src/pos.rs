// ─────────────────────────────────────────────────────────────────────────────
// pos — POS-хендлери (етап 3): чеки v2, робочі сесії, списання, переміщення,
// зміни ПРРО.
// ─────────────────────────────────────────────────────────────────────────────
// Відтворюють Python-еталон 1:1:
//   - /api/v2/receipts/*      → v2/receipts.py + ReceiptUseCases
//   - /api/v1/work-sessions/* → v1/work_sessions.py
//   - /api/v1/write-offs/*    → v1/write_offs.py (+ DocumentService)
//   - /api/v1/transfers/*     → v1/transfers.py (+ DocumentService)
//   - /api/v2/prro/shifts|shift/open|close → v2/prro.py
// Статуси: 201 (create receipt/doc), 200, 204 (delete), 404, 400, 403, 422.
// Авторизація: як Python (JWT на весь роутер; require_admin — POST/PUT/DELETE
// документів, report/user сесій, shift/close).
// Монтуються лише під KASA_RUST_READDIRS=1; інакше — fallback на Python :8001.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, NaiveDateTime};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use kasa_application::PosServiceFacade;
use kasa_domain::{
    DocItemInput, PosError, ReceiptCreateInput, ReceiptItemInput, ReceiptListQuery,
    ReceiptSearchQuery, TransferCreateInput, TransferUpdateInput, WriteOffCreateInput,
    WriteOffUpdateInput,
};

use crate::auth::Claims;
use crate::AppState;

/// Помилки POS-хендлерів → HTTP (1:1 з Python).
#[derive(Debug)]
pub enum PosErr {
    Service(PosError),
    /// 422 Pydantic-валідація (uuid/числа/довжина).
    Validation(serde_json::Value),
    /// 403 — роль не адміністратор.
    Forbidden(String),
    /// 401 — користувача не знайдено.
    Unauthorized(String),
}

impl From<PosError> for PosErr {
    fn from(e: PosError) -> Self {
        PosErr::Service(e)
    }
}

fn v422(vtype: &str, loc: &[&str], msg: &str, input: &str) -> PosErr {
    PosErr::Validation(serde_json::json!({
        "detail": [{
            "type": vtype,
            "loc": loc,
            "msg": msg,
            "input": input,
        }]
    }))
}

impl IntoResponse for PosErr {
    fn into_response(self) -> Response {
        match self {
            PosErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            PosErr::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            PosErr::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            PosErr::Service(e) => match e {
                PosError::NotFound(msg) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                PosError::BadRequest(msg) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                PosError::Validation(msg) => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                PosError::Forbidden(msg) => (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                PosError::Infrastructure(msg) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": format!("Помилка БД: {msg}")})),
                )
                    .into_response(),
            },
        }
    }
}

// ─── Доступ до репозиторію ─────────────────────────────────────────────────

fn pos_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn kasa_domain::PosService + Send + Sync>, PosErr> {
    state
        .pos
        .clone()
        .ok_or_else(|| PosErr::Forbidden("Rust-гілка POS вимкнена".to_string()))
}

/// require_admin (Python AuthService.require_admin → 403).
async fn require_admin(state: &AppState, claims: &Claims) -> Result<(), PosErr> {
    let pool = state
        .write_pool
        .clone()
        .ok_or_else(|| PosErr::Forbidden("Rust-гілка POS вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        PosErr::Unauthorized("Недійсний токен: відсутній ідентифікатор користувача".to_string())
    })?;
    kasa_infrastructure::repositories::write::require_admin_role(&pool, user_id)
        .await
        .map_err(|e| PosErr::Service(PosError::Infrastructure(e.to_string())))
}

fn sub_uuid(claims: &Claims) -> Result<Uuid, PosErr> {
    Uuid::parse_str(&claims.sub).map_err(|_| {
        PosErr::Unauthorized("Недійсний токен: відсутній ідентифікатор користувача".to_string())
    })
}

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, PosErr> {
    Uuid::parse_str(&raw).map_err(|_| {
        v422(
            "uuid_parsing",
            &["path", field],
            "Input should be a valid UUID",
            &raw,
        )
    })
}

/// Парсинг datetime query (ISO naive або RFC3339 з TZ → naive UTC).
fn parse_dt(s: &str) -> Option<NaiveDateTime> {
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(ndt);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_utc());
    }
    None
}

/// Парсинг float (JSON number або рядок) — Pydantic float.
fn field_f64(v: &Value, key: &str) -> Result<Option<f64>, PosErr> {
    let Some(f) = v.get(key) else { return Ok(None) };
    if f.is_null() {
        return Ok(None);
    }
    if let Some(n) = f.as_f64() {
        return Ok(Some(n));
    }
    if let Some(s) = f.as_str() {
        if let Ok(n) = s.parse::<f64>() {
            return Ok(Some(n));
        }
    }
    Err(v422(
        "float_parsing",
        &["body", key],
        "Input should be a valid number",
        &f.to_string(),
    ))
}

fn field_str(v: &Value, key: &str) -> Option<Option<String>> {
    let f = v.get(key)?;
    if f.is_null() {
        Some(None)
    } else {
        Some(Some(f.as_str().unwrap_or("").to_string()))
    }
}

fn field_uuid(v: &Value, key: &str, required: bool) -> Result<Option<Uuid>, PosErr> {
    let Some(f) = v.get(key) else {
        if required {
            return Err(v422("missing", &["body", key], "Field required", ""));
        }
        return Ok(None);
    };
    if f.is_null() {
        return Ok(None);
    }
    let s = f.as_str().ok_or_else(|| {
        v422(
            "uuid_type",
            &["body", key],
            "Input should be a valid UUID",
            &f.to_string(),
        )
    })?;
    Uuid::parse_str(s).map(Some).map_err(|_| {
        v422(
            "uuid_parsing",
            &["body", key],
            "Input should be a valid UUID",
            s,
        )
    })
}

fn parse_receipt_item(v: &Value, idx: usize) -> Result<ReceiptItemInput, PosErr> {
    let prefix = format!("items.{idx}");
    let product_id = field_uuid(v, "product_id", true)?
        .ok_or_else(|| v422("missing", &["body", &prefix], "Field required", ""))?;
    let quantity = field_f64(v, "quantity")?
        .ok_or_else(|| v422("missing", &["body", &prefix], "Field required", ""))?;
    if quantity <= 0.0 {
        return Err(v422(
            "greater_than",
            &["body", &prefix],
            "Input should be greater than 0",
            &quantity.to_string(),
        ));
    }
    let price = field_f64(v, "price")?
        .ok_or_else(|| v422("missing", &["body", &prefix], "Field required", ""))?;
    if price <= 0.0 {
        return Err(v422(
            "greater_than",
            &["body", &prefix],
            "Input should be greater than 0",
            &price.to_string(),
        ));
    }
    let tax_rate = v.get("tax_rate").and_then(|t| t.as_i64()).unwrap_or(20);
    Ok(ReceiptItemInput {
        product_id,
        name: field_str(v, "name").flatten().unwrap_or_default(),
        quantity: format!("{}", quantity),
        price: format!("{}", price),
        tax_rate,
    })
}

fn parse_receipt_create(v: &Value, cashier_id: Option<Uuid>) -> Result<ReceiptCreateInput, PosErr> {
    let items = match v.get("items") {
        Some(Value::Array(arr)) if !arr.is_empty() => arr,
        _ => {
            return Err(v422(
                "too_short",
                &["body", "items"],
                "List should have at least 1 item after validation, not 0",
                "",
            ))
        }
    };
    let items = items
        .iter()
        .enumerate()
        .map(|(i, it)| parse_receipt_item(it, i))
        .collect::<Result<Vec<_>, _>>()?;
    let terminal_created_at = match v.get("terminal_created_at") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => parse_dt(s),
        Some(other) => {
            return Err(v422(
                "datetime_type",
                &["body", "terminal_created_at"],
                "Input should be a valid datetime",
                &other.to_string(),
            ))
        }
    };
    Ok(ReceiptCreateInput {
        items,
        payment_method: field_str(v, "payment_method")
            .flatten()
            .unwrap_or_else(|| "cash".to_string()),
        cash_amount: field_f64(v, "cash_amount")?.map(|f| f.to_string()),
        card_amount: field_f64(v, "card_amount")?.map(|f| f.to_string()),
        customer_id: field_uuid(v, "customer_id", false)?,
        cashier_id,
        notes: field_str(v, "notes").flatten().unwrap_or_default(),
        terminal_rrn: field_str(v, "terminal_rrn").flatten(),
        terminal_approval_code: field_str(v, "terminal_approval_code").flatten(),
        terminal_invoice_number: field_str(v, "terminal_invoice_number").flatten(),
        terminal_transaction_id: field_str(v, "terminal_transaction_id").flatten(),
        terminal_response_code: field_str(v, "terminal_response_code").flatten(),
        terminal_status: field_str(v, "terminal_status").flatten(),
        terminal_receipt: field_str(v, "terminal_receipt").flatten(),
        terminal_card_pan: field_str(v, "terminal_card_pan").flatten(),
        terminal_payment_system: field_str(v, "terminal_payment_system").flatten(),
        terminal_merchant: field_str(v, "terminal_merchant").flatten(),
        terminal_created_at,
        is_fiscal: v
            .get("is_fiscal")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        split_group_id: field_uuid(v, "split_group_id", false)?,
    })
}

// ─── Парсинг документів (write-off/transfer) ───────────────────────────────

fn parse_doc_item(v: &Value, idx: usize) -> Result<DocItemInput, PosErr> {
    let prefix = format!("items.{idx}");
    let product_id = field_uuid(v, "product_id", true)?
        .ok_or_else(|| v422("missing", &["body", &prefix], "Field required", ""))?;
    let quantity = field_f64(v, "quantity")?
        .ok_or_else(|| v422("missing", &["body", &prefix], "Field required", ""))?;
    Ok(DocItemInput {
        product_id,
        quantity: format!("{}", quantity),
        cost_price: field_f64(v, "cost_price")?.map(|f| f.to_string()),
        price: field_f64(v, "price")?.map(|f| f.to_string()),
    })
}

fn parse_items(v: &Value) -> Result<Vec<DocItemInput>, PosErr> {
    let items = match v.get("items") {
        Some(Value::Array(arr)) => arr,
        _ => return Ok(Vec::new()),
    };
    items
        .iter()
        .enumerate()
        .map(|(i, it)| parse_doc_item(it, i))
        .collect()
}

fn parse_doc_date(v: &Value, key: &str) -> Result<Option<NaiveDateTime>, PosErr> {
    let Some(f) = v.get(key) else { return Ok(None) };
    if f.is_null() {
        return Ok(None);
    }
    let s = f.as_str().ok_or_else(|| {
        v422(
            "datetime_type",
            &["body", key],
            "Input should be a valid datetime",
            &f.to_string(),
        )
    })?;
    parse_dt(s).map(Some).ok_or_else(|| {
        v422(
            "datetime_parsing",
            &["body", key],
            "Input should be a valid datetime",
            s,
        )
    })
}

fn parse_write_off_create(v: &Value, created_by: Uuid) -> Result<WriteOffCreateInput, PosErr> {
    let reason = v
        .get("reason")
        .and_then(|r| r.as_str())
        .ok_or_else(|| v422("missing", &["body", "reason"], "Field required", ""))?
        .to_string();
    let date = parse_doc_date(v, "write_off_date")?
        .ok_or_else(|| v422("missing", &["body", "write_off_date"], "Field required", ""))?;
    Ok(WriteOffCreateInput {
        number: field_str(v, "number").flatten(),
        reason,
        write_off_date: date,
        notes: field_str(v, "notes").flatten(),
        created_by,
        items: parse_items(v)?,
    })
}

fn parse_write_off_update(v: &Value) -> Result<WriteOffUpdateInput, PosErr> {
    let items = match v.get("items") {
        None | Some(Value::Null) => None,
        Some(_) => Some(parse_items(v)?),
    };
    Ok(WriteOffUpdateInput {
        number: field_str(v, "number"),
        reason: field_str(v, "reason").flatten(),
        write_off_date: parse_doc_date(v, "write_off_date")?,
        notes: field_str(v, "notes"),
        items,
    })
}

fn parse_transfer_create(v: &Value, created_by: Uuid) -> Result<TransferCreateInput, PosErr> {
    let from = v
        .get("from_location")
        .and_then(|r| r.as_str())
        .ok_or_else(|| v422("missing", &["body", "from_location"], "Field required", ""))?
        .to_string();
    let to = v
        .get("to_location")
        .and_then(|r| r.as_str())
        .ok_or_else(|| v422("missing", &["body", "to_location"], "Field required", ""))?
        .to_string();
    let date = parse_doc_date(v, "transfer_date")?
        .ok_or_else(|| v422("missing", &["body", "transfer_date"], "Field required", ""))?;
    Ok(TransferCreateInput {
        number: field_str(v, "number").flatten(),
        from_location: from,
        to_location: to,
        transfer_date: date,
        notes: field_str(v, "notes").flatten(),
        created_by,
        items: parse_items(v)?,
    })
}

fn parse_transfer_update(v: &Value) -> Result<TransferUpdateInput, PosErr> {
    let items = match v.get("items") {
        None | Some(Value::Null) => None,
        Some(_) => Some(parse_items(v)?),
    };
    Ok(TransferUpdateInput {
        number: field_str(v, "number"),
        from_location: field_str(v, "from_location").flatten(),
        to_location: field_str(v, "to_location").flatten(),
        transfer_date: parse_doc_date(v, "transfer_date")?,
        notes: field_str(v, "notes"),
        items,
    })
}

// ─── Query-параметри ────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
    pub search: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub payment_method: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub receipt_type: Option<String>,
    pub page: Option<i64>,
    pub size: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct MonthQuery {
    pub month: Option<i64>,
    pub year: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LimitQuery {
    pub limit: Option<i64>,
}

// ─── Чеки v2 ────────────────────────────────────────────────────────────────

/// POST /api/v2/receipts/sale → 201
pub async fn create_sale(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::ReceiptDto>), PosErr> {
    let cashier = sub_uuid(&claims).ok();
    let input = parse_receipt_create(&body, cashier)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_sale_receipt(&input).await?),
    ))
}

/// POST /api/v2/receipts/return → 201
pub async fn create_return(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::ReceiptDto>), PosErr> {
    let cashier = sub_uuid(&claims).ok();
    let input = parse_receipt_create(&body, cashier)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_return_receipt(&input).await?),
    ))
}

/// GET /api/v2/receipts
pub async fn list_receipts(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<kasa_domain::ReceiptListDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    let query = ReceiptListQuery {
        page: q.page.unwrap_or(1),
        size: q.size.unwrap_or(20),
        search: q.search,
        date_from: q.date_from.as_deref().and_then(parse_dt),
        date_to: q.date_to.as_deref().and_then(parse_dt),
        payment_method: q.payment_method,
    };
    Ok(Json(svc.list_receipts(&query).await?))
}

/// GET /api/v2/receipts/stats/today
pub async fn today_stats(
    State(state): State<AppState>,
) -> Result<Json<kasa_domain::ReceiptStatsDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.today_stats().await?))
}

/// GET /api/v2/receipts/search
pub async fn search_receipts(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<kasa_domain::ReceiptSearchDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    let query = ReceiptSearchQuery {
        q: q.q.unwrap_or_default(),
        date_from: q.date_from.as_deref().and_then(parse_dt),
        date_to: q.date_to.as_deref().and_then(parse_dt),
        receipt_type: q.receipt_type,
        page: q.page.unwrap_or(1),
        size: q.size.unwrap_or(20),
    };
    Ok(Json(svc.search_receipts(&query).await?))
}

/// GET /api/v2/receipts/by-product/{query}/recent-sales
pub async fn recent_sales(
    State(state): State<AppState>,
    Path(query): Path<String>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Vec<kasa_domain::ProductRecentSalesDto>>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    let limit = q.limit.unwrap_or(5).clamp(1, 20);
    Ok(Json(svc.recent_sales_by_product(&query, limit).await?))
}

/// GET /api/v2/receipts/products/{product_id}/returnable-quantity
pub async fn returnable_quantity(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<kasa_domain::ReturnableQtyDto>, PosErr> {
    let id = path_uuid(product_id, "product_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.returnable_quantity(id).await?))
}

/// GET /api/v2/receipts/{receipt_id}/items
pub async fn receipt_items(
    State(state): State<AppState>,
    Path(receipt_id): Path<String>,
) -> Result<Json<Vec<kasa_domain::ReceiptItemDetailDto>>, PosErr> {
    let id = path_uuid(receipt_id, "receipt_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.receipt_items(id).await?))
}

/// GET /api/v2/receipts/{receipt_id}
pub async fn get_receipt(
    State(state): State<AppState>,
    Path(receipt_id): Path<String>,
) -> Result<Json<kasa_domain::ReceiptDto>, PosErr> {
    let id = path_uuid(receipt_id, "receipt_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.get_receipt(id).await?))
}

// ─── Робочі сесії ──────────────────────────────────────────────────────────

/// GET /api/v1/work-sessions/my
pub async fn my_sessions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<MonthQuery>,
) -> Result<Json<kasa_domain::MySessionsDto>, PosErr> {
    let user_id = sub_uuid(&claims)?;
    let now = chrono::Utc::now().naive_utc();
    let month = q.month.unwrap_or(now.month() as i64);
    let year = q.year.unwrap_or(now.year() as i64);
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.my_sessions(user_id, month, year).await?))
}

/// GET /api/v1/work-sessions/report
pub async fn work_report(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<MonthQuery>,
) -> Result<Json<kasa_domain::WorkReportDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let month = q
        .month
        .ok_or_else(|| v422("missing", &["query", "month"], "Field required", ""))?;
    let year = q
        .year
        .ok_or_else(|| v422("missing", &["query", "year"], "Field required", ""))?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.work_report(month, year).await?))
}

/// GET /api/v1/work-sessions/user/{user_id}
pub async fn user_sessions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
    Query(q): Query<MonthQuery>,
) -> Result<Json<kasa_domain::UserSessionsDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let uid = path_uuid(user_id, "user_id")?;
    let month = q
        .month
        .ok_or_else(|| v422("missing", &["query", "month"], "Field required", ""))?;
    let year = q
        .year
        .ok_or_else(|| v422("missing", &["query", "year"], "Field required", ""))?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.user_sessions(uid, month, year).await?))
}

// ─── Списання ───────────────────────────────────────────────────────────────

/// GET /api/v1/write-offs
pub async fn list_write_offs(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<kasa_domain::WriteOffListDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(
        svc.list_write_offs(q.page.unwrap_or(1), q.size.unwrap_or(50))
            .await?,
    ))
}

/// GET /api/v1/write-offs/{id}
pub async fn get_write_off(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::WriteOffDto>, PosErr> {
    let id = path_uuid(id, "write_off_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.get_write_off(id).await?))
}

/// POST /api/v1/write-offs → 201
pub async fn create_write_off(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::WriteOffDto>), PosErr> {
    require_admin(&state, &claims).await?;
    let user_id = sub_uuid(&claims)?;
    let input = parse_write_off_create(&body, user_id)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_write_off(&input).await?),
    ))
}

/// PUT /api/v1/write-offs/{id}
pub async fn update_write_off(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::WriteOffDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "write_off_id")?;
    let input = parse_write_off_update(&body)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.update_write_off(id, &input).await?))
}

/// DELETE /api/v1/write-offs/{id} → 204
pub async fn delete_write_off(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, PosErr> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "write_off_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    svc.delete_write_off(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/write-offs/{id}/confirm
pub async fn confirm_write_off(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::WriteOffDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "write_off_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.confirm_write_off(id).await?))
}

// ─── Переміщення ────────────────────────────────────────────────────────────

/// GET /api/v1/transfers
pub async fn list_transfers(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<kasa_domain::TransferListDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(
        svc.list_transfers(q.page.unwrap_or(1), q.size.unwrap_or(50))
            .await?,
    ))
}

/// GET /api/v1/transfers/{id}
pub async fn get_transfer(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::TransferDto>, PosErr> {
    let id = path_uuid(id, "transfer_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.get_transfer(id).await?))
}

/// POST /api/v1/transfers → 201
pub async fn create_transfer(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::TransferDto>), PosErr> {
    require_admin(&state, &claims).await?;
    let user_id = sub_uuid(&claims)?;
    let input = parse_transfer_create(&body, user_id)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_transfer(&input).await?),
    ))
}

/// PUT /api/v1/transfers/{id}
pub async fn update_transfer(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::TransferDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "transfer_id")?;
    let input = parse_transfer_update(&body)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.update_transfer(id, &input).await?))
}

/// DELETE /api/v1/transfers/{id} → 204
pub async fn delete_transfer(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, PosErr> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "transfer_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    svc.delete_transfer(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/transfers/{id}/confirm
pub async fn confirm_transfer(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::TransferDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "transfer_id")?;
    let status = body
        .get("status")
        .and_then(|s| s.as_str())
        .ok_or_else(|| v422("missing", &["body", "status"], "Field required", ""))?
        .to_string();
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.confirm_transfer(id, &status).await?))
}

// ─── Зміни ПРРО (X/Z) ───────────────────────────────────────────────────────

/// GET /api/v2/prro/shifts
pub async fn list_shifts(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<kasa_domain::ShiftListDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(
        svc.list_shifts(q.page.unwrap_or(1), q.size.unwrap_or(20))
            .await?,
    ))
}

/// POST /api/v2/prro/shift/open
pub async fn open_shift(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::PrroShiftDto>, PosErr> {
    let comment = body
        .get("comment")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.open_shift(comment).await?))
}

/// POST /api/v2/prro/shift/close
pub async fn close_shift(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::PrroShiftDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let comment = body
        .get("comment")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.close_shift(comment).await?))
}
