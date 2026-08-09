//! Товари v2: CRUD + зображення + штрих-коди (етап 8 — група 7).
//!
//! 1:1 з Python backend/app/api/v2/products.py (10 роутів, 360 рядків).
//! Включено serve зображень: GET /uploads/products/{product_id}/{filename}
//! з диска (Python app.mount("/uploads", StaticFiles(...))) — роут під
//! публічним шляхом (auth.rs пропускає /uploads/ без токена).
//!
//! Відмінності від v1 (crud.rs):
//!   - create: Pydantic 422 (name 1..255, barcode 1..50, price gt=0);
//!     дублікат barcode/sku → 400 (Python ValueError), не 409.
//!   - update: без category_id/supplier_id/quantity (Python UpdateProductRequest).
//!   - delete: detail 400 «... залишок на складі 5.0 шт.» (Python float).

use std::path::{Path, PathBuf};

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use torgashka_domain::{ProductsV2Error, ProductsV2Service};

use crate::AppState;

/// Максимальний розмір завантаженого зображення (Python без явного ліміту).
const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;

/// Query GET /api/v2/products (всі опційні, як Python).
#[derive(Debug, Default, Deserialize)]
pub struct RawListQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
    pub search: Option<String>,
    pub category_id: Option<String>,
}

/// Отримання сервісу зі стану (недосяжна гілка коли flag вимкнено).
fn svc(
    state: &AppState,
) -> Result<std::sync::Arc<dyn ProductsV2Service + Send + Sync>, ProductsV2ApiError> {
    state.products_v2.clone().ok_or_else(|| {
        ProductsV2ApiError::Domain(ProductsV2Error::Infrastructure(
            "Rust-гілка товарів v2 вимкнена".into(),
        ))
    })
}

/// Local error: IntoResponse можна імплементувати лише для local типів
/// (orphan rule) — обгортка над domain ProductsV2Error.
#[derive(Debug)]
pub enum ProductsV2ApiError {
    Domain(ProductsV2Error),
}

impl From<ProductsV2Error> for ProductsV2ApiError {
    fn from(e: ProductsV2Error) -> Self {
        ProductsV2ApiError::Domain(e)
    }
}

/// 422 Pydantic-деталь → local error.
fn v422(detail: Value) -> ProductsV2ApiError {
    ProductsV2ApiError::Domain(ProductsV2Error::Validation(detail))
}

/// Помилки → HTTP (1:1 Python v2/products.py).
impl IntoResponse for ProductsV2ApiError {
    fn into_response(self) -> Response {
        match self {
            ProductsV2ApiError::Domain(e) => match e {
                ProductsV2Error::NotFound(msg) => {
                    (StatusCode::NOT_FOUND, Json(json!({"detail": msg}))).into_response()
                }
                ProductsV2Error::BadRequest(msg) => {
                    (StatusCode::BAD_REQUEST, Json(json!({"detail": msg}))).into_response()
                }
                ProductsV2Error::Conflict(msg) => {
                    (StatusCode::CONFLICT, Json(json!({"detail": msg}))).into_response()
                }
                ProductsV2Error::Validation(detail) => {
                    (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
                }
                ProductsV2Error::Infrastructure(msg) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"detail": msg})),
                )
                    .into_response(),
            },
        }
    }
}

/// 422 Pydantic: string_too_short (min_length) → Value (для збору помилок).
fn err_str_short(loc: &str, value: &str, min: usize) -> Value {
    json!({"type": "string_too_short", "loc": ["body", loc],
           "msg": format!("String should have at least {min} character"),
           "input": value, "ctx": {"min_length": min}})
}

/// 422 Pydantic: string_too_long (max_length) → Value.
fn err_str_long(loc: &str, value: &str, max: usize) -> Value {
    json!({"type": "string_too_long", "loc": ["body", loc],
           "msg": format!("String should have at most {max} characters"),
           "input": value, "ctx": {"max_length": max}})
}

/// 422 Pydantic: missing (Field required) → Value (Python input = весь body).
fn err_missing(loc: &str, body: &Value) -> Value {
    json!({"type": "missing", "loc": ["body", loc], "msg": "Field required",
           "input": body})
}

/// 422 Pydantic: greater_than (gt) → Value (input 0 int, ctx.gt 0.0 float).
fn err_gt(loc: &str, value: f64, gt: f64) -> Value {
    let input = if value.fract() == 0.0 {
        json!((value as i64))
    } else {
        json!(value)
    };
    json!({"type": "greater_than", "loc": ["body", loc],
           "msg": format!("Input should be greater than {gt}"),
           "input": input, "ctx": {"gt": gt}})
}

/// 422 Pydantic: string_too_short → ProductsV2ApiError (одна помилка).
fn py_str_short(loc: &str, value: &str, min: usize) -> ProductsV2ApiError {
    v422(json!({"detail": [err_str_short(loc, value, min)]}))
}

/// 422 Pydantic: string_too_long → ProductsV2ApiError (одна помилка).
fn py_str_long(loc: &str, value: &str, max: usize) -> ProductsV2ApiError {
    v422(json!({"detail": [err_str_long(loc, value, max)]}))
}

/// 422 Pydantic: missing (barcode в add_barcode — без input body).
fn py_missing(loc: &str) -> ProductsV2ApiError {
    v422(json!({"detail": [{
        "type": "missing",
        "loc": ["body", loc],
        "msg": "Field required",
    }]}))
}

/// Path UUID → 422 uuid_parsing (як FastAPI path param UUID).
fn path_uuid(raw: &str, field: &'static str) -> Result<Uuid, ProductsV2ApiError> {
    Uuid::parse_str(raw).map_err(|_| {
        v422(json!({"detail": [{
            "type": "uuid_parsing",
            "loc": ["path", field],
            "msg": "Input should be a valid UUID",
            "input": raw,
        }]}))
    })
}

// ─── GET /api/v2/products ─────────────────────────────────────────────────────

pub async fn list_products(
    State(state): State<AppState>,
    Query(q): Query<RawListQuery>,
) -> Result<Json<Value>, ProductsV2ApiError> {
    let page = q.page.unwrap_or(1);
    let size = q.size.unwrap_or(20);
    if page < 1 {
        return Err(v422(json!({"detail": [{
            "type": "greater_than_equal",
            "loc": ["query", "page"],
            "msg": "Input should be greater than or equal to 1",
            "input": page.to_string(),
            "ctx": {"ge": 1},
        }]})));
    }
    if !(1..=100).contains(&size) {
        return Err(v422(json!({"detail": [{
            "type": if size < 1 { "greater_than_equal" } else { "less_than_equal" },
            "loc": ["query", "size"],
            "msg": if size < 1 {
                "Input should be greater than or equal to 1"
            } else {
                "Input should be less than or equal to 100"
            },
            "input": size.to_string(),
            "ctx": if size < 1 { json!({"ge": 1}) } else { json!({"le": 100}) },
        }]})));
    }
    // category_id: '' → None, невалідний UUID → 400 (Python _uuid_or_none).
    let category_id = match q.category_id.as_deref() {
        None => None,
        Some(s) if s.trim().is_empty() => None,
        Some(s) => Some(Uuid::parse_str(s.trim()).map_err(|_| {
            ProductsV2ApiError::Domain(ProductsV2Error::BadRequest(format!(
                "Поле category_id: очікується UUID, отримано: '{s}'"
            )))
        })?),
    };
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.list(page, size, q.search.as_deref(), category_id).await?).unwrap(),
    ))
}

// ─── GET /api/v2/products/barcode/{barcode} ───────────────────────────────────

pub async fn get_by_barcode(
    State(state): State<AppState>,
    AxumPath(barcode): AxumPath<String>,
) -> Result<Json<Value>, ProductsV2ApiError> {
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.get_by_barcode(&barcode).await?).unwrap(),
    ))
}

// ─── POST /api/v2/products/{product_id}/images (multipart) ───────────────────

pub async fn upload_image(
    State(state): State<AppState>,
    AxumPath(product_id): AxumPath<String>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ProductsV2ApiError> {
    let product_id = path_uuid(&product_id, "product_id")?;
    let s = svc(&state)?;

    // Парсимо multipart: file (UploadFile) + is_main (Form bool).
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut is_main = false;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ProductsV2Error::BadRequest(format!("Помилка multipart: {e}")))?
    {
        match field.name() {
            Some("file") => {
                file_name = field.file_name().map(|s| s.to_string());
                file_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| {
                            ProductsV2Error::BadRequest(format!("Помилка читання файлу: {e}"))
                        })?
                        .to_vec(),
                );
            }
            Some("is_main") => {
                let raw = field.text().await.unwrap_or_default();
                is_main = matches!(
                    raw.trim().to_lowercase().as_str(),
                    "true" | "1" | "yes" | "on"
                );
            }
            _ => {}
        }
    }
    let Some(bytes) = file_bytes else {
        return Err(ProductsV2Error::BadRequest(
            "Файл не передано (очікується поле 'file')".into(),
        )
        .into());
    };
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ProductsV2Error::BadRequest(format!(
            "Файл завеликий: {} байт (ліміт {MAX_UPLOAD_BYTES})",
            bytes.len()
        ))
        .into());
    }

    // Python: ext = splitext(file.filename or "image.jpg")[1].
    let ext = Path::new(file_name.as_deref().unwrap_or("image.jpg"))
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_else(|| ".jpg".into());

    let filename = format!("{}{}", Uuid::new_v4(), ext);
    let rel_dir = PathBuf::from("products").join(product_id.to_string());
    let dir = state.uploads_dir.join(&rel_dir);
    std::fs::create_dir_all(&dir).map_err(|e| {
        ProductsV2Error::Infrastructure(format!("не вдалося створити {dir:?}: {e}"))
    })?;
    let filepath = dir.join(&filename);
    std::fs::write(&filepath, &bytes).map_err(|e| {
        ProductsV2Error::Infrastructure(format!("не вдалося зберегти {filepath:?}: {e}"))
    })?;

    let url = format!("/uploads/products/{product_id}/{filename}");
    let dto = s.add_image(product_id, &url, is_main).await?;
    Ok(Json(serde_json::to_value(dto).unwrap()))
}

// ─── DELETE /api/v2/products/{product_id}/images/{image_id} ──────────────────

pub async fn delete_image(
    State(state): State<AppState>,
    AxumPath((product_id, image_id)): AxumPath<(String, String)>,
) -> Result<StatusCode, ProductsV2ApiError> {
    let _product_id = path_uuid(&product_id, "product_id")?;
    let image_id = path_uuid(&image_id, "image_id")?;
    let s = svc(&state)?;
    s.delete_image(image_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── POST /api/v2/products/{product_id}/barcodes ──────────────────────────────

pub async fn add_barcode(
    State(state): State<AppState>,
    AxumPath(product_id): AxumPath<String>,
    Json(body): Json<torgashka_domain::BarcodeCreateV2Input>,
) -> Result<Json<Value>, ProductsV2ApiError> {
    let product_id = path_uuid(&product_id, "product_id")?;
    let Some(barcode) = body.barcode.clone() else {
        return Err(py_missing("barcode"));
    };
    if barcode.is_empty() {
        return Err(py_str_short("barcode", &barcode, 1));
    }
    if barcode.chars().count() > 50 {
        return Err(py_str_long("barcode", &barcode, 50));
    }
    let s = svc(&state)?;
    let dto = s
        .add_barcode(product_id, &barcode, body.is_primary.unwrap_or(false))
        .await?;
    Ok(Json(serde_json::to_value(dto).unwrap()))
}

// ─── DELETE /api/v2/products/{product_id}/barcodes/{barcode_id} ───────────────

pub async fn delete_barcode(
    State(state): State<AppState>,
    AxumPath((product_id, barcode_id)): AxumPath<(String, String)>,
) -> Result<StatusCode, ProductsV2ApiError> {
    let _product_id = path_uuid(&product_id, "product_id")?;
    let barcode_id = path_uuid(&barcode_id, "barcode_id")?;
    let s = svc(&state)?;
    s.delete_barcode(barcode_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── GET /api/v2/products/{product_id} ────────────────────────────────────────

pub async fn get_product(
    State(state): State<AppState>,
    AxumPath(product_id): AxumPath<String>,
) -> Result<Json<Value>, ProductsV2ApiError> {
    let product_id = path_uuid(&product_id, "product_id")?;
    let s = svc(&state)?;
    Ok(Json(
        serde_json::to_value(s.get(product_id).await?).unwrap(),
    ))
}

// ─── POST /api/v2/products (201) ──────────────────────────────────────────────

pub async fn create_product(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Response, ProductsV2ApiError> {
    // Pydantic v2: валідує ВСІ поля тіла і збирає всі помилки в detail.
    let mut errors: Vec<Value> = Vec::new();
    let name: Option<String> = body.get("name").and_then(|v| v.as_str().map(String::from));
    match &name {
        None => errors.push(err_missing("name", &body)),
        Some(n) if n.is_empty() => errors.push(err_str_short("name", n, 1)),
        Some(n) if n.chars().count() > 255 => errors.push(err_str_long("name", n, 255)),
        _ => {}
    }
    if let Some(bc) = body.get("barcode").and_then(|v| v.as_str()) {
        if bc.is_empty() {
            errors.push(err_str_short("barcode", bc, 1));
        } else if bc.chars().count() > 50 {
            errors.push(err_str_long("barcode", bc, 50));
        }
    }
    if let Some(p) = body.get("price").and_then(|v| v.as_f64()) {
        if p <= 0.0 {
            errors.push(err_gt("price", p, 0.0));
        }
    }
    if !errors.is_empty() {
        return Err(v422(json!({"detail": errors})));
    }
    let input = torgashka_domain::ProductCreateV2Input {
        name,
        barcode: body
            .get("barcode")
            .and_then(|v| v.as_str())
            .map(String::from),
        price: body.get("price").and_then(|v| v.as_f64()),
        cost_price: body.get("cost_price").and_then(|v| v.as_f64()),
        quantity: body.get("quantity").and_then(|v| v.as_f64()),
        unit: body.get("unit").and_then(|v| v.as_str()).map(String::from),
        category_id: body
            .get("category_id")
            .and_then(|v| v.as_str())
            .and_then(|x| Uuid::parse_str(x).ok()),
        supplier_id: body
            .get("supplier_id")
            .and_then(|v| v.as_str())
            .and_then(|x| Uuid::parse_str(x).ok()),
        sku: body.get("sku").and_then(|v| v.as_str()).map(String::from),
        description: body
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    let s = svc(&state)?;
    let dto = s.create(&input).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(dto).unwrap()),
    )
        .into_response())
}

// ─── PUT /api/v2/products/{product_id} ────────────────────────────────────────

pub async fn update_product(
    State(state): State<AppState>,
    AxumPath(product_id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ProductsV2ApiError> {
    let product_id = path_uuid(&product_id, "product_id")?;
    // Pydantic v2: збирає всі помилки (UpdateProductRequest — всі Optional).
    let mut errors: Vec<Value> = Vec::new();
    if let Some(n) = body.get("name").and_then(|v| v.as_str()) {
        if n.is_empty() {
            errors.push(err_str_short("name", n, 1));
        } else if n.chars().count() > 255 {
            errors.push(err_str_long("name", n, 255));
        }
    }
    if let Some(bc) = body.get("barcode").and_then(|v| v.as_str()) {
        if bc.is_empty() {
            errors.push(err_str_short("barcode", bc, 1));
        } else if bc.chars().count() > 50 {
            errors.push(err_str_long("barcode", bc, 50));
        }
    }
    if let Some(p) = body.get("price").and_then(|v| v.as_f64()) {
        if p <= 0.0 {
            errors.push(err_gt("price", p, 0.0));
        }
    }
    if !errors.is_empty() {
        return Err(v422(json!({"detail": errors})));
    }
    let input = torgashka_domain::ProductUpdateV2Input {
        name: body.get("name").and_then(|v| v.as_str()).map(String::from),
        barcode: body
            .get("barcode")
            .and_then(|v| v.as_str())
            .map(String::from),
        price: body.get("price").and_then(|v| v.as_f64()),
        cost_price: body.get("cost_price").and_then(|v| v.as_f64()),
        unit: body.get("unit").and_then(|v| v.as_str()).map(String::from),
        is_active: body.get("is_active").and_then(|v| v.as_bool()),
        sku: body.get("sku").and_then(|v| v.as_str()).map(String::from),
        description: body
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    let s = svc(&state)?;
    let dto = s.update(product_id, &input).await?;
    Ok(Json(serde_json::to_value(dto).unwrap()))
}

// ─── DELETE /api/v2/products/{product_id} (204) ───────────────────────────────

pub async fn delete_product(
    State(state): State<AppState>,
    AxumPath(product_id): AxumPath<String>,
) -> Result<StatusCode, ProductsV2ApiError> {
    let product_id = path_uuid(&product_id, "product_id")?;
    let s = svc(&state)?;
    s.delete(product_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── GET /uploads/products/{product_id}/{filename} (static serve) ─────────────

/// Serve завантаженого зображення з диска (Python StaticFiles).
pub async fn serve_upload(
    State(state): State<AppState>,
    AxumPath((product_id, filename)): AxumPath<(String, String)>,
) -> Response {
    let rel = PathBuf::from("products").join(&product_id).join(&filename);
    let path = state.uploads_dir.join(&rel);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let ctype = content_type_for(&filename);
            ([(header::CONTENT_TYPE, ctype)], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(json!({"detail": "Not Found"}))).into_response(),
    }
}

/// Content-Type за розширенням (як Python mimetypes/StaticFiles).
fn content_type_for(filename: &str) -> &'static str {
    let ext = Path::new(filename)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "jpeg" | "jpg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// Ліміт тіла для multipart-завантаження (роут upload_image).
pub fn upload_body_limit() -> DefaultBodyLimit {
    DefaultBodyLimit::max(MAX_UPLOAD_BYTES)
}
