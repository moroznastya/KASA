//! Товари v2: зображення + додаткові штрих-коди (етап 8 — група 7).
//!
//! 1:1 з Python:
//!   - backend/app/api/v2/products.py (10 роутів, 360 рядків): CRUD v2,
//!     upload/serve зображень (multipart/form-data, static files), додаткові
//!     штрих-коди (barcodes), пошук за штрих-кодом.
//!   - use_cases/product_use_cases.py + product_repository.py + models:
//!     product_images (id, product_id, url, is_main, sort_order, created_at),
//!     barcodes (id, product_id, barcode UNIQUE, is_primary, created_at).
//!
//! Ключові відмінності v2 від v1 (crates/torgashka-domain::dto::ProductDto):
//!   - name замість title, quantity замість stock, is_active (завжди true —
//!     у таблиці products НЕМАЄ колонки is_active; Python `getattr(entity,
//!     "is_active", True)`), sku/description за замовчуванням "".
//!   - create/update: Pydantic-валідації (name 1..255, barcode 1..50,
//!     price gt=0) → 422; дублікат barcode/sku → 400 (Python ValueError),
//!     а НЕ 409 як у v1.
//!   - delete: stock != 0 → 400 «Неможливо видалити товар ...» (Python
//!     формат float: "5.0" — БЕЗ суфікса «Спочатку списати», як у v1 Rust).
//!   - зображення: POST /products/{id}/images (multipart file + is_main),
//!     файл у uploads/products/{id}/{uuid4}{ext}, url у БД; serve з диска.
//!   - додаткові штрих-коди: POST/DELETE /products/{id}/barcodes[/{bc_id}],
//!     дублікат → 409 «Штрих-код '...' вже існує».
//!
//! Barcode-генерація: у групі 7 Python НЕ генерує штрих-коди — лише зберігає
//! рядок barcode. Code128 (crates/torgashka-infrastructure/src/repositories/
//! price_tag.rs, група 3/6) перевикористовується при друці цінників.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use chrono::NaiveDateTime;

/// Помилки товарів v2 (1:1 з HTTP-статусами Python v2/products.py).
#[derive(Debug, thiserror::Error)]
pub enum ProductsV2Error {
    /// 404.
    #[error("{0}")]
    NotFound(String),
    /// 400 — бізнес-валідація (Python ValueError у хендлерах).
    #[error("{0}")]
    BadRequest(String),
    /// 409 — дублікат додаткового штрих-коду (Python add_product_barcode).
    #[error("{0}")]
    Conflict(String),
    /// 422 — Pydantic-валідація (1:1 detail).
    #[error("{0}")]
    Validation(Value),
    /// 500 — помилка БД.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

// ─── DTO відповідей (Python Pydantic-схеми) ─────────────────────────────────

/// Товар v2 (Python ProductResponse).
#[derive(Debug, Clone, Serialize)]
pub struct ProductV2Dto {
    pub id: Uuid,
    pub name: String,
    pub barcode: Option<String>,
    pub price: Option<f64>,
    pub cost_price: Option<f64>,
    pub quantity: f64,
    pub unit: String,
    pub category_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub is_active: bool,
    pub sku: String,
    pub description: String,
}

/// Список товарів v2 (Python ProductListResponse).
#[derive(Debug, Clone, Serialize)]
pub struct ProductListV2Dto {
    pub items: Vec<ProductV2Dto>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

/// Зображення товару (Python ProductImageResponse).
#[derive(Debug, Clone, Serialize)]
pub struct ProductImageV2Dto {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub url: String,
    pub is_main: bool,
    pub sort_order: i32,
    pub created_at: Option<NaiveDateTime>,
}

/// Додатковий штрих-код (Python ProductBarcodeResponse).
#[derive(Debug, Clone, Serialize)]
pub struct ProductBarcodeV2Dto {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub barcode: String,
    pub is_primary: bool,
}

// ─── Вхідні дані ─────────────────────────────────────────────────────────────

/// Створення товару v2 (Python CreateProductRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct ProductCreateV2Input {
    pub name: Option<String>,
    pub barcode: Option<String>,
    pub price: Option<f64>,
    pub cost_price: Option<f64>,
    pub quantity: Option<f64>,
    pub unit: Option<String>,
    pub category_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub sku: Option<String>,
    pub description: Option<String>,
}

/// Оновлення товару v2 (Python UpdateProductRequest).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProductUpdateV2Input {
    pub name: Option<String>,
    pub barcode: Option<String>,
    pub price: Option<f64>,
    pub cost_price: Option<f64>,
    pub unit: Option<String>,
    pub is_active: Option<bool>,
    pub sku: Option<String>,
    pub description: Option<String>,
}

/// Додатковий штрих-код (Python BarcodeCreateRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct BarcodeCreateV2Input {
    pub barcode: Option<String>,
    pub is_primary: Option<bool>,
}

// ─── Сервіс ──────────────────────────────────────────────────────────────────

/// Сервіс товарів v2 (10 роутів Python v2/products.py).
#[async_trait::async_trait]
pub trait ProductsV2Service: Send + Sync {
    /// GET /products — список з пошуком/фільтром категорії/пагінацією.
    async fn list(
        &self,
        page: i64,
        size: i64,
        search: Option<&str>,
        category_id: Option<Uuid>,
    ) -> Result<ProductListV2Dto, ProductsV2Error>;

    /// GET /products/barcode/{barcode} — основний або додатковий штрих-код.
    async fn get_by_barcode(&self, barcode: &str) -> Result<ProductV2Dto, ProductsV2Error>;

    /// POST /products — створення (Python ValueError → 400).
    async fn create(&self, input: &ProductCreateV2Input) -> Result<ProductV2Dto, ProductsV2Error>;

    /// PUT /products/{id} — оновлення (404/400).
    async fn update(
        &self,
        id: Uuid,
        input: &ProductUpdateV2Input,
    ) -> Result<ProductV2Dto, ProductsV2Error>;

    /// DELETE /products/{id} — видалення (404/400 stock != 0).
    async fn delete(&self, id: Uuid) -> Result<(), ProductsV2Error>;

    /// GET /products/{id}.
    async fn get(&self, id: Uuid) -> Result<ProductV2Dto, ProductsV2Error>;

    /// POST /products/{id}/images — url вже сформовано хендлером (файл на диску).
    async fn add_image(
        &self,
        product_id: Uuid,
        url: &str,
        is_main: bool,
    ) -> Result<ProductImageV2Dto, ProductsV2Error>;

    /// DELETE /products/{id}/images/{image_id} (404 «Зображення з ID ...»).
    async fn delete_image(&self, image_id: Uuid) -> Result<(), ProductsV2Error>;

    /// POST /products/{id}/barcodes (404 товар / 409 дублікат).
    async fn add_barcode(
        &self,
        product_id: Uuid,
        barcode: &str,
        is_primary: bool,
    ) -> Result<ProductBarcodeV2Dto, ProductsV2Error>;

    /// DELETE /products/{id}/barcodes/{barcode_id} (404).
    async fn delete_barcode(&self, barcode_id: Uuid) -> Result<(), ProductsV2Error>;
}
