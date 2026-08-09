//! DTO постачальника: товари з залишками та рух по документах (дезактивація).
//!
//! Точні копії схем відповідей Python-еталону (`app/schemas/supplier_products.py`):
//!   - `SupplierProductsResponse`            — GET /api/v1/suppliers/{id}/products
//!   - `SupplierProductMovementsResponse`    — GET /api/v1/suppliers/{id}/products/{pid}/movements
//!
//! Грошові поля — `String`, бо Python-еталон серіалізує `Decimal` у JSON-рядок
//! зі збереженою scale БД (колонки NUMERIC: `quantity` 10,3; `price` 10,2;
//! `total` 12,2). SQL `::text` відтворює scale бітово-ідентично.

use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

/// Товар постачальника з поточним залишком (`SupplierProductItem`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupplierProductItem {
    pub id: Uuid,
    pub barcode: Option<String>,
    pub sku: Option<String>,
    pub title: String,
    pub price: Option<String>,
    pub cost_price: Option<String>,
    pub stock: Option<String>,
    pub unit: Option<String>,
    pub category_name: Option<String>,
}

/// Один запис руху товару (`SupplierProductMovement`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupplierProductMovement {
    pub id: Uuid,
    pub date: NaiveDateTime,
    pub document_type: String,
    pub document_number: String,
    pub document_id: Uuid,
    /// Кількість: додатна — прихід, від'ємна — витрата.
    pub quantity: String,
    pub price: Option<String>,
    pub total: Option<String>,
    pub notes: Option<String>,
}

/// Відповідь зі списком товарів постачальника (`SupplierProductsResponse`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupplierProductsResponse {
    pub supplier_id: Uuid,
    pub supplier_name: String,
    pub total_products: i64,
    pub total_stock_value: String,
    pub products: Vec<SupplierProductItem>,
}

/// Відповідь з рухом конкретного товару постачальника
/// (`SupplierProductMovementsResponse`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SupplierProductMovementsResponse {
    pub product: SupplierProductItem,
    pub movements: Vec<SupplierProductMovement>,
    pub total_movements: i64,
}
