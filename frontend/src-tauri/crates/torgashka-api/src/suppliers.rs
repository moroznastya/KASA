// ─────────────────────────────────────────────────────────────────────────────
// suppliers — Rust-гілка товарів постачальника та руху (дезактивація Python).
// 1:1 з Python backend/app/api/v1/suppliers.py + supplier_product_service.py:
//   GET /api/v1/suppliers/{id}/products            — товари з залишками
//   GET /api/v1/suppliers/{id}/products/{pid}/movements — рух по 5 документах
// Авторизація: JWT глобально (як Python — get_current_user).
// Монтуються під TORGASHKA_RUST_READDIRS=1; інакше — fallback → 410 (дезактивація).
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

use torgashka_domain::{DirectoryError, SupplierProductMovementsResponse, SupplierProductsResponse};

use crate::AppState;

/// Помилки хендлерів suppliers → HTTP (1:1 з Python).
#[derive(Debug)]
pub enum SupplierError {
    /// 404 — DirectoryError::NotFound.
    NotFound(String),
    /// 422 — Pydantic-валідація query (limit поза діапазоном).
    Validation(Value),
    /// 403 — Rust-гілка вимкнена (не має статись).
    Forbidden(String),
    /// 500 — інфраструктурна помилка.
    Internal(String),
}

impl From<DirectoryError> for SupplierError {
    fn from(e: DirectoryError) -> Self {
        match e {
            DirectoryError::NotFound(msg) => SupplierError::NotFound(msg),
            other => SupplierError::Internal(other.to_string()),
        }
    }
}

/// 422 Pydantic v2 для query-параметрів: `input` — рядок (як FastAPI),
/// `ctx` — межі обмеження.
fn v422(vtype: &str, loc: &[&str], msg: &str, input: &str, ctx: Value) -> SupplierError {
    SupplierError::Validation(serde_json::json!({
        "detail": [{
            "type": vtype,
            "loc": loc,
            "msg": msg,
            "input": input,
            "ctx": ctx,
        }]
    }))
}

impl IntoResponse for SupplierError {
    fn into_response(self) -> Response {
        match self {
            SupplierError::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            SupplierError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            SupplierError::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            SupplierError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
        }
    }
}

fn read_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn torgashka_domain::ReadDirectories + Send + Sync>, SupplierError> {
    state
        .readdirs
        .clone()
        .ok_or_else(|| SupplierError::Forbidden("Rust-гілка довідників вимкнена".to_string()))
}

/// Query-параметри GET /suppliers/{id}/products (search опційний).
#[derive(Debug, Default, Deserialize)]
pub struct ProductsQuery {
    pub search: Option<String>,
}

/// Query-параметри GET /suppliers/{id}/products/{pid}/movements.
/// limit: Python `Query(100, ge=1, le=500)`.
#[derive(Debug, Default, Deserialize)]
pub struct MovementsQuery {
    pub limit: Option<i64>,
}

fn parse_limit(raw: Option<i64>) -> Result<i64, SupplierError> {
    let limit = raw.unwrap_or(100);
    if limit < 1 {
        return Err(v422(
            "greater_than_equal",
            &["query", "limit"],
            "Input should be greater than or equal to 1",
            &limit.to_string(),
            serde_json::json!({"ge": 1}),
        ));
    }
    if limit > 500 {
        return Err(v422(
            "less_than_equal",
            &["query", "limit"],
            "Input should be less than or equal to 500",
            &limit.to_string(),
            serde_json::json!({"le": 500}),
        ));
    }
    Ok(limit)
}

/// GET /api/v1/suppliers/{supplier_id}/products
pub async fn products(
    State(state): State<AppState>,
    Path(supplier_id): Path<Uuid>,
    Query(q): Query<ProductsQuery>,
) -> Result<Json<SupplierProductsResponse>, SupplierError> {
    let repo = read_repo(&state)?;
    let result = repo
        .supplier_products(supplier_id, q.search.as_deref())
        .await?;
    Ok(Json(result))
}

/// GET /api/v1/suppliers/{supplier_id}/products/{product_id}/movements
pub async fn movements(
    State(state): State<AppState>,
    Path((supplier_id, product_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<MovementsQuery>,
) -> Result<Json<SupplierProductMovementsResponse>, SupplierError> {
    let limit = parse_limit(q.limit)?;
    let repo = read_repo(&state)?;
    let result = repo
        .product_movements(supplier_id, product_id, limit)
        .await?;
    Ok(Json(result))
}
