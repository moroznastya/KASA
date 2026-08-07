//! Write-порти довідників та інвентаризації (етап 2 — CRUD + inventory).
//!
//! Контракт між application і infrastructure для всіх write-операцій:
//!   - products / categories / suppliers — create/update/delete
//!   - inventory — create/update/delete/confirm/cancel + read з підсумками
//!
//! Валідації та статус-коди 1:1 з Python-еталоном (ProductService,
//! categories/suppliers роутери, DocumentService):
//!   - 404: `Not found`
//!   - 409: конфлікт унікальності (barcode/sku)
//!   - 400: бізнес-правила (залишок, статус чернетки, батьківська категорія)
//!   - 403: недостатньо прав (роль адміністратора)
//!
//! Decimal-поля передаються рядками (як у JSON Python-еталону) — scale
//! вхідного значення зберігається, щоб відтворити відповіді 1:1
//! (`"142.7"` ≠ `"142.70"` — Python повертає вхідну scale через identity map).

use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

use crate::dto::{CategoryDto, InventoryDto, Page, ProductDto, SupplierDto};

/// Помилки write-шару (мапляться в HTTP-відповіді 1:1 з Python).
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// 404 Not Found — детальний текст з Python-еталону.
    #[error("{0}")]
    NotFound(String),
    /// 409 Conflict — конфлікт унікальності (barcode/sku).
    #[error("{0}")]
    Conflict(String),
    /// 400 Bad Request — бізнес-правило.
    #[error("{0}")]
    BadRequest(String),
    /// 403 Forbidden — роль не адміністратор.
    #[error("{0}")]
    Forbidden(String),
    /// 500 Internal Server Error — інфраструктура/БД.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

/// Контракт write-операцій довідників та інвентаризації.
///
/// `#[async_trait]` — dyn-сумісність (`Arc<dyn WriteDirectories>` у стані фасаду).
#[async_trait::async_trait]
pub trait WriteDirectories: Send + Sync {
    // ─── Products ───────────────────────────────────────────────────────────
    async fn create_product(&self, input: &ProductCreateInput) -> Result<ProductDto, WriteError>;
    async fn update_product(
        &self,
        id: Uuid,
        input: &ProductUpdateInput,
    ) -> Result<ProductDto, WriteError>;
    async fn delete_product(&self, id: Uuid) -> Result<(), WriteError>;

    // ─── Categories ─────────────────────────────────────────────────────────
    async fn create_category(&self, input: &CategoryCreateInput)
        -> Result<CategoryDto, WriteError>;
    async fn update_category(
        &self,
        id: Uuid,
        input: &CategoryUpdateInput,
    ) -> Result<CategoryDto, WriteError>;
    async fn delete_category(&self, id: Uuid) -> Result<(), WriteError>;

    // ─── Suppliers ──────────────────────────────────────────────────────────
    async fn create_supplier(&self, input: &SupplierCreateInput)
        -> Result<SupplierDto, WriteError>;
    async fn update_supplier(
        &self,
        id: Uuid,
        input: &SupplierUpdateInput,
    ) -> Result<SupplierDto, WriteError>;
    async fn delete_supplier(&self, id: Uuid) -> Result<(), WriteError>;

    // ─── Inventory ──────────────────────────────────────────────────────────
    async fn list_inventories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<InventoryDto>, WriteError>;
    async fn inventory_counts(&self) -> Result<InventoryCountsDto, WriteError>;
    async fn get_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError>;
    async fn create_inventory(
        &self,
        input: &InventoryCreateInput,
    ) -> Result<InventoryDto, WriteError>;
    async fn update_inventory(
        &self,
        id: Uuid,
        input: &InventoryUpdateInput,
    ) -> Result<InventoryDto, WriteError>;
    async fn delete_inventory(&self, id: Uuid) -> Result<(), WriteError>;
    async fn confirm_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError>;
    async fn cancel_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError>;
}

/// Blanket: `Arc<T>` делегує [`WriteDirectories`].
#[async_trait::async_trait]
impl<T: WriteDirectories + ?Sized> WriteDirectories for std::sync::Arc<T> {
    async fn create_product(&self, input: &ProductCreateInput) -> Result<ProductDto, WriteError> {
        (**self).create_product(input).await
    }
    async fn update_product(
        &self,
        id: Uuid,
        input: &ProductUpdateInput,
    ) -> Result<ProductDto, WriteError> {
        (**self).update_product(id, input).await
    }
    async fn delete_product(&self, id: Uuid) -> Result<(), WriteError> {
        (**self).delete_product(id).await
    }
    async fn create_category(
        &self,
        input: &CategoryCreateInput,
    ) -> Result<CategoryDto, WriteError> {
        (**self).create_category(input).await
    }
    async fn update_category(
        &self,
        id: Uuid,
        input: &CategoryUpdateInput,
    ) -> Result<CategoryDto, WriteError> {
        (**self).update_category(id, input).await
    }
    async fn delete_category(&self, id: Uuid) -> Result<(), WriteError> {
        (**self).delete_category(id).await
    }
    async fn create_supplier(
        &self,
        input: &SupplierCreateInput,
    ) -> Result<SupplierDto, WriteError> {
        (**self).create_supplier(input).await
    }
    async fn update_supplier(
        &self,
        id: Uuid,
        input: &SupplierUpdateInput,
    ) -> Result<SupplierDto, WriteError> {
        (**self).update_supplier(id, input).await
    }
    async fn delete_supplier(&self, id: Uuid) -> Result<(), WriteError> {
        (**self).delete_supplier(id).await
    }
    async fn list_inventories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<InventoryDto>, WriteError> {
        (**self).list_inventories(page, size).await
    }
    async fn inventory_counts(&self) -> Result<InventoryCountsDto, WriteError> {
        (**self).inventory_counts().await
    }
    async fn get_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError> {
        (**self).get_inventory(id).await
    }
    async fn create_inventory(
        &self,
        input: &InventoryCreateInput,
    ) -> Result<InventoryDto, WriteError> {
        (**self).create_inventory(input).await
    }
    async fn update_inventory(
        &self,
        id: Uuid,
        input: &InventoryUpdateInput,
    ) -> Result<InventoryDto, WriteError> {
        (**self).update_inventory(id, input).await
    }
    async fn delete_inventory(&self, id: Uuid) -> Result<(), WriteError> {
        (**self).delete_inventory(id).await
    }
    async fn confirm_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError> {
        (**self).confirm_inventory(id).await
    }
    async fn cancel_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError> {
        (**self).cancel_inventory(id).await
    }
}

// ─── Вхідні структури (створення) ─────────────────────────────────────────

/// Вхідні дані POST /api/v1/products (відповідає pydantic `ProductCreate`).
#[derive(Debug, Clone)]
pub struct ProductCreateInput {
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
}

/// Вхідні дані PUT /api/v1/products/{id} (відповідає `ProductUpdate`).
///
/// `Option<Option<T>>`: зовнішній `None` = поле НЕ передано (exclude_unset),
/// `Some(None)` = передано `null`, `Some(Some(v))` = передано значення.
#[derive(Debug, Clone, Default)]
pub struct ProductUpdateInput {
    pub barcode: Option<Option<String>>,
    pub sku: Option<Option<String>>,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub price: Option<Option<String>>,
    pub cost_price: Option<Option<String>>,
    pub markup: Option<Option<String>>,
    pub stock: Option<Option<String>>,
    pub recommended_qty: Option<Option<String>>,
    pub uktzed: Option<Option<String>>,
    pub scan_excise: Option<bool>,
    pub tax_rate: Option<Option<String>>,
    pub tax_group: Option<Option<String>>,
    pub is_weight: Option<bool>,
    pub unit: Option<Option<String>>,
    pub category_id: Option<Option<Uuid>>,
    pub supplier_id: Option<Option<Uuid>>,
}

/// Вхідні дані POST /api/v1/categories (`CategoryCreate`).
#[derive(Debug, Clone)]
pub struct CategoryCreateInput {
    pub name: String,
    pub description: Option<String>,
    pub parent_id: Option<Uuid>,
}

/// Вхідні дані PUT /api/v1/categories/{id} (`CategoryUpdate`).
#[derive(Debug, Clone, Default)]
pub struct CategoryUpdateInput {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub parent_id: Option<Option<Uuid>>,
}

/// Вхідні дані POST /api/v1/suppliers (`SupplierCreate`).
#[derive(Debug, Clone)]
pub struct SupplierCreateInput {
    pub name: String,
    pub edrpou: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
}

/// Вхідні дані PUT /api/v1/suppliers/{id} (`SupplierUpdate`).
#[derive(Debug, Clone, Default)]
pub struct SupplierUpdateInput {
    pub name: Option<String>,
    pub edrpou: Option<Option<String>>,
    pub phone: Option<Option<String>>,
    pub email: Option<Option<String>>,
    pub address: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}

/// Позиція інвентаризації (`InventoryItemCreate`).
///
/// Decimal-поля — рядки зі scale вхідного значення (відповідь Python
/// зберігає цю scale через identity map: `"18.5"` ≠ `"18.500"`).
#[derive(Debug, Clone)]
pub struct InventoryItemInput {
    pub product_id: Uuid,
    pub actual_quantity: String,
    pub accounting_quantity: String,
    pub difference: String,
    pub cost_price: String,
    pub price: String,
}

/// Вхідні дані POST /api/v1/inventory (`InventoryCreate`).
#[derive(Debug, Clone)]
pub struct InventoryCreateInput {
    pub number: Option<String>,
    pub location: Option<String>,
    pub inventory_date: NaiveDateTime,
    pub notes: Option<String>,
    pub items: Vec<InventoryItemInput>,
    /// `created_by_id` — sub з JWT (адміністратор).
    pub created_by: Uuid,
}

/// Вхідні дані PUT /api/v1/inventory/{id} (`InventoryUpdate`).
#[derive(Debug, Clone, Default)]
pub struct InventoryUpdateInput {
    pub number: Option<Option<String>>,
    pub location: Option<Option<String>>,
    pub inventory_date: Option<NaiveDateTime>,
    pub notes: Option<Option<String>>,
    pub items: Option<Vec<InventoryItemInput>>,
}

/// Кількість інвентаризацій за статусами (GET /inventory/counts).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InventoryCountsDto {
    pub total: i64,
    pub draft: i64,
    pub confirmed: i64,
    pub cancelled: i64,
}

// ─── Розрахунок націнки (HALF_EVEN, як Python round(Decimal, 2)) ──────────

/// Парсить десяткове число в ціле з масштабом 2 (scale 2).
/// "142.7" → 14270, "16" → 1600, "-5.5" → -550. None — невалідний формат.
fn parse_scaled2(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };
    if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac_part.len() > 2 || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let int_val: i64 = int_part.parse().ok()?;
    let frac_val: i64 = match frac_part.len() {
        0 => 0,
        1 => frac_part.parse::<i64>().ok()? * 10,
        _ => frac_part.parse().ok()?,
    };
    let v = int_val * 100 + frac_val;
    Some(if neg { -v } else { v })
}

/// Обчислює націнку: `round((price - cost) / cost * 100, 2)` з ROUND_HALF_EVEN
/// (дефолтний decimal-контекст Python). Повертає рядок з 2 знаками після коми
/// або `None`, якщо cost = 0 або формат невалідний.
///
/// Python: `round((Decimal(price) - Decimal(cost)) / Decimal(cost) * 100, 2)`.
pub fn calc_markup(price: &str, cost: &str) -> Option<String> {
    let p = parse_scaled2(price)?;
    let c = parse_scaled2(cost)?;
    if c == 0 {
        return None;
    }
    // markup*100 = (p - c) * 10000 / c — раціональне число, round до цілого
    // (markup = (p-c)/c*100; щоб отримати ціле markup*100, множимо на 10000).
    let num = (p - c) * 10000;
    let (sign, num) = if num < 0 { (-1i8, -num) } else { (1i8, num) };
    let q = num / c;
    let rem = num % c;
    // ROUND_HALF_EVEN: 2*rem == c → до парного.
    let rounded = match (rem * 2).cmp(&c) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => {
            if q % 2 == 0 {
                q
            } else {
                q + 1
            }
        }
    };
    let sign = if sign < 0 { "-" } else { "" };
    Some(format!("{sign}{}.{:02}", rounded / 100, rounded % 100))
}

#[cfg(test)]
mod tests {
    use super::calc_markup;

    #[test]
    fn markup_basic() {
        assert_eq!(calc_markup("142.7", "87.23").as_deref(), Some("63.59"));
        assert_eq!(calc_markup("40.05", "40.00").as_deref(), Some("0.12"));
        assert_eq!(calc_markup("3.00", "2.00").as_deref(), Some("50.00"));
        assert_eq!(calc_markup("16", "8").as_deref(), Some("100.00"));
    }

    #[test]
    fn markup_half_even() {
        // 0.125% → Python round(Decimal('0.125'), 2) = 0.12 (HALF_EVEN).
        assert_eq!(calc_markup("40.05", "40.00").as_deref(), Some("0.12"));
        // 0.135% → round = 0.14 (HALF_EVEN: 13 непарне → 14).
        assert_eq!(calc_markup("200.27", "200.00").as_deref(), Some("0.14"));
    }

    #[test]
    fn markup_negative_and_zero() {
        assert_eq!(calc_markup("10.00", "20.00").as_deref(), Some("-50.00"));
        assert_eq!(calc_markup("5", "0"), None);
    }
}
