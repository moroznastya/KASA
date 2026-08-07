// ─────────────────────────────────────────────────────────────────────────────
// debtors — Rust-гілка боржників (етап 8, група 1)
// ─────────────────────────────────────────────────────────────────────────────
// 1:1 з Python v1/debtors.py:
//   GET  /api/v1/debtors/search?query=&limit=   — пошук за ім'ям (ilike)
//   GET  /api/v1/debtors?page=&size=            — список з пагінацією
//   POST /api/v1/debtors                        — створення (201)
//   GET  /api/v1/debtors/{id}                   — деталі
//   PUT  /api/v1/debtors/{id}                   — оновлення
//   POST /api/v1/debtors/{id}/pay               — погашення боргу
//   GET  /api/v1/debtors/{id}/receipts          — чеки боржника
//   GET  /api/v1/debtors/{id}/payments          — історія оплат
// Авторизація: JWT на весь роутер (get_current_user — будь-яка роль).
// Монтуються лише під KASA_RUST_DEBTORS=1; інакше — fallback на Python :8001.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use kasa_application::DebtorServiceFacade;
use kasa_domain::{
    DebtorCreateInput, DebtorError, DebtorPayInput, DebtorSearchQuery, DebtorService,
    DebtorUpdateInput,
};

use crate::AppState;

/// Помилки хендлерів боржників → HTTP (1:1 з Python).
#[derive(Debug)]
pub enum DebtErr {
    Service(DebtorError),
    /// 422 Pydantic-валідація.
    Validation(Value),
    /// 403 — Rust-гілка вимкнена (не має статись: роут монтується лише під флагом).
    Forbidden(String),
}

impl From<DebtorError> for DebtErr {
    fn from(e: DebtorError) -> Self {
        DebtErr::Service(e)
    }
}

fn v422(vtype: &str, loc: &[&str], msg: &str, input: &str) -> DebtErr {
    DebtErr::Validation(serde_json::json!({
        "detail": [{
            "type": vtype,
            "loc": loc,
            "msg": msg,
            "input": input,
        }]
    }))
}

impl IntoResponse for DebtErr {
    fn into_response(self) -> Response {
        match self {
            DebtErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            DebtErr::Service(DebtorError::NotFound(id)) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"detail": format!("Боржника з ID '{id}' не знайдено")})),
            )
                .into_response(),
            DebtErr::Service(DebtorError::BadRequest(msg)) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            DebtErr::Service(DebtorError::Infrastructure(msg)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            DebtErr::Service(DebtorError::Validation(detail)) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            DebtErr::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
        }
    }
}

fn debtor_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn DebtorService + Send + Sync>, DebtErr> {
    state
        .debtors
        .clone()
        .ok_or_else(|| DebtErr::Forbidden("Rust-гілка боржників вимкнена".to_string()))
}

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, DebtErr> {
    Uuid::parse_str(&raw).map_err(|_| {
        v422(
            "uuid_parsing",
            &["path", field],
            "Input should be a valid UUID",
            &raw,
        )
    })
}

// ─── Query-параметри ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub query: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
}

// ─── Хендлери ───────────────────────────────────────────────────────────────

/// GET /api/v1/debtors/search?query=&limit=
pub async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<kasa_domain::DebtorDto>>, DebtErr> {
    let query = q.query.unwrap_or_default();
    if query.is_empty() {
        return Err(v422(
            "string_too_short",
            &["query", "query"],
            "String should have at least 1 character",
            &query,
        ));
    }
    let limit = q.limit.unwrap_or(10);
    if !(1..=50).contains(&limit) {
        return Err(v422(
            "greater_than_equal",
            &["query", "limit"],
            "Input should be greater than or equal to 1",
            &limit.to_string(),
        ));
    }
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok(Json(svc.search(&DebtorSearchQuery { query, limit }).await?))
}

/// GET /api/v1/debtors?page=&size=
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<PageQuery>,
) -> Result<Json<kasa_domain::DebtorListDto>, DebtErr> {
    let page = q.page.unwrap_or(1);
    if page < 1 {
        return Err(v422(
            "greater_than_equal",
            &["query", "page"],
            "Input should be greater than or equal to 1",
            &page.to_string(),
        ));
    }
    let size = q.size.unwrap_or(50);
    if !(1..=1000).contains(&size) {
        return Err(v422(
            "less_than_equal",
            &["query", "size"],
            "Input should be less than or equal to 1000",
            &size.to_string(),
        ));
    }
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok(Json(svc.list(page, size).await?))
}

/// POST /api/v1/debtors → 201
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::DebtorDto>), DebtErr> {
    let input = parse_create(&body)?;
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok((StatusCode::CREATED, Json(svc.create(&input).await?)))
}

/// GET /api/v1/debtors/{id}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::DebtorDto>, DebtErr> {
    let id = path_uuid(id, "debtor_id")?;
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok(Json(svc.get(id).await?))
}

/// PUT /api/v1/debtors/{id}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::DebtorDto>, DebtErr> {
    let id = path_uuid(id, "debtor_id")?;
    let input = parse_update(&body)?;
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok(Json(svc.update(id, &input).await?))
}

/// POST /api/v1/debtors/{id}/pay
pub async fn pay(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::DebtorDto>, DebtErr> {
    let id = path_uuid(id, "debtor_id")?;
    let input = parse_pay(&body)?;
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok(Json(svc.pay(id, &input).await?))
}

/// GET /api/v1/debtors/{id}/receipts
pub async fn receipts(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<kasa_domain::DebtorReceiptDto>>, DebtErr> {
    let id = path_uuid(id, "debtor_id")?;
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok(Json(svc.receipts(id).await?))
}

/// GET /api/v1/debtors/{id}/payments
pub async fn payments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<kasa_domain::DebtorPaymentDto>>, DebtErr> {
    let id = path_uuid(id, "debtor_id")?;
    let repo = debtor_repo(&state)?;
    let svc = DebtorServiceFacade::new(repo);
    Ok(Json(svc.payments(id).await?))
}

// ─── Парсери тіла (Pydantic-валідація 1:1) ─────────────────────────────────

fn parse_create(body: &Value) -> Result<DebtorCreateInput, DebtErr> {
    let obj = body.as_object().ok_or_else(|| {
        v422(
            "dict_type",
            &["body"],
            "Input should be a valid dictionary",
            "",
        )
    })?;
    // name — обов'язкове.
    let name = match obj.get("name") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => {
            return Err(v422("missing", &["body", "name"], "Field required", ""))
        }
        Some(_) => {
            return Err(v422(
                "string_type",
                &["body", "name"],
                "Input should be a valid string",
                "",
            ))
        }
    };
    if name.is_empty() {
        return Err(v422(
            "string_too_short",
            &["body", "name"],
            "String should have at least 1 character",
            &name,
        ));
    }
    if name.chars().count() > 255 {
        return Err(v422(
            "string_too_long",
            &["body", "name"],
            "String should have at most 255 characters",
            &name,
        ));
    }
    let phone = opt_str(obj, "phone", 50)?;
    let notes = opt_str_any(obj, "notes")?;
    Ok(DebtorCreateInput { name, phone, notes })
}

fn parse_update(body: &Value) -> Result<DebtorUpdateInput, DebtErr> {
    let obj = body.as_object().ok_or_else(|| {
        v422(
            "dict_type",
            &["body"],
            "Input should be a valid dictionary",
            "",
        )
    })?;
    let name = match obj.get("name") {
        Some(Value::String(s)) => {
            if s.is_empty() {
                return Err(v422(
                    "string_too_short",
                    &["body", "name"],
                    "String should have at least 1 character",
                    s,
                ));
            }
            if s.chars().count() > 255 {
                return Err(v422(
                    "string_too_long",
                    &["body", "name"],
                    "String should have at most 255 characters",
                    s,
                ));
            }
            Some(s.clone())
        }
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(v422(
                "string_type",
                &["body", "name"],
                "Input should be a valid string",
                "",
            ))
        }
    };
    let phone = opt_str(obj, "phone", 50)?;
    let notes = opt_str_any(obj, "notes")?;
    Ok(DebtorUpdateInput { name, phone, notes })
}

fn parse_pay(body: &Value) -> Result<DebtorPayInput, DebtErr> {
    let obj = body.as_object().ok_or_else(|| {
        v422(
            "dict_type",
            &["body"],
            "Input should be a valid dictionary",
            "",
        )
    })?;
    let amount = match obj.get("amount") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Null) | None => {
            return Err(v422("missing", &["body", "amount"], "Field required", ""))
        }
        Some(_) => {
            return Err(v422(
                "decimal_parsing",
                &["body", "amount"],
                "Input should be a valid decimal",
                "",
            ))
        }
    };
    let payment_method = match obj.get("payment_method") {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    // Pydantic Decimal-валідація 1:1 (Python: Field(gt=0, decimal_places=2)).
    validate_decimal_amount(&amount)?;
    Ok(DebtorPayInput {
        amount,
        payment_method,
    })
}

/// Pydantic Decimal-валідація суми оплати (1:1 Python schemas.debtor.DebtorPayRequest).
fn validate_decimal_amount(amount: &str) -> Result<(), DebtErr> {
    let s = amount.trim();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };
    let digits_ok = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit());
    if !digits_ok(int_part) || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return Err(v422(
            "decimal_parsing",
            &["body", "amount"],
            "Input should be a valid decimal",
            amount,
        ));
    }
    if frac_part.len() > 2 {
        return Err(DebtErr::Validation(serde_json::json!({
            "detail": [{
                "type": "decimal_max_places",
                "loc": ["body", "amount"],
                "msg": "Decimal input should have no more than 2 decimal places",
                "input": amount,
                "ctx": {"decimal_places": 2},
            }]
        })));
    }
    // gt=0: "0.00"/"-1.00" → 422 (Python Pydantic блокує до ручної перевірки).
    let cents = kasa_domain::parse_scaled2(amount).unwrap_or(0);
    if neg || cents <= 0 {
        return Err(DebtErr::Validation(serde_json::json!({
            "detail": [{
                "type": "greater_than",
                "loc": ["body", "amount"],
                "msg": "Input should be greater than 0",
                "input": amount,
                "ctx": {"gt": 0},
            }]
        })));
    }
    Ok(())
}

/// Опційний рядок з обмеженням довжини (None → null/відсутній).
fn opt_str(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<String>, DebtErr> {
    match obj.get(key) {
        Some(Value::String(s)) => {
            if s.chars().count() > max {
                return Err(v422(
                    "string_too_long",
                    &["body", key],
                    &format!("String should have at most {max} characters"),
                    s,
                ));
            }
            Ok(Some(s.clone()))
        }
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(v422(
            "string_type",
            &["body", key],
            "Input should be a valid string",
            "",
        )),
    }
}

/// Опційний рядок без обмеження довжини.
fn opt_str_any(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Option<String>, DebtErr> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(v422(
            "string_type",
            &["body", key],
            "Input should be a valid string",
            "",
        )),
    }
}
