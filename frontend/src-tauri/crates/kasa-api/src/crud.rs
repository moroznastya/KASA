// ─────────────────────────────────────────────────────────────────────────────
// crud — CRUD-хендлери довідників та інвентаризації (етап 2)
// ─────────────────────────────────────────────────────────────────────────────
// Відтворюють Python-еталон 1:1:
//   - статус-коди: 201 (create), 200 (update/get), 204 (delete),
//     404 (не знайдено), 409 (конфлікт barcode/sku), 400 (бізнес-правило),
//     403 (роль), 422 (валідація Pydantic)
//   - формат помилок: {"detail": "..."} (FastAPI) або {"detail":[...]}
//     для 422 (Pydantic ValidationError)
//   - авторизація: POST/PUT/DELETE — require_admin (як Python), GET — JWT
//
// Монтуються лише під feature-flag KASA_RUST_READDIRS=1; інакше всі ці
// шляхи йдуть у fallback → проксі на Python :8001 (режим відкату).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use kasa_application::{ReadDirectoryService, ServiceError, WriteService};
use kasa_domain::{
    CategoryCreateInput, CategoryUpdateInput, InventoryCreateInput, InventoryItemInput,
    InventoryUpdateInput, ProductCreateInput, ProductUpdateInput, SupplierCreateInput,
    SupplierUpdateInput,
};

use crate::auth::Claims;
use crate::AppState;

// ─── Помилки ────────────────────────────────────────────────────────────────

/// Помилки CRUD-хендлерів → HTTP (1:1 з Python-еталоном).
#[derive(Debug)]
pub enum CrudError {
    Service(ServiceError),
    /// 422 Pydantic-валідація (uuid_parsing / string_too_long / decimal ...).
    Validation(serde_json::Value),
    /// 403 — роль не адміністратор.
    Forbidden(String),
    /// 401 — користувача не знайдено в БД (як Python get_current_user).
    Unauthorized(String),
}

impl From<ServiceError> for CrudError {
    fn from(e: ServiceError) -> Self {
        CrudError::Service(e)
    }
}

/// Формує Pydantic-style 422 detail для одного поля.
fn v422(vtype: &str, loc: &[&str], msg: &str, input: &str) -> CrudError {
    CrudError::Validation(serde_json::json!({
        "detail": [{
            "type": vtype,
            "loc": loc,
            "msg": msg,
            "input": input,
        }]
    }))
}

impl IntoResponse for CrudError {
    fn into_response(self) -> Response {
        match self {
            CrudError::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            CrudError::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            CrudError::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            CrudError::Service(e) => match e {
                ServiceError::Write(kasa_domain::WriteError::NotFound(msg)) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                ServiceError::Write(kasa_domain::WriteError::Conflict(msg)) => (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                ServiceError::Write(kasa_domain::WriteError::BadRequest(msg)) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                ServiceError::Write(kasa_domain::WriteError::Forbidden(msg)) => (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                ServiceError::Write(kasa_domain::WriteError::Infrastructure(msg)) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": format!("Помилка БД: {msg}")})),
                )
                    .into_response(),
                ServiceError::Directory(kasa_domain::DirectoryError::NotFound(msg)) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"detail": msg})),
                )
                    .into_response(),
                ServiceError::Directory(kasa_domain::DirectoryError::InvalidUuid {
                    field,
                    value,
                }) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"detail": format!(
                        "Поле {field}: очікується UUID, отримано: {value:?}"
                    )})),
                )
                    .into_response(),
                ServiceError::Directory(kasa_domain::DirectoryError::Infrastructure(msg)) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": format!("Помилка БД довідників: {msg}")})),
                )
                    .into_response(),
            },
        }
    }
}

// ─── Доступ до репозиторіїв ────────────────────────────────────────────────

/// Rust-репозиторій читання (GET-хендлери етапу 2).
fn read_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn kasa_domain::ReadDirectories + Send + Sync>, CrudError> {
    state
        .readdirs
        .clone()
        .ok_or_else(|| CrudError::Forbidden("Rust-гілка довідників вимкнена".to_string()))
}

/// Rust-репозиторій запису (POST/PUT/DELETE етапу 2).
fn write_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn kasa_domain::WriteDirectories + Send + Sync>, CrudError> {
    state
        .write
        .clone()
        .ok_or_else(|| CrudError::Forbidden("Rust-гілка довідників вимкнена".to_string()))
}

/// require_admin: перевіряє роль користувача в БД (як Python
/// `AuthService.require_admin` → `user.role != ADMIN` → 403).
async fn require_admin(state: &AppState, claims: &Claims) -> Result<(), CrudError> {
    let pool = state
        .write_pool
        .clone()
        .ok_or_else(|| CrudError::Forbidden("Rust-гілка довідників вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        CrudError::Unauthorized("Недійсний токен: відсутній ідентифікатор користувача".to_string())
    })?;
    kasa_infrastructure::repositories::write::require_admin_role(&pool, user_id)
        .await
        .map_err(|e| CrudError::Service(ServiceError::Write(e)))
}

// ─── Валідація UUID у path (422 uuid_parsing, як FastAPI) ──────────────────

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, CrudError> {
    Uuid::parse_str(&raw).map_err(|_| {
        v422(
            "uuid_parsing",
            &["path", field],
            "Input should be a valid UUID",
            &raw,
        )
    })
}

// ─── Валідація Decimal-полів (Pydantic max_digits/decimal_places) ──────────

/// Перевіряє десятковий рядок: `^-?\d+(\.\d+)?$`, ціла частина ≤
/// max_digits - decimal_places, дробова ≤ decimal_places. Повертає рядок.
fn validate_decimal(
    s: &str,
    max_digits: usize,
    decimal_places: usize,
    field: &str,
) -> Result<String, CrudError> {
    let t = s.trim();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };
    if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
        return Err(v422(
            "decimal_parsing",
            &["body", field],
            "Input should be a valid decimal",
            t,
        ));
    }
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return Err(v422(
            "decimal_parsing",
            &["body", field],
            "Input should be a valid decimal",
            t,
        ));
    }
    let int_len = int_part.trim_start_matches('0').len().max(1);
    if int_len > max_digits - decimal_places {
        return Err(v422(
            "decimal_max_digits",
            &["body", field],
            &format!("Decimal input should have no more than {max_digits} digits in total"),
            t,
        ));
    }
    if frac_part.len() > decimal_places {
        return Err(v422(
            "decimal_max_places",
            &["body", field],
            &format!("Decimal input should have no more than {decimal_places} decimal places"),
            t,
        ));
    }
    Ok(if neg {
        format!("-{rest}")
    } else {
        rest.to_string()
    })
}

/// Валідація max_length рядка (як Pydantic Field(max_length=...)).
fn check_max_len(s: &str, max: usize, field: &str) -> Result<(), CrudError> {
    if s.chars().count() > max {
        return Err(v422(
            "string_too_long",
            &["body", field],
            &format!("String should have at most {max} characters"),
            s,
        ));
    }
    Ok(())
}

// ─── Парсинг тіла (serde_json::Value → вхідні структури) ──────────────────

/// Значення поля: відсутнє → None; null → Some(None); інакше Some(Some(..)).
fn field_str(v: &Value, key: &str) -> Option<Option<String>> {
    let field = v.get(key)?;
    if field.is_null() {
        Some(None)
    } else {
        if let Some(st) = field.as_str() {
            Some(Some(st.to_string()))
        } else {
            field.as_f64().map(|f| Some(normalize_number(f)))
        }
    }
}

/// JSON number → рядок без зайвих нулів: 142.7 → "142.7", 16 → "16".
fn normalize_number(f: f64) -> String {
    if f == f.trunc() && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        let s = format!("{f}");
        s
    }
}

/// Парсить поле як UUID: відсутнє → Ok(None); null → Ok(Some(None));
/// невалідний → 422 (uuid_parsing).
fn field_uuid(v: &Value, key: &str) -> Result<Option<Option<Uuid>>, CrudError> {
    let Some(field) = v.get(key) else {
        return Ok(None);
    };
    if field.is_null() {
        return Ok(Some(None));
    }
    let s = field.as_str().ok_or_else(|| {
        v422(
            "uuid_type",
            &["body", key],
            "Input should be a valid UUID",
            &field.to_string(),
        )
    })?;
    Uuid::parse_str(s).map(|u| Some(Some(u))).map_err(|_| {
        v422(
            "uuid_parsing",
            &["body", key],
            "Input should be a valid UUID",
            s,
        )
    })
}

/// Парсить Decimal-поле з валідацією (max_digits, decimal_places).
fn field_decimal(
    v: &Value,
    key: &str,
    max_digits: usize,
    decimal_places: usize,
) -> Result<Option<Option<String>>, CrudError> {
    let Some(raw) = field_str(v, key) else {
        return Ok(None);
    };
    match raw {
        None => Ok(Some(None)),
        Some(s) => Ok(Some(Some(validate_decimal(
            &s,
            max_digits,
            decimal_places,
            key,
        )?))),
    }
}

/// Парсить bool-поле: відсутнє → Ok(None); null → помилка (Python: bool не
/// приймає null); не bool → 422.
fn field_bool(v: &Value, key: &str) -> Result<Option<bool>, CrudError> {
    let Some(field) = v.get(key) else {
        return Ok(None);
    };
    field.as_bool().map(Some).ok_or_else(|| {
        v422(
            "bool_parsing",
            &["body", key],
            "Input should be a valid boolean",
            &field.to_string(),
        )
    })
}

/// Парсить datetime: ISO без TZ або RFC3339 з TZ (Python прибирає TZ).
fn field_datetime(v: &Value, key: &str) -> Result<NaiveDateTime, CrudError> {
    let Some(field) = v.get(key) else {
        return Err(v422("missing", &["body", key], "Field required", ""));
    };
    let s = field.as_str().ok_or_else(|| {
        v422(
            "datetime_type",
            &["body", key],
            "Input should be a valid datetime",
            &field.to_string(),
        )
    })?;
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(ndt);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.naive_utc());
    }
    Err(v422(
        "datetime_parsing",
        &["body", key],
        "Input should be a valid datetime",
        s,
    ))
}

fn parse_inventory_item(v: &Value, idx: usize) -> Result<InventoryItemInput, CrudError> {
    let prefix = format!("items.{idx}");
    let product_id = field_uuid(v, "product_id")?
        .flatten()
        .ok_or_else(|| v422("missing", &["body", &prefix], "Field required", ""))?;
    let get_dec = |key: &str, max_digits: usize, places: usize| -> Result<String, CrudError> {
        field_decimal(v, key, max_digits, places)?
            .flatten()
            .ok_or_else(|| v422("missing", &["body", &prefix], "Field required", ""))
    };
    Ok(InventoryItemInput {
        product_id,
        actual_quantity: get_dec("actual_quantity", 10, 3)?,
        accounting_quantity: get_dec("accounting_quantity", 10, 3)?,
        difference: get_dec("difference", 10, 3)?,
        cost_price: get_dec("cost_price", 12, 2)?,
        price: get_dec("price", 12, 2)?,
    })
}

fn parse_product_create(v: &Value) -> Result<ProductCreateInput, CrudError> {
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .ok_or_else(|| v422("missing", &["body", "title"], "Field required", ""))?;
    check_max_len(title, 255, "title")?;
    for (key, max) in [
        ("barcode", 50),
        ("sku", 100),
        ("uktzed", 10),
        ("tax_group", 2),
        ("unit", 10),
    ] {
        if let Some(Some(s)) = field_str(v, key) {
            check_max_len(&s, max, key)?;
        }
    }
    Ok(ProductCreateInput {
        barcode: field_str(v, "barcode").flatten(),
        sku: field_str(v, "sku").flatten(),
        title: title.to_string(),
        description: field_str(v, "description").flatten(),
        price: field_decimal(v, "price", 10, 2)?.flatten(),
        cost_price: field_decimal(v, "cost_price", 10, 2)?.flatten(),
        markup: field_decimal(v, "markup", 5, 2)?.flatten(),
        stock: field_decimal(v, "stock", 10, 3)?.flatten(),
        recommended_qty: field_decimal(v, "recommended_qty", 10, 3)?.flatten(),
        uktzed: field_str(v, "uktzed").flatten(),
        scan_excise: field_bool(v, "scan_excise")?.unwrap_or(false),
        tax_rate: field_decimal(v, "tax_rate", 5, 2)?.flatten(),
        tax_group: field_str(v, "tax_group").flatten(),
        is_weight: field_bool(v, "is_weight")?.unwrap_or(false),
        unit: field_str(v, "unit").flatten(),
        category_id: field_uuid(v, "category_id")?.flatten(),
        supplier_id: field_uuid(v, "supplier_id")?.flatten(),
    })
}

fn parse_product_update(v: &Value) -> Result<ProductUpdateInput, CrudError> {
    for (key, max) in [
        ("barcode", 50),
        ("sku", 100),
        ("uktzed", 10),
        ("tax_group", 2),
        ("unit", 10),
    ] {
        if let Some(Some(s)) = field_str(v, key) {
            check_max_len(&s, max, key)?;
        }
    }
    if let Some(Some(s)) = field_str(v, "title") {
        check_max_len(&s, 255, "title")?;
    }
    Ok(ProductUpdateInput {
        barcode: field_str(v, "barcode"),
        sku: field_str(v, "sku"),
        title: field_str(v, "title").flatten(),
        description: field_str(v, "description"),
        price: field_decimal(v, "price", 10, 2)?,
        cost_price: field_decimal(v, "cost_price", 10, 2)?,
        markup: field_decimal(v, "markup", 5, 2)?,
        stock: field_decimal(v, "stock", 10, 3)?,
        recommended_qty: field_decimal(v, "recommended_qty", 10, 3)?,
        uktzed: field_str(v, "uktzed"),
        scan_excise: field_bool(v, "scan_excise")?,
        tax_rate: field_decimal(v, "tax_rate", 5, 2)?,
        tax_group: field_str(v, "tax_group"),
        is_weight: field_bool(v, "is_weight")?,
        unit: field_str(v, "unit"),
        category_id: field_uuid(v, "category_id")?,
        supplier_id: field_uuid(v, "supplier_id")?,
    })
}

fn parse_category_create(v: &Value) -> Result<CategoryCreateInput, CrudError> {
    let name = v
        .get("name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| v422("missing", &["body", "name"], "Field required", ""))?;
    check_max_len(name, 255, "name")?;
    Ok(CategoryCreateInput {
        name: name.to_string(),
        description: field_str(v, "description").flatten(),
        parent_id: field_uuid(v, "parent_id")?.flatten(),
    })
}

fn parse_category_update(v: &Value) -> Result<CategoryUpdateInput, CrudError> {
    if let Some(Some(s)) = field_str(v, "name") {
        check_max_len(&s, 255, "name")?;
    }
    Ok(CategoryUpdateInput {
        name: field_str(v, "name").flatten(),
        description: field_str(v, "description"),
        parent_id: field_uuid(v, "parent_id")?,
    })
}

fn parse_supplier_create(v: &Value) -> Result<SupplierCreateInput, CrudError> {
    let name = v
        .get("name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| v422("missing", &["body", "name"], "Field required", ""))?;
    check_max_len(name, 255, "name")?;
    for (key, max) in [("edrpou", 10), ("phone", 20), ("email", 255)] {
        if let Some(Some(s)) = field_str(v, key) {
            check_max_len(&s, max, key)?;
        }
    }
    Ok(SupplierCreateInput {
        name: name.to_string(),
        edrpou: field_str(v, "edrpou").flatten(),
        phone: field_str(v, "phone").flatten(),
        email: field_str(v, "email").flatten(),
        address: field_str(v, "address").flatten(),
        notes: field_str(v, "notes").flatten(),
    })
}

fn parse_supplier_update(v: &Value) -> Result<SupplierUpdateInput, CrudError> {
    if let Some(Some(s)) = field_str(v, "name") {
        check_max_len(&s, 255, "name")?;
    }
    for (key, max) in [("edrpou", 10), ("phone", 20), ("email", 255)] {
        if let Some(Some(s)) = field_str(v, key) {
            check_max_len(&s, max, key)?;
        }
    }
    Ok(SupplierUpdateInput {
        name: field_str(v, "name").flatten(),
        edrpou: field_str(v, "edrpou"),
        phone: field_str(v, "phone"),
        email: field_str(v, "email"),
        address: field_str(v, "address"),
        notes: field_str(v, "notes"),
    })
}

fn parse_inventory_create(v: &Value, created_by: Uuid) -> Result<InventoryCreateInput, CrudError> {
    let items = v
        .get("items")
        .and_then(|i| i.as_array())
        .ok_or_else(|| v422("missing", &["body", "items"], "Field required", ""))?;
    let items = items
        .iter()
        .enumerate()
        .map(|(i, item)| parse_inventory_item(item, i))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(Some(s)) = field_str(v, "number") {
        check_max_len(&s, 50, "number")?;
    }
    if let Some(Some(s)) = field_str(v, "location") {
        check_max_len(&s, 255, "location")?;
    }
    Ok(InventoryCreateInput {
        number: field_str(v, "number").flatten(),
        location: field_str(v, "location").flatten(),
        inventory_date: field_datetime(v, "inventory_date")?,
        notes: field_str(v, "notes").flatten(),
        items,
        created_by,
    })
}

fn parse_inventory_update(v: &Value) -> Result<InventoryUpdateInput, CrudError> {
    if let Some(Some(s)) = field_str(v, "number") {
        check_max_len(&s, 50, "number")?;
    }
    if let Some(Some(s)) = field_str(v, "location") {
        check_max_len(&s, 255, "location")?;
    }
    let items = match v.get("items") {
        None | Some(Value::Null) => None,
        Some(Value::Array(arr)) => Some(
            arr.iter()
                .enumerate()
                .map(|(i, item)| parse_inventory_item(item, i))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => {
            return Err(v422(
                "list_type",
                &["body", "items"],
                "Input should be a valid list",
                "not a list",
            ))
        }
    };
    let inventory_date = match v.get("inventory_date") {
        None | Some(Value::Null) => None,
        Some(_) => Some(field_datetime(v, "inventory_date")?),
    };
    Ok(InventoryUpdateInput {
        number: field_str(v, "number"),
        location: field_str(v, "location"),
        inventory_date,
        notes: field_str(v, "notes"),
        items,
    })
}

// ─── Query-параметри ────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
}

// ─── Products ───────────────────────────────────────────────────────────────

/// GET /api/v1/products/{id}
pub async fn get_product(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::ProductDto>, CrudError> {
    let id = path_uuid(id, "product_id")?;
    let repo = read_repo(&state)?;
    let svc = ReadDirectoryService::new(repo);
    Ok(Json(svc.get_product(id).await?))
}

/// GET /api/v1/products/barcode/{barcode}
pub async fn get_product_by_barcode(
    State(state): State<AppState>,
    Path(barcode): Path<String>,
) -> Result<Json<kasa_domain::ProductDto>, CrudError> {
    let repo = read_repo(&state)?;
    let svc = ReadDirectoryService::new(repo);
    Ok(Json(svc.get_product_by_barcode(&barcode).await?))
}

/// POST /api/v1/products → 201
pub async fn create_product(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::ProductDto>), CrudError> {
    require_admin(&state, &claims).await?;
    let input = parse_product_create(&body)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok((StatusCode::CREATED, Json(svc.create_product(&input).await?)))
}

/// PUT /api/v1/products/{id} → 200
pub async fn update_product(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::ProductDto>, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "product_id")?;
    let input = parse_product_update(&body)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok(Json(svc.update_product(id, &input).await?))
}

/// DELETE /api/v1/products/{id} → 204
pub async fn delete_product(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "product_id")?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    svc.delete_product(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Categories ─────────────────────────────────────────────────────────────

/// GET /api/v1/categories/{id}
pub async fn get_category(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::CategoryDto>, CrudError> {
    let id = path_uuid(id, "category_id")?;
    let repo = read_repo(&state)?;
    let svc = ReadDirectoryService::new(repo);
    Ok(Json(svc.get_category(id).await?))
}

/// POST /api/v1/categories → 201
pub async fn create_category(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::CategoryDto>), CrudError> {
    require_admin(&state, &claims).await?;
    let input = parse_category_create(&body)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_category(&input).await?),
    ))
}

/// PUT /api/v1/categories/{id} → 200
pub async fn update_category(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::CategoryDto>, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "category_id")?;
    let input = parse_category_update(&body)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok(Json(svc.update_category(id, &input).await?))
}

/// DELETE /api/v1/categories/{id} → 204
pub async fn delete_category(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "category_id")?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    svc.delete_category(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Suppliers ──────────────────────────────────────────────────────────────

/// GET /api/v1/suppliers/{id}
pub async fn get_supplier(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::SupplierDto>, CrudError> {
    let id = path_uuid(id, "supplier_id")?;
    let repo = read_repo(&state)?;
    let svc = ReadDirectoryService::new(repo);
    Ok(Json(svc.get_supplier(id).await?))
}

/// GET /api/v1/suppliers/all (без пагінації)
pub async fn list_all_suppliers(
    State(state): State<AppState>,
) -> Result<Json<Vec<kasa_domain::SupplierDto>>, CrudError> {
    let repo = read_repo(&state)?;
    let svc = ReadDirectoryService::new(repo);
    Ok(Json(svc.list_all_suppliers().await?))
}

/// POST /api/v1/suppliers → 201
pub async fn create_supplier(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::SupplierDto>), CrudError> {
    require_admin(&state, &claims).await?;
    let input = parse_supplier_create(&body)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_supplier(&input).await?),
    ))
}

/// PUT /api/v1/suppliers/{id} → 200
pub async fn update_supplier(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::SupplierDto>, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "supplier_id")?;
    let input = parse_supplier_update(&body)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok(Json(svc.update_supplier(id, &input).await?))
}

/// DELETE /api/v1/suppliers/{id} → 204
pub async fn delete_supplier(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "supplier_id")?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    svc.delete_supplier(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Inventory ──────────────────────────────────────────────────────────────

/// GET /api/v1/inventory?page=&size=
pub async fn list_inventories(
    State(state): State<AppState>,
    Query(q): Query<PageQuery>,
) -> Result<Json<serde_json::Value>, CrudError> {
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    let page = svc
        .list_inventories(q.page.unwrap_or(1), q.size.unwrap_or(50))
        .await?;
    Ok(Json(serde_json::to_value(page).unwrap_or_default()))
}

/// GET /api/v1/inventory/counts
pub async fn inventory_counts(
    State(state): State<AppState>,
) -> Result<Json<kasa_domain::InventoryCountsDto>, CrudError> {
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok(Json(svc.inventory_counts().await?))
}

/// GET /api/v1/inventory/{id}
pub async fn get_inventory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<kasa_domain::InventoryDto>, CrudError> {
    let id = path_uuid(id, "inventory_id")?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok(Json(svc.get_inventory(id).await?))
}

/// POST /api/v1/inventory → 201
pub async fn create_inventory(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<kasa_domain::InventoryDto>), CrudError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        CrudError::Unauthorized("Недійсний токен: відсутній ідентифікатор користувача".to_string())
    })?;
    require_admin(&state, &claims).await?;
    let input = parse_inventory_create(&body, user_id)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok((
        StatusCode::CREATED,
        Json(svc.create_inventory(&input).await?),
    ))
}

/// PUT /api/v1/inventory/{id} → 200
pub async fn update_inventory(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::InventoryDto>, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "inventory_id")?;
    let input = parse_inventory_update(&body)?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    Ok(Json(svc.update_inventory(id, &input).await?))
}

/// DELETE /api/v1/inventory/{id} → 204
pub async fn delete_inventory(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "inventory_id")?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    svc.delete_inventory(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/inventory/{id}/confirm → 200
pub async fn confirm_inventory(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::InventoryDto>, CrudError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(id, "inventory_id")?;
    // Python: {"status": "confirmed" | "cancelled"} — інакше 400.
    let status = body
        .get("status")
        .and_then(|s| s.as_str())
        .ok_or_else(|| v422("missing", &["body", "status"], "Field required", ""))?;
    let repo = write_repo(&state)?;
    let svc = WriteService::new(repo);
    match status {
        "confirmed" => Ok(Json(svc.confirm_inventory(id).await?)),
        "cancelled" => Ok(Json(svc.cancel_inventory(id).await?)),
        // "draft" — валідний enum, але недопустимий для confirm → 400 (як Python else).
        "draft" => Err(CrudError::Service(ServiceError::Write(
            kasa_domain::WriteError::BadRequest(
                "Невірний статус. Використовуйте 'confirmed' або 'cancelled'".to_string(),
            ),
        ))),
        // Невалідний enum → 422 (Pydantic InventoryStatus).
        other => Err(v422(
            "enum",
            &["body", "status"],
            "Input should be 'draft', 'confirmed' or 'cancelled'",
            other,
        )),
    }
}
