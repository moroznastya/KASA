//! DTO довідників (етап 1 міграції — read directories).
//!
//! Точні копії схем відповідей Python-еталону (pydantic):
//!   - `ProductResponse` / `ProductImageResponse` / `BarcodeResponse`
//!   - `CategoryResponse`
//!   - `SupplierResponse`
//!   - `ProductListResponse` / CategoryListResponse / SupplierListResponse → [`Page`]
//!
//! Грошові поля — `String`, бо Python-еталон серіалізує `Decimal` у JSON-рядок
//! зі збереженою scale БД (`price: "41.00"`, `stock: "0.000"`). Рядок гарантує
//! бітову ідентичність відповіді без втрати точності при Decimal↔JSON.

use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

/// Штрих-код товару (додатковий, з таблиці `barcodes`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BarcodeDto {
    pub id: Uuid,
    pub barcode: String,
    pub is_primary: bool,
    pub created_at: NaiveDateTime,
}

/// Зображення товару (з таблиці `product_images`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProductImageDto {
    pub id: Uuid,
    pub url: String,
    pub is_main: bool,
    pub sort_order: i32,
    pub created_at: NaiveDateTime,
}

/// Товар — відповідь GET /api/v1/products (item).
///
/// Порядок полів збігається з pydantic `ProductResponse` — критично для
/// snapshot-тестів (порівняння нормалізованого JSON).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProductDto {
    pub id: Uuid,
    pub barcode: Option<String>,
    pub sku: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub price: Option<String>,
    pub cost_price: Option<String>,
    pub markup: Option<String>,
    pub stock: Option<String>,
    pub recommended_qty: Option<String>,
    pub uktzed: Option<String>,
    pub scan_excise: bool,
    pub tax_rate: Option<String>,
    pub tax_group: Option<String>,
    pub is_weight: bool,
    pub unit: Option<String>,
    pub category_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub images: Vec<ProductImageDto>,
    pub barcodes: Vec<BarcodeDto>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Категорія — відповідь GET /api/v1/categories (item).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CategoryDto {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub parent_id: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Постачальник — відповідь GET /api/v1/suppliers (item).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupplierDto {
    pub id: Uuid,
    pub name: String,
    pub edrpou: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    /// Поточний борг (грн) — рядок, бо Python віддає `Decimal` як JSON-рядок.
    pub current_balance: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Універсальна обгортка списку з пагінацією (як `ProductListResponse`):
/// `{items, total, page, page_size, pages}`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

// ─── Інвентаризація (етап 2) ───────────────────────────────────────────────

/// Коротка інформація про товар у позиції інвентаризації (`ProductBrief`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProductBriefDto {
    pub id: Uuid,
    pub title: String,
    pub barcode: Option<String>,
}

/// Позиція інвентаризації (`InventoryItemResponse`).
///
/// `total_cost` / `total_selling` — завжди число 0: Python-еталон не обчислює
/// їх у відповіді (`InventoryItemResponse.model_validate` без атрибута →
/// default 0, серіалізується як JSON-число).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InventoryItemDto {
    pub id: Uuid,
    pub inventory_id: Uuid,
    pub product_id: Uuid,
    pub product: Option<ProductBriefDto>,
    pub actual_quantity: String,
    pub accounting_quantity: String,
    pub difference: String,
    pub cost_price: String,
    pub price: String,
    pub total_cost: i64,
    pub total_selling: i64,
    pub created_at: NaiveDateTime,
}

/// Підсумки інвентаризації (`InventorySummary`): суми зі scale добутку
/// (Python: Decimal-множення, scale = сума масштабів операндів).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InventorySummaryDto {
    pub total_cost: String,
    pub total_selling: String,
    pub total_deviation: String,
}

/// Інвентаризація (`InventoryResponse`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InventoryDto {
    pub id: Uuid,
    pub number: String,
    pub location: String,
    pub inventory_date: NaiveDateTime,
    /// `draft` | `confirmed` | `cancelled` (значення enum `inventory_status`).
    pub status: String,
    pub notes: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub items: Vec<InventoryItemDto>,
    pub summary: InventorySummaryDto,
}
