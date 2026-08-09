// ─────────────────────────────────────────────────────────────────────────────
// ledger — роути /api/v1/ledger + /api/v2/ledger (етап 4, Strangler Fig)
// ─────────────────────────────────────────────────────────────────────────────
// 1:1 з Python-еталоном:
//   v1 (app/api/v1/ledger.py, LedgerService):
//     GET  /api/v1/ledger/balance/{supplier_id}  → {supplier_id, supplier_name,
//          current_balance, last_updated}   (⚠️ статичний сегмент ПЕРЕД {supplier_id})
//     GET  /api/v1/ledger/{supplier_id}?page&size → {items,total,page,size}
//     POST /api/v1/ledger (require_admin) → 201
//   v2 (app/api/v2/ledger.py, LedgerUseCases):
//     GET  /api/v2/ledger/entries?page&size&supplier_id&operation_type&date_from&date_to
//     POST /api/v2/ledger/entries → 201
//     GET  /api/v2/ledger/balance/{supplier_id}
//     GET  /api/v2/ledger/balances
//
// ВАЖЛИВО: v2 Python має баги (POST /entries → 500 UnmappedInstanceError;
// GET /entries → 500 ResponseValidationError при notes=NULL) — тут реалізовано
// ЗАДУМАНУ робочу поведінку (фронтенд ledgerService використовує v2).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::NaiveDateTime;
use serde_json::Value;
use uuid::Uuid;

use torgashka_application::services::ledger::LedgerServiceFacade;
use torgashka_domain::{LedgerEntriesQuery, LedgerEntryInput, LedgerError, LedgerService as LedgerPort};

use crate::auth::Claims;
use crate::AppState;

/// Обгортка помилок ledger-шару: 1:1 з HTTP Python.
#[derive(Debug)]
pub enum LedgerErr {
    Service(LedgerError),
    /// 422 Pydantic-валідація.
    Validation(serde_json::Value),
    /// 403 — роль не адміністратор.
    Forbidden(String),
    /// 401 — користувача не знайдено / токен недійсний.
    Unauthorized(String),
}

impl From<LedgerError> for LedgerErr {
    fn from(e: LedgerError) -> Self {
        LedgerErr::Service(e)
    }
}

fn v422(vtype: &str, loc: &[&str], msg: &str, input: &str) -> LedgerErr {
    v422v(
        vtype,
        loc,
        msg,
        serde_json::Value::String(input.to_string()),
        None,
    )
}

/// 422 з JSON-input (Pydantic v2: missing → input = весь body) та ctx.
fn v422v(
    vtype: &str,
    loc: &[&str],
    msg: &str,
    input: serde_json::Value,
    ctx: Option<serde_json::Value>,
) -> LedgerErr {
    let mut item = serde_json::json!({
        "type": vtype,
        "loc": loc,
        "msg": msg,
        "input": input,
    });
    if let Some(c) = ctx {
        item["ctx"] = c;
    }
    LedgerErr::Validation(serde_json::json!({ "detail": [item] }))
}

impl IntoResponse for LedgerErr {
    fn into_response(self) -> Response {
        match self {
            LedgerErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            LedgerErr::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            LedgerErr::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            LedgerErr::Service(e) => match e {
                LedgerError::NotFound(msg) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                LedgerError::BadRequest(msg) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                LedgerError::InvalidOperationType(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "detail": "Внутрішня помилка сервера",
                        "type": "ValueError",
                    })),
                )
                    .into_response(),
                LedgerError::Infrastructure(msg) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": format!("Помилка БД: {msg}")})),
                )
                    .into_response(),
            },
        }
    }
}

/// Доступ до ledger-репозиторію.
fn ledger_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn LedgerPort + Send + Sync>, LedgerErr> {
    state
        .ledger
        .clone()
        .ok_or_else(|| LedgerErr::Forbidden("Rust-гілка ledger вимкнена".to_string()))
}

/// require_admin (Python AuthService.require_admin → 403).
async fn require_admin(state: &AppState, claims: &Claims) -> Result<(), LedgerErr> {
    let pool = state
        .write_pool
        .clone()
        .ok_or_else(|| LedgerErr::Forbidden("Rust-гілка ledger вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        LedgerErr::Unauthorized("Недійсний токен: відсутній ідентифікатор користувача".to_string())
    })?;
    torgashka_infrastructure::repositories::write::require_admin_role(&pool, user_id)
        .await
        .map_err(|e| LedgerErr::Service(LedgerError::Infrastructure(e.to_string())))
}

fn sub_uuid(claims: &Claims) -> Result<Uuid, LedgerErr> {
    Uuid::parse_str(&claims.sub).map_err(|_| {
        LedgerErr::Unauthorized("Недійсний токен: відсутній ідентифікатор користувача".to_string())
    })
}

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, LedgerErr> {
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

/// Decimal v1 (Pydantic Decimal): рядок або число → рядок зі scale вводу.
/// Валідація max_digits=12, decimal_places=2 (як Pydantic).
fn decimal_str(v: &Value, key: &str, required: bool) -> Result<Option<String>, LedgerErr> {
    let Some(f) = v.get(key) else {
        if required {
            return Err(v422v(
                "missing",
                &["body", key],
                "Field required",
                v.clone(),
                None,
            ));
        }
        return Ok(None);
    };
    if f.is_null() {
        return Ok(None);
    }
    let raw = if let Some(s) = f.as_str() {
        s.to_string()
    } else if let Some(n) = f.as_f64() {
        // Python: Decimal(str(100.5)) = "100.5"; Decimal(str(100.0)) = "100.0".
        if n.fract() == 0.0 {
            format!("{n:.1}")
        } else {
            format!("{n}")
        }
    } else {
        return Err(v422(
            "decimal_parsing",
            &["body", key],
            "Input should be a valid decimal",
            &f.to_string(),
        ));
    };
    // decimal_places ≤ 2
    if let Some(dot) = raw.find('.') {
        let frac = raw.len() - dot - 1;
        if frac > 2 {
            return Err(v422v(
                "decimal_max_places",
                &["body", key],
                "Decimal input should have no more than 2 decimal places",
                serde_json::Value::String(raw.clone()),
                Some(serde_json::json!({"decimal_places": 2})),
            ));
        }
    }
    // max_digits ≤ 12 (цілі знаки + дробові)
    let digits: usize = raw.chars().filter(|c| c.is_ascii_digit()).count();
    if digits > 12 {
        return Err(v422(
            "decimal_max_digits",
            &["body", key],
            "Decimal input should have no more than 12 digits in total",
            &raw,
        ));
    }
    Ok(Some(raw))
}

fn field_uuid(v: &Value, key: &str, required: bool) -> Result<Option<Uuid>, LedgerErr> {
    let Some(f) = v.get(key) else {
        if required {
            return Err(v422v(
                "missing",
                &["body", key],
                "Field required",
                v.clone(),
                None,
            ));
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

fn field_str(v: &Value, key: &str) -> Option<Option<String>> {
    let f = v.get(key)?;
    if f.is_null() {
        Some(None)
    } else {
        Some(Some(f.as_str().unwrap_or("").to_string()))
    }
}

/// operation_type v1: Pydantic enum LedgerOperationType (4 значення).
fn parse_op_type_v1(v: &Value, key: &str) -> Result<String, LedgerErr> {
    let s = field_str(v, key)
        .flatten()
        .ok_or_else(|| v422v("missing", &["body", key], "Field required", v.clone(), None))?;
    match s.as_str() {
        "invoice" | "payment" | "return" | "correction" => Ok(s),
        other => Err(v422v(
            "enum",
            &["body", key],
            "Input should be 'invoice', 'payment', 'return' or 'correction'",
            serde_json::Value::String(other.to_string()),
            Some(serde_json::json!({
                "expected": "'invoice', 'payment', 'return' or 'correction'"
            })),
        )),
    }
}

/// Парсинг тіла POST (v1 + v2).
fn parse_create(v: &Value, v1: bool) -> Result<LedgerEntryInput, LedgerErr> {
    let supplier_id = field_uuid(v, "supplier_id", true)?.ok_or_else(|| {
        v422v(
            "missing",
            &["body", "supplier_id"],
            "Field required",
            v.clone(),
            None,
        )
    })?;
    let amount = decimal_str(v, "amount", true)?.ok_or_else(|| {
        v422v(
            "missing",
            &["body", "amount"],
            "Field required",
            v.clone(),
            None,
        )
    })?;
    let operation_type = if v1 {
        parse_op_type_v1(v, "operation_type")?
    } else {
        field_str(v, "operation_type")
            .flatten()
            .unwrap_or_else(|| "invoice".to_string())
    };
    let document_id = field_uuid(v, "document_id", false)?;
    let document_number = field_str(v, "document_number").flatten();
    let notes = field_str(v, "notes").flatten();
    let operation_date = if v1 {
        let raw = v
            .get("operation_date")
            .and_then(|d| d.as_str())
            .ok_or_else(|| {
                v422v(
                    "missing",
                    &["body", "operation_date"],
                    "Field required",
                    v.clone(),
                    None,
                )
            })?;
        let dt = parse_dt(raw).ok_or_else(|| {
            v422(
                "datetime_parsing",
                &["body", "operation_date"],
                "Input should be a valid datetime",
                raw,
            )
        })?;
        Some(dt)
    } else {
        None
    };
    Ok(LedgerEntryInput {
        supplier_id,
        amount,
        operation_type,
        document_id,
        document_number,
        operation_date,
        notes,
    })
}

// ─── v1 ─────────────────────────────────────────────────────────────────────

/// POST /api/v1/ledger → 201 (require_admin).
pub async fn create_entry_v1(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<torgashka_domain::LedgerEntryV1Dto>), LedgerErr> {
    require_admin(&state, &claims).await?;
    let _ = sub_uuid(&claims)?;
    let input = parse_create(&body, true)?;
    let repo = ledger_repo(&state)?;
    let svc = LedgerServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_entry_v1(&input).await?),
    ))
}

/// GET /api/v1/ledger/balance/{supplier_id}
pub async fn balance_v1(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<torgashka_domain::LedgerBalanceV1Dto>, LedgerErr> {
    let id = path_uuid(id, "supplier_id")?;
    let repo = ledger_repo(&state)?;
    let svc = LedgerServiceFacade::new(repo);
    Ok(Json(svc.balance_v1(id).await?))
}

/// GET /api/v1/ledger/{supplier_id}?page&size
pub async fn history_v1(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<torgashka_domain::LedgerHistoryV1Dto>, LedgerErr> {
    let id = path_uuid(id, "supplier_id")?;
    let page = q.page.unwrap_or(1);
    let size = q.size.unwrap_or(20).clamp(1, 100);
    let repo = ledger_repo(&state)?;
    let svc = LedgerServiceFacade::new(repo);
    Ok(Json(svc.history_v1(id, page, size).await?))
}

// ─── v2 ─────────────────────────────────────────────────────────────────────

/// GET /api/v2/ledger/entries
pub async fn list_entries_v2(
    State(state): State<AppState>,
    Query(q): Query<EntriesQuery>,
) -> Result<Json<torgashka_domain::LedgerListV2Dto>, LedgerErr> {
    let query = LedgerEntriesQuery {
        page: q.page.unwrap_or(1),
        size: q.size.unwrap_or(20).clamp(1, 100),
        supplier_id: q.supplier_id,
        operation_type: q.operation_type,
        date_from: q.date_from.as_deref().and_then(parse_dt),
        date_to: q.date_to.as_deref().and_then(parse_dt),
    };
    let repo = ledger_repo(&state)?;
    let svc = LedgerServiceFacade::new(repo);
    Ok(Json(svc.list_entries_v2(&query).await?))
}

/// POST /api/v2/ledger/entries → 201 (без require_admin — як Python).
pub async fn create_entry_v2(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<torgashka_domain::LedgerEntryV2Dto>), LedgerErr> {
    let input = parse_create(&body, false)?;
    let repo = ledger_repo(&state)?;
    let svc = LedgerServiceFacade::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_entry_v2(&input).await?),
    ))
}

/// GET /api/v2/ledger/balance/{supplier_id}
pub async fn balance_v2(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<torgashka_domain::LedgerBalanceV2Dto>, LedgerErr> {
    let id = path_uuid(id, "supplier_id")?;
    let repo = ledger_repo(&state)?;
    let svc = LedgerServiceFacade::new(repo);
    Ok(Json(svc.balance_v2(id).await?))
}

/// GET /api/v2/ledger/balances
pub async fn all_balances_v2(
    State(state): State<AppState>,
) -> Result<Json<Vec<torgashka_domain::SupplierBalanceV2Dto>>, LedgerErr> {
    let repo = ledger_repo(&state)?;
    let svc = LedgerServiceFacade::new(repo);
    Ok(Json(svc.all_balances_v2().await?))
}

// ─── Query-структури ────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
pub struct EntriesQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
    pub supplier_id: Option<Uuid>,
    pub operation_type: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}
