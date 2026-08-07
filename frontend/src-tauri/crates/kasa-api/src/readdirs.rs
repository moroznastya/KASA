//! Хендлери GET-довідників (етап 1 — Rust-гілка під feature-flag).
//!
//! Відповідають Python-еталону:
//! - `GET /api/v1/products` — список з пошуком/фільтрами/пагінацією
//! - `GET /api/v1/categories` — список з пагінацією
//! - `GET /api/v1/suppliers` — список з пагінацією та балансом
//!
//! Query-параметри парсяться вручну (не через `Option<Uuid>` serde), щоб
//! відтворити Python `_uuid_or_none`: порожній рядок → `None`, невалідний
//! UUID → HTTP 400 з тим самим повідомленням.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use kasa_application::ReadDirectoryService;
use kasa_domain::{CategoryDto, DirectoryError, Page, ProductDto, ProductFilters, SupplierDto};
use uuid::Uuid;

use crate::AppState;

/// Raw query-параметри GET /api/v1/products (всі опційні, як Python).
#[derive(Debug, Default, Deserialize)]
pub struct RawProductQuery {
    pub query: Option<String>,
    pub search: Option<String>,
    pub barcode: Option<String>,
    pub category_id: Option<String>,
    pub supplier_id: Option<String>,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub is_weight: Option<bool>,
    pub page: Option<i64>,
    pub size: Option<i64>,
}

/// Raw query-параметри GET /api/v1/categories та /suppliers.
#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
}

/// Помилки readdirs-хендлерів → HTTP-відповіді.
#[derive(Debug, thiserror::Error)]
pub enum ReaddirsError {
    #[error(transparent)]
    Service(#[from] kasa_application::ServiceError),
    #[error("невалідний запит: {0}")]
    BadRequest(String),
}

impl ReaddirsError {
    /// UUID з рядка: `''` → None (як Python `_uuid_or_none`), невалідний → 400.
    fn uuid_or_none(value: Option<String>, field: &'static str) -> Result<Option<Uuid>, Self> {
        match value {
            None => Ok(None),
            Some(s) if s.trim().is_empty() => Ok(None),
            Some(s) => Uuid::parse_str(s.trim()).map(Some).map_err(|_| {
                ReaddirsError::Service(kasa_application::ServiceError::Directory(
                    DirectoryError::InvalidUuid { field, value: s },
                ))
            }),
        }
    }
}

/// Достає Rust-репозиторій зі стану (роутер монтує ці хендлери лише
/// коли readdirs Some — ця гілка фактично недосяжна, але безпечна).
fn rust_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn kasa_domain::ReadDirectories + Send + Sync>, ReaddirsError> {
    state
        .readdirs
        .clone()
        .ok_or_else(|| ReaddirsError::BadRequest("Rust-гілка довідників вимкнена".to_string()))
}

impl IntoResponse for ReaddirsError {
    fn into_response(self) -> Response {
        match self {
            // Python: 400 з detail "Поле X: очікується UUID, отримано: '...'".
            ReaddirsError::Service(kasa_application::ServiceError::Directory(
                DirectoryError::InvalidUuid { field, value },
            )) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"detail": format!("Поле {field}: очікується UUID, отримано: {value:?}")})),
            )
                .into_response(),
            ReaddirsError::Service(kasa_application::ServiceError::Directory(
                DirectoryError::Infrastructure(msg),
            )) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"detail": format!("Помилка БД довідників: {msg}")})),
            )
                .into_response(),
            ReaddirsError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
        }
    }
}

impl RawProductQuery {
    /// Зведення raw-параметрів у доменні фільтри (як Python-хендлер).
    fn into_filters(self) -> Result<ProductFilters, ReaddirsError> {
        let category_id = ReaddirsError::uuid_or_none(self.category_id, "category_id")?;
        let supplier_id = ReaddirsError::uuid_or_none(self.supplier_id, "supplier_id")?;
        Ok(ProductFilters {
            // Python: effective_query = query or search.
            query: self.query.or(self.search),
            barcode: self.barcode,
            category_id,
            supplier_id,
            min_price: self.min_price,
            max_price: self.max_price,
            is_weight: self.is_weight,
            page: self.page.unwrap_or(1),
            size: self.size.unwrap_or(20),
        })
    }
}

impl PageQuery {
    fn page_size(self) -> (i64, i64) {
        (self.page.unwrap_or(1), self.size.unwrap_or(50))
    }
}

/// GET /api/v1/products (Rust-гілка, feature-flag KASA_RUST_READDIRS=1).
pub async fn list_products(
    State(state): State<AppState>,
    Query(raw): Query<RawProductQuery>,
) -> Result<Json<Page<ProductDto>>, ReaddirsError> {
    let filters = raw.into_filters()?;
    let repo = rust_repo(&state)?;
    let service = ReadDirectoryService::new(repo);
    Ok(Json(service.list_products(&filters).await?))
}

/// GET /api/v1/categories (Rust-гілка).
pub async fn list_categories(
    State(state): State<AppState>,
    Query(raw): Query<PageQuery>,
) -> Result<Json<Page<CategoryDto>>, ReaddirsError> {
    let (page, size) = raw.page_size();
    let repo = rust_repo(&state)?;
    let service = ReadDirectoryService::new(repo);
    Ok(Json(service.list_categories(page, size).await?))
}

/// GET /api/v1/suppliers (Rust-гілка).
pub async fn list_suppliers(
    State(state): State<AppState>,
    Query(raw): Query<PageQuery>,
) -> Result<Json<Page<SupplierDto>>, ReaddirsError> {
    let (page, size) = raw.page_size();
    let repo = rust_repo(&state)?;
    let service = ReadDirectoryService::new(repo);
    Ok(Json(service.list_suppliers(page, size).await?))
}
