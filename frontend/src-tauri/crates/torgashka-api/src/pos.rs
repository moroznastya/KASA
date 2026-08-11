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
// Монтуються лише під TORGASHKA_RUST_READDIRS=1; інакше — fallback → 410 (дезактивація).
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

use torgashka_application::PosServiceFacade;
use torgashka_domain::{
    DebtPaymentInput, DocItemInput, PosError, ReceiptCreateInput, ReceiptItemInput,
    ReceiptListQuery, ReceiptSearchQuery, ReceiptV1CreateInput, ReceiptV1ItemInput,
    TransferCreateInput, TransferUpdateInput, WriteOffCreateInput, WriteOffReasonItem,
    WriteOffReasonsListDto, WriteOffUpdateInput,
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
                PosError::Conflict(msg) => (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                PosError::Infrastructure(msg) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": format!("Помилка БД: {msg}")})),
                )
                    .into_response(),
                PosError::Integrity(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "detail": "Внутрішня помилка сервера",
                        "type": "IntegrityError",
                    })),
                )
                    .into_response(),
            },
        }
    }
}

// ─── Доступ до репозиторію ─────────────────────────────────────────────────

fn pos_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn torgashka_domain::PosService + Send + Sync>, PosErr> {
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
    torgashka_infrastructure::repositories::write::require_admin_role(&pool, user_id)
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

// ─── Парсинг v1 ReceiptCreate (app/schemas/receipt.py) ──────────────────────

/// 422 зі змішаним loc (рядки + числові індекси списків — як Pydantic).
fn v422v(vtype: &str, loc: &[Value], msg: &str, input: &str) -> PosErr {
    PosErr::Validation(serde_json::json!({
        "detail": [{
            "type": vtype,
            "loc": loc,
            "msg": msg,
            "input": input,
        }]
    }))
}

fn s_l(s: &str) -> Value {
    Value::String(s.to_string())
}

/// UUID з об'єкта (required).
fn req_uuid(obj: &Value, key: &str, loc: &[Value]) -> Result<Uuid, PosErr> {
    let f = obj
        .get(key)
        .ok_or_else(|| v422v("missing", loc, "Field required", ""))?;
    if f.is_null() {
        return Err(v422v("missing", loc, "Field required", ""));
    }
    let s = f.as_str().ok_or_else(|| {
        v422v(
            "uuid_type",
            loc,
            "Input should be a valid UUID",
            &f.to_string(),
        )
    })?;
    Uuid::parse_str(s).map_err(|_| v422v("uuid_parsing", loc, "Input should be a valid UUID", s))
}

/// UUID з об'єкта (optional).
fn opt_uuid(obj: &Value, key: &str, loc: &[Value]) -> Result<Option<Uuid>, PosErr> {
    let Some(f) = obj.get(key) else {
        return Ok(None);
    };
    if f.is_null() {
        return Ok(None);
    }
    let s = f.as_str().ok_or_else(|| {
        v422v(
            "uuid_type",
            loc,
            "Input should be a valid UUID",
            &f.to_string(),
        )
    })?;
    Uuid::parse_str(s)
        .map(Some)
        .map_err(|_| v422v("uuid_parsing", loc, "Input should be a valid UUID", s))
}

/// Decimal з об'єкта (required) — зберігає scale, валідує max places.
fn req_decimal(obj: &Value, key: &str, places: usize, loc: &[Value]) -> Result<String, PosErr> {
    let f = obj
        .get(key)
        .ok_or_else(|| v422v("missing", loc, "Field required", ""))?;
    if f.is_null() {
        return Err(v422v("missing", loc, "Field required", ""));
    }
    decimal_str(f, places, loc)
}

/// Decimal з об'єкта (optional).
fn opt_decimal(
    obj: &Value,
    key: &str,
    places: usize,
    loc: &[Value],
) -> Result<Option<String>, PosErr> {
    let Some(f) = obj.get(key) else {
        return Ok(None);
    };
    if f.is_null() {
        return Ok(None);
    }
    decimal_str(f, places, loc).map(Some)
}

fn decimal_str(f: &Value, places: usize, loc: &[Value]) -> Result<String, PosErr> {
    let s = match f {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => {
            return Err(v422v(
                "decimal_type",
                loc,
                "Input should be a valid decimal",
                &f.to_string(),
            ))
        }
    };
    if let Some(dot) = s.find('.') {
        let frac_len = s[dot + 1..].len();
        if frac_len > places {
            return Err(PosErr::Validation(serde_json::json!({
                "detail": [{
                    "type": "decimal_max_places",
                    "loc": loc,
                    "msg": format!("Decimal input should have no more than {places} decimal places"),
                    "input": s,
                    "ctx": { "decimal_places": places },
                }]
            })));
        }
    }
    Ok(s)
}

/// Pydantic v2 bool (lax): true/false/1/0/yes/no/on/off/y/n/t/f.
fn v1_bool(v: &Value, key: &str) -> Result<bool, PosErr> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::String(s)) => Ok(matches!(
            s.to_lowercase().as_str(),
            "true" | "1" | "yes" | "on" | "y" | "t"
        )),
        Some(other) => Err(v422(
            "bool_parsing",
            &["body", key],
            "Input should be a valid boolean",
            &other.to_string(),
        )),
    }
}

fn parse_receipt_v1_item(v: &Value, idx: usize) -> Result<ReceiptV1ItemInput, PosErr> {
    let idx_v = Value::Number(idx.into());
    let loc_pid = [s_l("body"), s_l("items"), idx_v.clone(), s_l("product_id")];
    let loc_qty = [s_l("body"), s_l("items"), idx_v.clone(), s_l("quantity")];
    let loc_price = [s_l("body"), s_l("items"), idx_v.clone(), s_l("price")];
    let loc_total = [s_l("body"), s_l("items"), idx_v, s_l("total")];
    Ok(ReceiptV1ItemInput {
        product_id: req_uuid(v, "product_id", &loc_pid)?,
        quantity: req_decimal(v, "quantity", 3, &loc_qty)?,
        price: req_decimal(v, "price", 2, &loc_price)?,
        total: opt_decimal(v, "total", 2, &loc_total)?,
    })
}

fn parse_receipt_v1(v: &Value, cashier_id: Option<Uuid>) -> Result<ReceiptV1CreateInput, PosErr> {
    let items = match v.get("items") {
        None => Vec::new(),
        Some(Value::Array(arr)) => arr
            .iter()
            .enumerate()
            .map(|(i, it)| parse_receipt_v1_item(it, i))
            .collect::<Result<Vec<_>, _>>()?,
        Some(other) => {
            return Err(v422(
                "list_type",
                &["body", "items"],
                "Input should be a valid list",
                &other.to_string(),
            ))
        }
    };

    let total_amount = match v.get("total_amount") {
        Some(f) if !f.is_null() => decimal_str(f, 2, &[s_l("body"), s_l("total_amount")])?,
        _ => {
            return Err(PosErr::Validation(serde_json::json!({
                "detail": [{
                    "type": "missing",
                    "loc": ["body", "total_amount"],
                    "msg": "Field required",
                    "input": v,
                }]
            })));
        }
    };
    let receipt_type = match v.get("receipt_type") {
        None | Some(Value::Null) => "sale".to_string(),
        Some(Value::String(s)) if s == "sale" || s == "return" => s.clone(),
        Some(other) => {
            return Err(PosErr::Validation(serde_json::json!({
                "detail": [{
                    "type": "enum",
                    "loc": ["body", "receipt_type"],
                    "msg": "Input should be 'sale' or 'return'",
                    "input": other.as_str().map(str::to_string).unwrap_or_else(|| other.to_string()),
                    "ctx": { "expected": "'sale' or 'return'" },
                }]
            })));
        }
    };
    let payment_method = match v.get("payment_method") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if matches!(s.as_str(), "cash" | "card" | "mixed") => {
            Some(s.clone())
        }
        Some(other) => {
            return Err(PosErr::Validation(serde_json::json!({
                "detail": [{
                    "type": "enum",
                    "loc": ["body", "payment_method"],
                    "msg": "Input should be 'cash', 'card' or 'mixed'",
                    "input": other.as_str().map(str::to_string).unwrap_or_else(|| other.to_string()),
                    "ctx": { "expected": "'cash', 'card' or 'mixed'" },
                }]
            })));
        }
    };

    let debt_payment = match v.get("debt_payment") {
        None | Some(Value::Null) => None,
        Some(dp) => {
            let debtor_id = req_uuid(
                dp,
                "debtor_id",
                &[s_l("body"), s_l("debt_payment"), s_l("debtor_id")],
            )?;
            let amount = req_decimal(
                dp,
                "amount",
                2,
                &[s_l("body"), s_l("debt_payment"), s_l("amount")],
            )?;
            Some(DebtPaymentInput { debtor_id, amount })
        }
    };

    let receipt_number = v
        .get("receipt_number")
        .and_then(|f| f.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok(ReceiptV1CreateInput {
        receipt_number,
        receipt_type,
        cashier_id: opt_uuid(v, "cashier_id", &[s_l("body"), s_l("cashier_id")])?.or(cashier_id),
        total_amount,
        paid_amount: opt_decimal(v, "paid_amount", 2, &[s_l("body"), s_l("paid_amount")])?,
        debtor_id: opt_uuid(v, "debtor_id", &[s_l("body"), s_l("debtor_id")])?,
        is_return: v1_bool(v, "is_return")?,
        notes: v.get("notes").and_then(|f| f.as_str()).map(str::to_string),
        original_receipt_id: opt_uuid(
            v,
            "original_receipt_id",
            &[s_l("body"), s_l("original_receipt_id")],
        )?,
        return_reason: v
            .get("return_reason")
            .and_then(|f| f.as_str())
            .map(str::to_string),
        items,
        debt_payment,
        payment_method,
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
) -> Result<(StatusCode, Json<torgashka_domain::ReceiptDto>), PosErr> {
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
) -> Result<(StatusCode, Json<torgashka_domain::ReceiptDto>), PosErr> {
    let cashier = sub_uuid(&claims).ok();
    let input = parse_receipt_create(&body, cashier)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_return_receipt(&input).await?),
    ))
}

/// POST /api/v1/receipts — v1 create_receipt (боргова семантика) → 201.
pub async fn create_receipt_v1(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<torgashka_domain::ReceiptV1Dto>), PosErr> {
    let cashier = sub_uuid(&claims).ok();
    let input = parse_receipt_v1(&body, cashier)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_receipt_v1(&input).await?),
    ))
}

/// GET /api/v2/receipts
pub async fn list_receipts(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<torgashka_domain::ReceiptListDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::ReceiptStatsDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.today_stats().await?))
}

/// GET /api/v2/receipts/search
pub async fn search_receipts(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<torgashka_domain::ReceiptSearchDto>, PosErr> {
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

/// GET /api/v1/receipts/search — 1:1 Python v1 (total з дублікатами JOIN).
pub async fn search_receipts_v1(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<torgashka_domain::ReceiptV1SearchDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    let query = torgashka_domain::ReceiptSearchQuery {
        q: q.q.unwrap_or_default(),
        date_from: q.date_from.as_deref().and_then(parse_dt),
        date_to: q.date_to.as_deref().and_then(parse_dt),
        receipt_type: q.receipt_type,
        page: q.page.unwrap_or(1),
        size: q.size.unwrap_or(20),
    };
    Ok(Json(svc.search_receipts_v1(&query).await?))
}

/// GET /api/v2/receipts/by-product/{query}/recent-sales
pub async fn recent_sales(
    State(state): State<AppState>,
    Path(query): Path<String>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Vec<torgashka_domain::ProductRecentSalesDto>>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    let limit = q.limit.unwrap_or(5).clamp(1, 20);
    Ok(Json(svc.recent_sales_by_product(&query, limit).await?))
}

/// GET /api/v2/receipts/products/{product_id}/returnable-quantity
pub async fn returnable_quantity(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<torgashka_domain::ReturnableQtyDto>, PosErr> {
    let id = path_uuid(product_id, "product_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.returnable_quantity(id).await?))
}

/// GET /api/v2/receipts/{receipt_id}/items
pub async fn receipt_items(
    State(state): State<AppState>,
    Path(receipt_id): Path<String>,
) -> Result<Json<Vec<torgashka_domain::ReceiptItemDetailDto>>, PosErr> {
    let id = path_uuid(receipt_id, "receipt_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.receipt_items(id).await?))
}

/// GET /api/v2/receipts/{receipt_id}
pub async fn get_receipt(
    State(state): State<AppState>,
    Path(receipt_id): Path<String>,
) -> Result<Json<torgashka_domain::ReceiptDto>, PosErr> {
    let id = path_uuid(receipt_id, "receipt_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.get_receipt(id).await?))
}

// ─── Чеки v1: LIST/GET/items (1:1 Python deprecated v1) ─────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ListV1Query {
    pub cashier_id: Option<String>,
    pub receipt_type: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub page: Option<i64>,
    pub size: Option<i64>,
    pub payment_method: Option<String>,
}

/// Python `_normalize_date_to`: дата з часом 00:00:00 → 23:59:59.999999.
fn normalize_date_to(dt: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
    use chrono::Timelike;
    if dt.hour() == 0 && dt.minute() == 0 && dt.second() == 0 && dt.nanosecond() == 0 {
        dt.date()
            .and_hms_nano_opt(23, 59, 59, 999_999_000)
            .unwrap_or(dt)
    } else {
        dt
    }
}

/// GET /api/v1/receipts
pub async fn list_receipts_v1(
    State(state): State<AppState>,
    Query(q): Query<ListV1Query>,
) -> Result<Json<torgashka_domain::ReceiptV1ListDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    let query = torgashka_domain::ReceiptV1ListQuery {
        cashier_id: q
            .cashier_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok()),
        receipt_type: q.receipt_type,
        date_from: q.date_from.as_deref().and_then(parse_dt),
        date_to: q
            .date_to
            .as_deref()
            .and_then(parse_dt)
            .map(normalize_date_to),
        page: q.page.unwrap_or(1).max(1),
        size: q.size.unwrap_or(20).clamp(1, 100),
        payment_method: q.payment_method,
    };
    Ok(Json(svc.list_receipts_v1(&query).await?))
}

/// GET /api/v1/receipts/{receipt_id}
pub async fn get_receipt_v1(
    State(state): State<AppState>,
    Path(receipt_id): Path<String>,
) -> Result<Json<torgashka_domain::ReceiptV1Dto>, PosErr> {
    let id = path_uuid(receipt_id, "receipt_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.get_receipt_v1(id).await?))
}

/// GET /api/v1/receipts/{receipt_id}/items
pub async fn receipt_items_v1(
    State(state): State<AppState>,
    Path(receipt_id): Path<String>,
) -> Result<Json<Vec<torgashka_domain::ReceiptV1ItemDto>>, PosErr> {
    let id = path_uuid(receipt_id, "receipt_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.receipt_items_v1(id).await?))
}

/// GET /api/v1/receipts/by-product/{query}/recent-sales — 1:1 Python v1.
/// Python v1: ProductRecentSalesListResponse.model_dump() → Decimal конвертується
/// у float (числа) — формат == v2 Rust; обгортка {items, total} + 404 якщо пусто.
pub async fn recent_sales_v1(
    State(state): State<AppState>,
    Path(query): Path<String>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<torgashka_domain::ReceiptV1RecentSalesListDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    let limit = q.limit.unwrap_or(5).clamp(1, 20);
    let items = svc.recent_sales_by_product(&query, limit).await?;
    if items.is_empty() {
        return Err(torgashka_domain::PosError::NotFound(format!(
            "Товарів за запитом '{query}' не знайдено. Спробуйте ввести штрих-код або назву товару"
        ))
        .into());
    }
    let total = items.len() as i64;
    Ok(Json(torgashka_domain::ReceiptV1RecentSalesListDto {
        items,
        total,
    }))
}

// ─── Робочі сесії ──────────────────────────────────────────────────────────

/// GET /api/v1/work-sessions/my
pub async fn my_sessions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<MonthQuery>,
) -> Result<Json<torgashka_domain::MySessionsDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::WorkReportDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::UserSessionsDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::WriteOffListDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::WriteOffDto>, PosErr> {
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
) -> Result<(StatusCode, Json<torgashka_domain::WriteOffDto>), PosErr> {
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
) -> Result<Json<torgashka_domain::WriteOffDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::WriteOffDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "write_off_id")?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.confirm_write_off(id).await?))
}

// ─── Довідник причин списання ───────────────────────────────────────────────

/// GET /api/v1/write-off-reasons — доступно авторизованим (без require_admin).
pub async fn list_write_off_reasons(
    State(state): State<AppState>,
) -> Result<Json<WriteOffReasonsListDto>, PosErr> {
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.list_write_off_reasons().await?))
}

/// POST /api/v1/write-off-reasons → 201 (require_admin; 409 на дублікат).
pub async fn create_write_off_reason(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<WriteOffReasonItem>), PosErr> {
    require_admin(&state, &claims).await?;
    let name = parse_write_off_reason_name(&body)?;
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_write_off_reason(&name).await?),
    ))
}

/// Валідація назви причини списання (1:1 Python: Pydantic min/max + strip).
fn parse_write_off_reason_name(v: &Value) -> Result<String, PosErr> {
    let raw = v
        .get("name")
        .and_then(|r| r.as_str())
        .ok_or_else(|| v422("missing", &["body", "name"], "Field required", ""))?
        .to_string();
    // Pydantic min_length=2 — на сирому рядку (до strip). chars() = символи,
    // не байти (кирилиця: 1 символ = 2 байти в UTF-8).
    if raw.chars().count() < 2 {
        return Err(v422(
            "string_too_short",
            &["body", "name"],
            "String should have at least 2 characters",
            &raw,
        ));
    }
    if raw.chars().count() > 100 {
        return Err(v422(
            "string_too_long",
            &["body", "name"],
            "String should have at most 100 characters",
            &raw,
        ));
    }
    let name = raw.trim();
    // Після strip може лишитись < 2 символів — Python HTTPException 422.
    if name.chars().count() < 2 {
        return Err(PosErr::Service(PosError::Validation(
            "Назва причини має містити щонайменше 2 символи".to_string(),
        )));
    }
    Ok(name.to_string())
}

// ─── Переміщення ────────────────────────────────────────────────────────────

/// GET /api/v1/transfers
pub async fn list_transfers(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<torgashka_domain::TransferListDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::TransferDto>, PosErr> {
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
) -> Result<(StatusCode, Json<torgashka_domain::TransferDto>), PosErr> {
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
) -> Result<Json<torgashka_domain::TransferDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::TransferDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::ShiftListDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::PrroShiftDto>, PosErr> {
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
) -> Result<Json<torgashka_domain::PrroShiftDto>, PosErr> {
    require_admin(&state, &claims).await?;
    let comment = body
        .get("comment")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    let repo = pos_repo(&state)?;
    let svc = PosServiceFacade::new(repo);
    Ok(Json(svc.close_shift(comment).await?))
}

// ─── Тести валідації ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn name_err(body: Value) -> PosErr {
        match parse_write_off_reason_name(&body) {
            Ok(_) => panic!("очікувалась помилка валідації для {body}"),
            Err(e) => e,
        }
    }

    #[test]
    fn reason_name_ok() {
        assert_eq!(
            parse_write_off_reason_name(&json!({"name": "Прострочений термін"})).unwrap(),
            "Прострочений термін"
        );
        // strip з обох боків
        assert_eq!(
            parse_write_off_reason_name(&json!({"name": "  Брак  "})).unwrap(),
            "Брак"
        );
    }

    #[test]
    fn reason_name_missing_or_not_string() {
        assert!(matches!(name_err(json!({})), PosErr::Validation(_)));
        assert!(matches!(
            name_err(json!({"name": 123})),
            PosErr::Validation(_)
        ));
    }

    #[test]
    fn reason_name_too_short() {
        // Сирий рядок < 2 символів → Pydantic string_too_short (422).
        assert!(matches!(
            name_err(json!({"name": "a"})),
            PosErr::Validation(_)
        ));
        // Після strip лишилось < 2 → Python HTTPException 422 (detail рядком).
        assert!(matches!(
            name_err(json!({"name": "  a  "})),
            PosErr::Service(PosError::Validation(_))
        ));
        assert!(matches!(
            name_err(json!({"name": "   "})),
            PosErr::Service(PosError::Validation(_))
        ));
    }

    #[test]
    fn reason_name_too_long() {
        let long = "д".repeat(101);
        assert!(matches!(
            name_err(json!({"name": long})),
            PosErr::Validation(_)
        ));
        // Рівно 100 символів — проходить (chars, не байти).
        let ok = "д".repeat(100);
        assert_eq!(
            parse_write_off_reason_name(&json!({"name": ok}))
                .unwrap()
                .chars()
                .count(),
            100
        );
        // 100 символів "д" = 200 байтів — байтовий len() не має відхиляти.
        assert_eq!(ok.len(), 200);
    }
}
