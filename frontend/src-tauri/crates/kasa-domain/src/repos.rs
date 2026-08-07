//! Порти репозиторіїв (trait-контракти для infrastructure) — етап 1.
//!
//! Application залежить ТІЛЬКИ від цих trait — sqlx/PostgreSQL деталі
//! заховані в kasa-infrastructure.

use crate::dto::{CategoryDto, Page, ProductDto, SupplierDto};
use uuid::Uuid;

/// Помилки read-шару довідників (мапляться з інфраструктурних).
#[derive(Debug, thiserror::Error)]
pub enum DirectoryError {
    /// Помилка рівня БД/інфраструктури (sqlx та ін.).
    #[error("інфраструктурна помилка довідника: {0}")]
    Infrastructure(String),
    /// Невалідний UUID у фільтрі (відтворює HTTP 400 Python-еталону).
    #[error("Поле {field}: очікується UUID, отримано: {value:?}")]
    InvalidUuid { field: &'static str, value: String },
}

/// Контракт читання довідників (products, categories, suppliers).
///
/// `#[async_trait]` — щоб trait був dyn-сумісним (`Arc<dyn ReadDirectories>`
/// у спільному стані фасаду).
#[async_trait::async_trait]
pub trait ReadDirectories: Send + Sync {
    /// Список товарів з пошуком, фільтрами та пагінацією
    /// (логіка ідентична `ProductService.search_products` Python-еталону).
    async fn list_products(
        &self,
        filters: &ProductFilters,
    ) -> Result<Page<ProductDto>, DirectoryError>;

    /// Список категорій з пагінацією (`ORDER BY name`, як Python).
    async fn list_categories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<CategoryDto>, DirectoryError>;

    /// Список постачальників з пагінацією та поточним балансом
    /// (`ORDER BY name`, як Python).
    async fn list_suppliers(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<SupplierDto>, DirectoryError>;
}

/// Фільтри пошуку товарів — відповідають query-параметрам Python-еталону.
#[derive(Debug, Clone, Default)]
pub struct ProductFilters {
    /// Пошуковий запит (назва, штрих-код, артикул) — ILIKE.
    pub query: Option<String>,
    /// Точний збіг штрих-коду.
    pub barcode: Option<String>,
    /// Фільтр за категорією (UUID або порожній рядок → None, як Python).
    pub category_id: Option<Uuid>,
    /// Фільтр за постачальником (UUID або порожній рядок → None, як Python).
    pub supplier_id: Option<Uuid>,
    /// Мінімальна ціна.
    pub min_price: Option<f64>,
    /// Максимальна ціна.
    pub max_price: Option<f64>,
    /// Фільтр вагових товарів.
    pub is_weight: Option<bool>,
    /// Номер сторінки (1-based).
    pub page: i64,
    /// Розмір сторінки (1..=100).
    pub size: i64,
}

impl ProductFilters {
    /// Дефолтні фільтри: сторінка 1, розмір 20 (як Python).
    pub fn default_page() -> Self {
        Self {
            page: 1,
            size: 20,
            ..Self::default()
        }
    }
}

/// Blanket: `Arc<T>` делегує [`ReadDirectories`] — зручно тримати
/// репозиторій у спільному стані фасаду (`Arc<dyn ReadDirectories>`).
#[async_trait::async_trait]
impl<T: ReadDirectories + ?Sized> ReadDirectories for std::sync::Arc<T> {
    async fn list_products(
        &self,
        filters: &ProductFilters,
    ) -> Result<Page<ProductDto>, DirectoryError> {
        (**self).list_products(filters).await
    }

    async fn list_categories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<CategoryDto>, DirectoryError> {
        (**self).list_categories(page, size).await
    }

    async fn list_suppliers(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<SupplierDto>, DirectoryError> {
        (**self).list_suppliers(page, size).await
    }
}
