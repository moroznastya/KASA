//! Порти репозиторіїв (trait-контракти для infrastructure) — етап 1.
//!
//! Application залежить ТІЛЬКИ від цих trait — sqlx/PostgreSQL деталі
//! заховані в torgashka-infrastructure.

use crate::dto::{CategoryDto, Page, ProductDto, SupplierDto};
use crate::suppliers::{SupplierProductMovementsResponse, SupplierProductsResponse};
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
    /// Не знайдено за ID (відтворює HTTP 404 Python-еталону).
    #[error("{0}")]
    NotFound(String),
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

    /// Пошук категорій за назвою (ILIKE) з пагінацією — v2 (як Python v2 list).
    async fn search_categories(
        &self,
        page: i64,
        size: i64,
        search: Option<&str>,
    ) -> Result<Page<CategoryDto>, DirectoryError>;

    /// Всі категорії без пагінації (`ORDER BY name`) — для дерева v2.
    async fn find_all_categories(&self) -> Result<Vec<CategoryDto>, DirectoryError>;

    /// Список постачальників з пагінацією та поточним балансом
    /// (`ORDER BY name`, як Python).
    async fn list_suppliers(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<SupplierDto>, DirectoryError>;

    // ─── Етап 2: читання за ID (CRUD-частина) ──────────────────────────────
    /// Товар за ID зі зв'язками (404 з текстом Python-еталону).
    async fn get_product(&self, id: Uuid) -> Result<ProductDto, DirectoryError>;
    /// Товар за штрих-кодом (спочатку products.barcode, потім barcodes).
    async fn get_product_by_barcode(&self, barcode: &str) -> Result<ProductDto, DirectoryError>;
    /// Категорія за ID.
    async fn get_category(&self, id: Uuid) -> Result<CategoryDto, DirectoryError>;
    /// Постачальник за ID з балансом.
    async fn get_supplier(&self, id: Uuid) -> Result<SupplierDto, DirectoryError>;
    /// Всі постачальники без пагінації (для випадаючих списків).
    async fn list_all_suppliers(&self) -> Result<Vec<SupplierDto>, DirectoryError>;

    // ─── Дезактивація Python (CRIT): товари постачальника та рух ──────────
    /// Товари постачальника з залишками та загальною вартістю
    /// (1:1 `SupplierProductService.get_supplier_products` Python-еталону).
    async fn supplier_products(
        &self,
        supplier_id: Uuid,
        search: Option<&str>,
    ) -> Result<SupplierProductsResponse, DirectoryError>;

    /// Рух товару по 5 типах документів (invoice, return_invoice, receipt,
    /// write_off, transfer) з сортуванням за датою та limit
    /// (1:1 `SupplierProductService.get_product_movements` Python-еталону).
    async fn product_movements(
        &self,
        supplier_id: Uuid,
        product_id: Uuid,
        limit: i64,
    ) -> Result<SupplierProductMovementsResponse, DirectoryError>;
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

    async fn search_categories(
        &self,
        page: i64,
        size: i64,
        search: Option<&str>,
    ) -> Result<Page<CategoryDto>, DirectoryError> {
        (**self).search_categories(page, size, search).await
    }

    async fn find_all_categories(&self) -> Result<Vec<CategoryDto>, DirectoryError> {
        (**self).find_all_categories().await
    }

    async fn list_suppliers(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<SupplierDto>, DirectoryError> {
        (**self).list_suppliers(page, size).await
    }

    async fn get_product(&self, id: Uuid) -> Result<ProductDto, DirectoryError> {
        (**self).get_product(id).await
    }

    async fn get_product_by_barcode(&self, barcode: &str) -> Result<ProductDto, DirectoryError> {
        (**self).get_product_by_barcode(barcode).await
    }

    async fn get_category(&self, id: Uuid) -> Result<CategoryDto, DirectoryError> {
        (**self).get_category(id).await
    }

    async fn get_supplier(&self, id: Uuid) -> Result<SupplierDto, DirectoryError> {
        (**self).get_supplier(id).await
    }

    async fn list_all_suppliers(&self) -> Result<Vec<SupplierDto>, DirectoryError> {
        (**self).list_all_suppliers().await
    }

    async fn supplier_products(
        &self,
        supplier_id: Uuid,
        search: Option<&str>,
    ) -> Result<SupplierProductsResponse, DirectoryError> {
        (**self).supplier_products(supplier_id, search).await
    }

    async fn product_movements(
        &self,
        supplier_id: Uuid,
        product_id: Uuid,
        limit: i64,
    ) -> Result<SupplierProductMovementsResponse, DirectoryError> {
        (**self)
            .product_movements(supplier_id, product_id, limit)
            .await
    }
}
