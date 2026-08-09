//! Write-репозиторії (етап 2 — CRUD довідників + інвентаризація).
//!
//! Реалізують [`WriteDirectories`] на sqlx/PostgreSQL. Транзакції — `BEGIN`
//! у кожному write-методі (як Python-сесія FastAPI: flush → commit на кінець).
//! Конкурентність: `SELECT ... FOR UPDATE` на рядку продукту під час
//! інвентаризації — два паралельні confirm серіалізуються, нуль втрат.
//!
//! Timestamps: `(now() AT TIME ZONE 'UTC')::timestamp` — сервер БД у
//! Europe/Kyiv, а Python-еталон пише `datetime.utcnow()` (UTC). Без явного
//! UTC Rust-записи розходилися б з Python на +3 години.
//!
//! Відповіді POST/PUT зберігають ВХІДНУ scale Decimal (Python identity map):
//! `"142.7"` ≠ `"142.70"`. GET/confirm читають scale колонки (`::text`).

use chrono::NaiveDateTime;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use kasa_domain::{
    calc_markup, CategoryCreateInput, CategoryDto, CategoryUpdateInput, InventoryCountsDto,
    InventoryCreateInput, InventoryDto, InventoryItemDto, InventoryItemInput, InventorySummaryDto,
    InventoryUpdateInput, Page, ProductBriefDto, ProductCreateInput, ProductDto,
    ProductUpdateInput, SupplierCreateInput, SupplierDto, SupplierUpdateInput, WriteDirectories,
    WriteError,
};

/// Локальний екстеншен: sqlx::Error → WriteError (без sqlx-залежності в domain).
trait SqlxResultExt<T> {
    fn wr(self) -> Result<T, WriteError>;
}

impl<T> SqlxResultExt<T> for Result<T, sqlx::Error> {
    fn wr(self) -> Result<T, WriteError> {
        self.map_err(|e| WriteError::Infrastructure(e.to_string()))
    }
}

/// SQL-реалізація write-операцій.
#[derive(Clone)]
pub struct SqlxWriteDirectories {
    pool: PgPool,
}

impl SqlxWriteDirectories {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Рядок значення для bind: None → NULL, Some(s) → s.
fn opt_str(s: &Option<String>) -> Option<&str> {
    s.as_deref()
}

// ─── Проміжні структури ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ProductRow {
    id: Uuid,
    barcode: Option<String>,
    sku: Option<String>,
    title: String,
    description: Option<String>,
    price: Option<String>,
    cost_price: Option<String>,
    markup: Option<String>,
    stock: Option<String>,
    recommended_qty: Option<String>,
    uktzed: Option<String>,
    scan_excise: bool,
    tax_rate: Option<String>,
    tax_group: Option<String>,
    is_weight: bool,
    unit: Option<String>,
    category_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

#[derive(Debug, Clone)]
struct InventoryRow {
    id: Uuid,
    number: String,
    location: String,
    inventory_date: NaiveDateTime,
    status: String,
    notes: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

// ─── Products ───────────────────────────────────────────────────────────────

/// Конвертує ProductRow у ProductDto (GET-формат, scale з БД).
fn product_dto_from_row(r: ProductRow) -> ProductDto {
    ProductDto {
        id: r.id,
        barcode: r.barcode,
        sku: r.sku,
        title: r.title,
        description: r.description,
        price: r.price,
        cost_price: r.cost_price,
        markup: r.markup,
        stock: r.stock,
        recommended_qty: r.recommended_qty,
        uktzed: r.uktzed,
        scan_excise: r.scan_excise,
        tax_rate: r.tax_rate,
        tax_group: r.tax_group,
        is_weight: r.is_weight,
        unit: r.unit,
        category_id: r.category_id,
        supplier_id: r.supplier_id,
        images: Vec::new(),
        barcodes: Vec::new(),
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[async_trait::async_trait]
impl WriteDirectories for SqlxWriteDirectories {
    // ─── Products ───────────────────────────────────────────────────────────
    async fn create_product(&self, input: &ProductCreateInput) -> Result<ProductDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;

        // 409: унікальність штрих-коду.
        if let Some(barcode) = input.barcode.as_deref() {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE barcode = $1)")
                    .bind(barcode)
                    .fetch_one(&mut *tx)
                    .await
                    .wr()?;
            if exists {
                return Err(WriteError::Conflict(format!(
                    "Товар зі штрих-кодом '{barcode}' вже існує"
                )));
            }
        }
        // 409: унікальність артикулу.
        if let Some(sku) = input.sku.as_deref() {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE sku = $1)")
                    .bind(sku)
                    .fetch_one(&mut *tx)
                    .await
                    .wr()?;
            if exists {
                return Err(WriteError::Conflict(format!(
                    "Товар з артикулом '{sku}' вже існує"
                )));
            }
        }

        // Націнка: авто-розрахунок, якщо не задана і можлива (Python round HALF_EVEN).
        let markup = match &input.markup {
            Some(m) => Some(m.clone()),
            None => match (&input.cost_price, &input.price) {
                (Some(cost), Some(price)) => calc_markup(price, cost),
                _ => None,
            },
        };

        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO products \
             (id, barcode, sku, title, description, price, cost_price, markup, stock, \
              recommended_qty, uktzed, scan_excise, tax_rate, tax_group, is_weight, unit, \
              category_id, supplier_id, created_at, updated_at) \
             VALUES ($1, $2::varchar, $3::varchar, $4, $5, $6::numeric, $7::numeric, \
                     $8::numeric, $9::numeric, $10::numeric, $11::varchar, $12, $13::numeric, \
                     $14::varchar, $15, $16::varchar, $17, $18, \
                     (now() AT TIME ZONE 'UTC')::timestamp, \
                     (now() AT TIME ZONE 'UTC')::timestamp) \
             RETURNING id, created_at, updated_at",
        )
        .bind(id)
        .bind(input.barcode.as_deref())
        .bind(input.sku.as_deref())
        .bind(&input.title)
        .bind(input.description.as_deref())
        .bind(opt_str(&input.price))
        .bind(opt_str(&input.cost_price))
        .bind(markup.as_deref())
        .bind(input.stock.as_deref())
        .bind(opt_str(&input.recommended_qty))
        .bind(input.uktzed.as_deref())
        .bind(input.scan_excise)
        .bind(opt_str(&input.tax_rate))
        .bind(input.tax_group.as_deref())
        .bind(input.is_weight)
        .bind(input.unit.as_deref())
        .bind(input.category_id)
        .bind(input.supplier_id)
        .fetch_one(&mut *tx)
        .await
        .wr()?;
        let created_at: NaiveDateTime = row.get(1);
        let updated_at: NaiveDateTime = row.get(2);

        tx.commit().await.wr()?;

        // Відповідь POST: ВХІДНА scale Decimal (Python identity map) + дефолти.
        Ok(ProductDto {
            id,
            barcode: input.barcode.clone(),
            sku: input.sku.clone(),
            title: input.title.clone(),
            description: input.description.clone(),
            price: input.price.clone(),
            cost_price: input.cost_price.clone(),
            markup,
            stock: Some(input.stock.clone().unwrap_or_else(|| "0.000".to_string())),
            recommended_qty: input.recommended_qty.clone(),
            uktzed: input.uktzed.clone(),
            scan_excise: input.scan_excise,
            tax_rate: Some(input.tax_rate.clone().unwrap_or_else(|| "0.00".to_string())),
            tax_group: Some(input.tax_group.clone().unwrap_or_else(|| "А".to_string())),
            is_weight: input.is_weight,
            unit: Some(input.unit.clone().unwrap_or_else(|| "шт".to_string())),
            category_id: input.category_id,
            supplier_id: input.supplier_id,
            images: Vec::new(),
            barcodes: Vec::new(),
            created_at,
            updated_at,
        })
    }

    async fn update_product(
        &self,
        id: Uuid,
        input: &ProductUpdateInput,
    ) -> Result<ProductDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;

        // Блокуємо рядок продукту (FOR UPDATE) — унікальність + поточні значення.
        let row = sqlx::query(
            "SELECT id, barcode, sku, title, description, price::text, cost_price::text, \
             markup::text, stock::text, recommended_qty::text, uktzed, scan_excise, \
             tax_rate::text, tax_group, is_weight, unit, category_id, supplier_id, \
             created_at, updated_at FROM products WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .wr()?;
        let Some(row) = row else {
            return Err(WriteError::NotFound(format!(
                "Товар з ID '{id}' не знайдено"
            )));
        };
        let mut cur = ProductRow {
            id: row.get(0),
            barcode: row.get(1),
            sku: row.get(2),
            title: row.get(3),
            description: row.get(4),
            price: row.get(5),
            cost_price: row.get(6),
            markup: row.get(7),
            stock: row.get(8),
            recommended_qty: row.get(9),
            uktzed: row.get(10),
            scan_excise: row.get(11),
            tax_rate: row.get(12),
            tax_group: row.get(13),
            is_weight: row.get(14),
            unit: row.get(15),
            category_id: row.get(16),
            supplier_id: row.get(17),
            created_at: row.get(18),
            updated_at: row.get(19),
        };

        // 409: унікальність штрих-коду (якщо змінюється і не порожній).
        if let Some(Some(barcode)) = &input.barcode {
            if cur.barcode.as_deref() != Some(barcode.as_str()) {
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM products WHERE barcode = $1 AND id != $2)",
                )
                .bind(barcode)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .wr()?;
                if exists {
                    return Err(WriteError::Conflict(format!(
                        "Товар зі штрих-кодом '{barcode}' вже існує"
                    )));
                }
            }
        }
        // 409: унікальність артикулу.
        if let Some(Some(sku)) = &input.sku {
            if cur.sku.as_deref() != Some(sku.as_str()) {
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM products WHERE sku = $1 AND id != $2)",
                )
                .bind(sku)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .wr()?;
                if exists {
                    return Err(WriteError::Conflict(format!(
                        "Товар з артикулом '{sku}' вже існує"
                    )));
                }
            }
        }

        // Нова націнка: якщо markup НЕ передано — авто-розрахунок з (нових) цін.
        let new_markup: Option<String> = match &input.markup {
            Some(m) => m.clone(),
            None => {
                let cost = match &input.cost_price {
                    Some(Some(c)) => Some(c.clone()),
                    Some(None) => None,
                    None => cur.cost_price.clone(),
                };
                let price = match &input.price {
                    Some(Some(p)) => Some(p.clone()),
                    Some(None) => None,
                    None => cur.price.clone(),
                };
                match (cost.as_deref(), price.as_deref()) {
                    (Some(c), Some(p)) => calc_markup(p, c),
                    _ => cur.markup.clone(),
                }
            }
        };

        sqlx::query(
            "UPDATE products SET \
             barcode = COALESCE($2, barcode), sku = COALESCE($3, sku), \
             title = COALESCE($4, title), description = $5, \
             price = COALESCE($6::numeric, price), cost_price = COALESCE($7::numeric, cost_price), \
             markup = COALESCE($8::numeric, markup), stock = COALESCE($9::numeric, stock), \
             recommended_qty = COALESCE($10::numeric, recommended_qty), uktzed = COALESCE($11, uktzed), \
             scan_excise = COALESCE($12, scan_excise), tax_rate = COALESCE($13::numeric, tax_rate), \
             tax_group = COALESCE($14, tax_group), is_weight = COALESCE($15, is_weight), \
             unit = COALESCE($16, unit), category_id = $17, supplier_id = $18, \
             updated_at = (now() AT TIME ZONE 'UTC')::timestamp \
             WHERE id = $1",
        )
        .bind(id)
        .bind(input.barcode.clone().flatten().as_deref())
        .bind(input.sku.clone().flatten().as_deref())
        .bind(input.title.as_deref())
        .bind(input.description.clone().flatten().as_deref())
        .bind(input.price.clone().flatten().as_deref())
        .bind(input.cost_price.clone().flatten().as_deref())
        .bind(new_markup.as_deref())
        .bind(input.stock.clone().flatten().as_deref())
        .bind(input.recommended_qty.clone().flatten().as_deref())
        .bind(input.uktzed.clone().flatten().as_deref())
        .bind(input.scan_excise)
        .bind(input.tax_rate.clone().flatten().as_deref())
        .bind(input.tax_group.clone().flatten().as_deref())
        .bind(input.is_weight)
        .bind(input.unit.clone().flatten().as_deref())
        .bind(input.category_id.flatten())
        .bind(input.supplier_id.flatten())
        .execute(&mut *tx)
        .await.wr()?;

        tx.commit().await.wr()?;

        // Відповідь PUT: змінені поля = ВХІДНІ рядки, незмінені = БД scale.
        let title = input.title.clone().unwrap_or(cur.title);
        if let Some(v) = &input.barcode {
            cur.barcode = v.clone();
        }
        if let Some(v) = &input.sku {
            cur.sku = v.clone();
        }
        if let Some(v) = &input.description {
            cur.description = v.clone();
        }
        if let Some(v) = &input.price {
            cur.price = v.clone();
        }
        if let Some(v) = &input.cost_price {
            cur.cost_price = v.clone();
        }
        cur.markup = new_markup;
        if let Some(v) = &input.stock {
            cur.stock = v.clone();
        }
        if let Some(v) = &input.recommended_qty {
            cur.recommended_qty = v.clone();
        }
        if let Some(v) = &input.uktzed {
            cur.uktzed = v.clone();
        }
        if let Some(v) = &input.tax_rate {
            cur.tax_rate = v.clone();
        }
        if let Some(v) = &input.tax_group {
            cur.tax_group = v.clone();
        }
        if let Some(v) = &input.unit {
            cur.unit = v.clone();
        }
        if let Some(v) = &input.category_id {
            cur.category_id = *v;
        }
        if let Some(v) = &input.supplier_id {
            cur.supplier_id = *v;
        }
        cur.title = title;
        if let Some(v) = input.scan_excise {
            cur.scan_excise = v;
        }
        if let Some(v) = input.is_weight {
            cur.is_weight = v;
        }

        Ok(product_dto_from_row(cur))
    }

    async fn delete_product(&self, id: Uuid) -> Result<(), WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let row = sqlx::query("SELECT title, stock::text FROM products WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .wr()?;
        let Some(row) = row else {
            return Err(WriteError::NotFound(format!(
                "Товар з ID '{id}' не знайдено"
            )));
        };
        let title: String = row.get(0);
        let stock: Option<String> = row.get(1);

        // 400: товар з ненульовим залишком видалити не можна (як Python).
        let stock_nonzero = stock
            .as_deref()
            .and_then(|s| s.parse::<sqlx::types::Decimal>().ok())
            .map(|d| d != sqlx::types::Decimal::ZERO)
            .unwrap_or(false);
        if stock_nonzero {
            let stock_str = stock.unwrap_or_default();
            return Err(WriteError::BadRequest(format!(
                "Неможливо видалити товар '{title}': залишок на складі {stock_str} шт. \
                 Спочатку списати залишок до нуля."
            )));
        }

        sqlx::query("DELETE FROM products WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .wr()?;
        tx.commit().await.wr()?;
        Ok(())
    }

    // ─── Categories ─────────────────────────────────────────────────────────
    async fn create_category(
        &self,
        input: &CategoryCreateInput,
    ) -> Result<CategoryDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        if let Some(pid) = input.parent_id {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM categories WHERE id = $1)")
                    .bind(pid)
                    .fetch_one(&mut *tx)
                    .await
                    .wr()?;
            if !exists {
                return Err(WriteError::NotFound(format!(
                    "Батьківську категорію з ID '{pid}' не знайдено"
                )));
            }
        }
        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO categories (id, name, description, parent_id, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, (now() AT TIME ZONE 'UTC')::timestamp, \
                     (now() AT TIME ZONE 'UTC')::timestamp) \
             RETURNING id, created_at, updated_at",
        )
        .bind(id)
        .bind(&input.name)
        .bind(input.description.as_deref())
        .bind(input.parent_id)
        .fetch_one(&mut *tx)
        .await
        .wr()?;
        let created_at: NaiveDateTime = row.get(1);
        let updated_at: NaiveDateTime = row.get(2);
        tx.commit().await.wr()?;
        Ok(CategoryDto {
            id,
            name: input.name.clone(),
            description: input.description.clone(),
            parent_id: input.parent_id,
            created_at,
            updated_at,
        })
    }

    async fn update_category(
        &self,
        id: Uuid,
        input: &CategoryUpdateInput,
    ) -> Result<CategoryDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let row = sqlx::query(
            "SELECT id, name, description, parent_id, created_at, updated_at \
             FROM categories WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .wr()?;
        let Some(row) = row else {
            return Err(WriteError::NotFound(format!(
                "Категорію з ID '{id}' не знайдено"
            )));
        };
        let mut name: String = row.get(1);
        let mut description: Option<String> = row.get(2);
        let mut parent_id: Option<Uuid> = row.get(3);
        let created_at: NaiveDateTime = row.get(4);

        // Перевірка батьківської категорії (якщо змінюється).
        if let Some(new_parent) = input.parent_id {
            if new_parent != parent_id {
                if new_parent == Some(id) {
                    return Err(WriteError::BadRequest(
                        "Категорія не може бути власною батьківською категорією".to_string(),
                    ));
                }
                if let Some(pid) = new_parent {
                    let exists: bool =
                        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM categories WHERE id = $1)")
                            .bind(pid)
                            .fetch_one(&mut *tx)
                            .await
                            .wr()?;
                    if !exists {
                        return Err(WriteError::NotFound(format!(
                            "Батьківську категорію з ID '{pid}' не знайдено"
                        )));
                    }
                }
                parent_id = new_parent;
            }
        }
        if let Some(n) = &input.name {
            name = n.clone();
        }
        if let Some(d) = &input.description {
            description = d.clone();
        }

        sqlx::query(
            "UPDATE categories SET name = $2, description = $3, parent_id = $4, \
             updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1",
        )
        .bind(id)
        .bind(&name)
        .bind(description.as_deref())
        .bind(parent_id)
        .execute(&mut *tx)
        .await
        .wr()?;
        let updated_at: NaiveDateTime =
            sqlx::query_scalar("SELECT updated_at FROM categories WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .wr()?;
        tx.commit().await.wr()?;
        Ok(CategoryDto {
            id,
            name,
            description,
            parent_id,
            created_at,
            updated_at,
        })
    }

    async fn delete_category(&self, id: Uuid) -> Result<(), WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM categories WHERE id = $1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .wr()?;
        if !exists {
            return Err(WriteError::NotFound(format!(
                "Категорію з ID '{id}' не знайдено"
            )));
        }
        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .wr()?;
        tx.commit().await.wr()?;
        Ok(())
    }

    async fn category_name_exists(
        &self,
        name: &str,
        exclude_id: Option<Uuid>,
    ) -> Result<bool, WriteError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM categories
                WHERE name = $1 AND ($2::uuid IS NULL OR id <> $2)
             )",
        )
        .bind(name)
        .bind(exclude_id)
        .fetch_one(&self.pool)
        .await
        .wr()?;
        Ok(exists)
    }

    // ─── Suppliers ──────────────────────────────────────────────────────────
    async fn create_supplier(
        &self,
        input: &SupplierCreateInput,
    ) -> Result<SupplierDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO suppliers (id, name, edrpou, phone, email, address, notes, \
             created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, (now() AT TIME ZONE 'UTC')::timestamp, \
                     (now() AT TIME ZONE 'UTC')::timestamp) \
             RETURNING id, created_at, updated_at",
        )
        .bind(id)
        .bind(&input.name)
        .bind(input.edrpou.as_deref())
        .bind(input.phone.as_deref())
        .bind(input.email.as_deref())
        .bind(input.address.as_deref())
        .bind(input.notes.as_deref())
        .fetch_one(&mut *tx)
        .await
        .wr()?;
        let created_at: NaiveDateTime = row.get(1);
        let updated_at: NaiveDateTime = row.get(2);
        tx.commit().await.wr()?;
        Ok(SupplierDto {
            id,
            name: input.name.clone(),
            edrpou: input.edrpou.clone(),
            phone: input.phone.clone(),
            email: input.email.clone(),
            address: input.address.clone(),
            notes: input.notes.clone(),
            // Python: `Decimal(str(scalar or "0.00"))` → "0.00" scale 2.
            current_balance: "0.00".to_string(),
            created_at,
            updated_at,
        })
    }

    async fn update_supplier(
        &self,
        id: Uuid,
        input: &SupplierUpdateInput,
    ) -> Result<SupplierDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let row = sqlx::query(
            "SELECT id, name, edrpou, phone, email, address, notes, created_at, updated_at \
             FROM suppliers WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .wr()?;
        let Some(row) = row else {
            return Err(WriteError::NotFound(format!(
                "Постачальника з ID '{id}' не знайдено"
            )));
        };
        let mut name: String = row.get(1);
        let mut edrpou: Option<String> = row.get(2);
        let mut phone: Option<String> = row.get(3);
        let mut email: Option<String> = row.get(4);
        let mut address: Option<String> = row.get(5);
        let mut notes: Option<String> = row.get(6);
        let created_at: NaiveDateTime = row.get(7);

        if let Some(n) = &input.name {
            name = n.clone();
        }
        if let Some(v) = &input.edrpou {
            edrpou = v.clone();
        }
        if let Some(v) = &input.phone {
            phone = v.clone();
        }
        if let Some(v) = &input.email {
            email = v.clone();
        }
        if let Some(v) = &input.address {
            address = v.clone();
        }
        if let Some(v) = &input.notes {
            notes = v.clone();
        }

        sqlx::query(
            "UPDATE suppliers SET name = $2, edrpou = $3, phone = $4, email = $5, \
             address = $6, notes = $7, updated_at = (now() AT TIME ZONE 'UTC')::timestamp \
             WHERE id = $1",
        )
        .bind(id)
        .bind(&name)
        .bind(edrpou.as_deref())
        .bind(phone.as_deref())
        .bind(email.as_deref())
        .bind(address.as_deref())
        .bind(notes.as_deref())
        .execute(&mut *tx)
        .await
        .wr()?;
        let updated_at: NaiveDateTime =
            sqlx::query_scalar("SELECT updated_at FROM suppliers WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .wr()?;
        // Поточний баланс з БД (як Python `_get_supplier_balance`).
        let balance: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE((SELECT SUM(amount) FROM supplier_ledger \
             WHERE supplier_id = $1), 0)::numeric(12,2)::text",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .wr()?;
        tx.commit().await.wr()?;
        Ok(SupplierDto {
            id,
            name,
            edrpou,
            phone,
            email,
            address,
            notes,
            current_balance: balance.unwrap_or_else(|| "0.00".to_string()),
            created_at,
            updated_at,
        })
    }

    async fn delete_supplier(&self, id: Uuid) -> Result<(), WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM suppliers WHERE id = $1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .wr()?;
        if !exists {
            return Err(WriteError::NotFound(format!(
                "Постачальника з ID '{id}' не знайдено"
            )));
        }
        sqlx::query("DELETE FROM suppliers WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .wr()?;
        tx.commit().await.wr()?;
        Ok(())
    }

    // ─── Inventory ──────────────────────────────────────────────────────────
    async fn list_inventories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<InventoryDto>, WriteError> {
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM inventories")
            .fetch_one(&self.pool)
            .await
            .wr()?;
        let offset = (page - 1).max(0) * size;
        let rows = sqlx::query(
            "SELECT id, number, location, inventory_date, status::text, notes, created_at, \
             updated_at FROM inventories ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .wr()?;
        let mut items: Vec<InventoryDto> = Vec::with_capacity(rows.len());
        for r in rows {
            let inv = InventoryRow {
                id: r.get(0),
                number: r.get(1),
                location: r.get(2),
                inventory_date: r.get(3),
                status: r.get(4),
                notes: r.get(5),
                created_at: r.get(6),
                updated_at: r.get(7),
            };
            items.push(self.load_inventory_details(inv).await?);
        }
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        Ok(Page {
            items,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn inventory_counts(&self) -> Result<InventoryCountsDto, WriteError> {
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM inventories")
            .fetch_one(&self.pool)
            .await
            .wr()?;
        let draft: i64 =
            sqlx::query_scalar("SELECT count(*) FROM inventories WHERE status = 'draft'")
                .fetch_one(&self.pool)
                .await
                .wr()?;
        let confirmed: i64 =
            sqlx::query_scalar("SELECT count(*) FROM inventories WHERE status = 'confirmed'")
                .fetch_one(&self.pool)
                .await
                .wr()?;
        let cancelled: i64 =
            sqlx::query_scalar("SELECT count(*) FROM inventories WHERE status = 'cancelled'")
                .fetch_one(&self.pool)
                .await
                .wr()?;
        Ok(InventoryCountsDto {
            total,
            draft,
            confirmed,
            cancelled,
        })
    }

    async fn get_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError> {
        let row = sqlx::query(
            "SELECT id, number, location, inventory_date, status::text, notes, created_at, \
             updated_at FROM inventories WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .wr()?;
        let Some(r) = row else {
            return Err(WriteError::NotFound(format!(
                "Інвентаризацію з ID '{id}' не знайдено"
            )));
        };
        let inv = InventoryRow {
            id: r.get(0),
            number: r.get(1),
            location: r.get(2),
            inventory_date: r.get(3),
            status: r.get(4),
            notes: r.get(5),
            created_at: r.get(6),
            updated_at: r.get(7),
        };
        self.load_inventory_details(inv).await
    }

    async fn create_inventory(
        &self,
        input: &InventoryCreateInput,
    ) -> Result<InventoryDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;

        // Номер: вхідний або авто-генерація ІН-{YYYYMMDD}-{XXX} (UTC, як Python).
        let number = match &input.number {
            Some(n) if !n.trim().is_empty() => n.clone(),
            _ => {
                let today = chrono::Utc::now().format("%Y%m%d").to_string();
                let prefix = format!("ІН-{today}-");
                let max_num: Option<String> =
                    sqlx::query_scalar("SELECT max(number) FROM inventories WHERE number LIKE $1")
                        .bind(format!("{prefix}%"))
                        .fetch_one(&mut *tx)
                        .await
                        .wr()?;
                let last_seq = max_num
                    .as_deref()
                    .and_then(|m| {
                        m.chars()
                            .rev()
                            .take(3)
                            .collect::<String>()
                            .parse::<i64>()
                            .ok()
                    })
                    .unwrap_or(0);
                format!("{prefix}{:03}", last_seq + 1)
            }
        };

        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO inventories (id, number, location, inventory_date, status, notes, \
             created_at, updated_at, created_by_id) \
             VALUES ($1, $2, $3, $4, 'draft', $5, (now() AT TIME ZONE 'UTC')::timestamp, \
                     (now() AT TIME ZONE 'UTC')::timestamp, $6) \
             RETURNING created_at, updated_at",
        )
        .bind(id)
        .bind(&number)
        .bind(input.location.as_deref().unwrap_or(""))
        .bind(input.inventory_date)
        .bind(input.notes.as_deref())
        .bind(input.created_by)
        .fetch_one(&mut *tx)
        .await
        .wr()?;
        let created_at: NaiveDateTime = row.get(0);
        let updated_at: NaiveDateTime = row.get(1);

        let mut item_ids: Vec<(Uuid, Uuid, NaiveDateTime)> = Vec::new();
        for item in &input.items {
            let iid = Uuid::new_v4();
            let irow = sqlx::query(
                "INSERT INTO inventory_items (id, inventory_id, product_id, actual_quantity, \
                 accounting_quantity, difference, cost_price, price, created_at) \
                 VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7::numeric, \
                         $8::numeric, (now() AT TIME ZONE 'UTC')::timestamp) \
                 RETURNING created_at",
            )
            .bind(iid)
            .bind(id)
            .bind(item.product_id)
            .bind(&item.actual_quantity)
            .bind(&item.accounting_quantity)
            .bind(&item.difference)
            .bind(&item.cost_price)
            .bind(&item.price)
            .fetch_one(&mut *tx)
            .await
            .wr()?;
            item_ids.push((iid, item.product_id, irow.get(0)));
        }
        tx.commit().await.wr()?;

        // Відповідь POST: quantity/price — ВХІДНІ рядки (Python identity map),
        // product title/barcode — з БД.
        let products = self.fetch_products_brief(&input.items).await?;
        let items = input
            .items
            .iter()
            .zip(item_ids.iter())
            .map(|(item, (iid, _pid, created))| InventoryItemDto {
                id: *iid,
                inventory_id: id,
                product_id: item.product_id,
                product: products.get(&item.product_id).cloned(),
                actual_quantity: item.actual_quantity.clone(),
                accounting_quantity: item.accounting_quantity.clone(),
                difference: item.difference.clone(),
                cost_price: item.cost_price.clone(),
                price: item.price.clone(),
                total_cost: 0,
                total_selling: 0,
                created_at: *created,
            })
            .collect();

        Ok(InventoryDto {
            id,
            number,
            location: input.location.clone().unwrap_or_default(),
            inventory_date: input.inventory_date,
            status: "draft".to_string(),
            notes: input.notes.clone(),
            created_at,
            updated_at,
            items,
            summary: Self::summary_from_inputs(&input.items),
        })
    }

    async fn update_inventory(
        &self,
        id: Uuid,
        input: &InventoryUpdateInput,
    ) -> Result<InventoryDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let row = sqlx::query(
            "SELECT id, number, location, inventory_date, status::text, notes, created_at, \
             updated_at FROM inventories WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .wr()?;
        let Some(r) = row else {
            return Err(WriteError::NotFound(format!(
                "Інвентаризацію з ID '{id}' не знайдено"
            )));
        };
        let status: String = r.get(4);
        if status != "draft" {
            return Err(WriteError::BadRequest(
                "Можна редагувати тільки чернетки".to_string(),
            ));
        }
        let mut number: String = r.get(1);
        let mut location: String = r.get(2);
        let mut inventory_date: NaiveDateTime = r.get(3);
        let mut notes: Option<String> = r.get(5);
        let created_at: NaiveDateTime = r.get(6);

        if let Some(v) = &input.number {
            number = v.clone().unwrap_or_default();
        }
        if let Some(v) = &input.location {
            location = v.clone().unwrap_or_default();
        }
        if let Some(d) = &input.inventory_date {
            inventory_date = *d;
        }
        if let Some(v) = &input.notes {
            notes = v.clone();
        }

        sqlx::query(
            "UPDATE inventories SET number = $2, location = $3, inventory_date = $4, \
             notes = $5, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1",
        )
        .bind(id)
        .bind(&number)
        .bind(&location)
        .bind(inventory_date)
        .bind(notes.as_deref())
        .execute(&mut *tx)
        .await
        .wr()?;

        // items: Some → видалити старі, вставити нові (як Python).
        if let Some(new_items) = &input.items {
            sqlx::query("DELETE FROM inventory_items WHERE inventory_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .wr()?;
            for item in new_items {
                sqlx::query(
                    "INSERT INTO inventory_items (id, inventory_id, product_id, actual_quantity, \
                     accounting_quantity, difference, cost_price, price, created_at) \
                     VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7::numeric, \
                             $8::numeric, (now() AT TIME ZONE 'UTC')::timestamp)",
                )
                .bind(Uuid::new_v4())
                .bind(id)
                .bind(item.product_id)
                .bind(&item.actual_quantity)
                .bind(&item.accounting_quantity)
                .bind(&item.difference)
                .bind(&item.cost_price)
                .bind(&item.price)
                .execute(&mut *tx)
                .await
                .wr()?;
            }
        }
        tx.commit().await.wr()?;

        let inv = InventoryRow {
            id,
            number: number.clone(),
            location: location.clone(),
            inventory_date,
            status: status.clone(),
            notes: notes.clone(),
            created_at,
            updated_at: chrono::Utc::now().naive_utc(),
        };
        match &input.items {
            // items замінено → відповідь з ВХІДНИМИ значеннями (як Python).
            Some(new_items) => {
                let products = self.fetch_products_brief(new_items).await?;
                let mut ids: Vec<Uuid> = Vec::new();
                let item_rows = sqlx::query(
                    "SELECT id, created_at FROM inventory_items \
                     WHERE inventory_id = $1 ORDER BY created_at, id",
                )
                .bind(id)
                .fetch_all(&self.pool)
                .await
                .wr()?;
                for ir in &item_rows {
                    ids.push(ir.get(0));
                }
                let items = new_items
                    .iter()
                    .zip(ids.iter())
                    .zip(item_rows.iter())
                    .map(|((item, iid), irow)| InventoryItemDto {
                        id: *iid,
                        inventory_id: id,
                        product_id: item.product_id,
                        product: products.get(&item.product_id).cloned(),
                        actual_quantity: item.actual_quantity.clone(),
                        accounting_quantity: item.accounting_quantity.clone(),
                        difference: item.difference.clone(),
                        cost_price: item.cost_price.clone(),
                        price: item.price.clone(),
                        total_cost: 0,
                        total_selling: 0,
                        created_at: irow.get(1),
                    })
                    .collect();
                Ok(InventoryDto {
                    id,
                    number,
                    location,
                    inventory_date,
                    status,
                    notes,
                    created_at,
                    updated_at: chrono::Utc::now().naive_utc(),
                    items,
                    summary: Self::summary_from_inputs(new_items),
                })
            }
            None => self.load_inventory_details(inv).await,
        }
    }

    async fn delete_inventory(&self, id: Uuid) -> Result<(), WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        let row = sqlx::query("SELECT status::text FROM inventories WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .wr()?;
        let Some(r) = row else {
            return Err(WriteError::NotFound(format!(
                "Інвентаризацію з ID '{id}' не знайдено"
            )));
        };
        let status: String = r.get(0);
        if status != "draft" {
            return Err(WriteError::BadRequest(
                "Можна видалити тільки чернетку".to_string(),
            ));
        }
        sqlx::query("DELETE FROM inventories WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .wr()?;
        tx.commit().await.wr()?;
        Ok(())
    }

    async fn confirm_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        self.check_inventory_status(&mut tx, id, "draft").await?;
        // Проведення: змінюємо залишки згідно з різницею (АТОМАРНО, FOR UPDATE).
        self.apply_differences(&mut tx, id, 1).await?;
        sqlx::query(
            "UPDATE inventories SET status = 'confirmed', \
             updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .wr()?;
        tx.commit().await.wr()?;
        self.get_inventory(id).await
    }

    async fn cancel_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError> {
        let mut tx = self.pool.begin().await.wr()?;
        self.check_inventory_status(&mut tx, id, "confirmed")
            .await?;
        // Відкат: зворотна операція до confirm.
        self.apply_differences(&mut tx, id, -1).await?;
        sqlx::query(
            "UPDATE inventories SET status = 'cancelled', \
             updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .wr()?;
        tx.commit().await.wr()?;
        self.get_inventory(id).await
    }
}

// ─── Допоміжні методи ──────────────────────────────────────────────────────

impl SqlxWriteDirectories {
    /// Перевіряє статус інвентаризації (FOR UPDATE) і повертає його.
    async fn check_inventory_status(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: Uuid,
        expected: &str,
    ) -> Result<String, WriteError> {
        let row = sqlx::query("SELECT status::text FROM inventories WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .wr()?;
        let Some(r) = row else {
            return Err(WriteError::NotFound(format!(
                "Інвентаризацію з ID '{id}' не знайдено"
            )));
        };
        let status: String = r.get(0);
        if status != expected {
            if expected == "draft" {
                return Err(WriteError::BadRequest(format!(
                    "Інвентаризація вже має статус '{status}'"
                )));
            }
            return Err(WriteError::BadRequest(
                "Скасувати можна лише підтверджену інвентаризацію".to_string(),
            ));
        }
        Ok(status)
    }

    /// Застосовує різниці до залишків товарів (confirm: +diff, cancel: -diff).
    ///
    /// Кожен рядок продукту блокується `FOR UPDATE` — паралельні проведення
    /// серіалізуються, залишок оновлюється атомарно (нуль втрат).
    async fn apply_differences(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        inventory_id: Uuid,
        sign: i32,
    ) -> Result<(), WriteError> {
        let items = sqlx::query(
            "SELECT product_id, difference::text FROM inventory_items \
             WHERE inventory_id = $1",
        )
        .bind(inventory_id)
        .fetch_all(&mut **tx)
        .await
        .wr()?;
        for item in items {
            let product_id: Uuid = item.get(0);
            let diff: String = item.get(1);
            let delta = match diff.parse::<sqlx::types::Decimal>() {
                Ok(d) => d * sqlx::types::Decimal::from(sign),
                Err(_) => sqlx::types::Decimal::ZERO,
            };

            // Блокуємо товар і перевіряємо достатність (як Python update_stock).
            let prow =
                sqlx::query("SELECT title, stock::text FROM products WHERE id = $1 FOR UPDATE")
                    .bind(product_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .wr()?;
            let Some(prow) = prow else {
                continue; // товар видалено — пропускаємо (Python: get_product_by_id → 404, але тут edge)
            };
            let title: String = prow.get(0);
            let stock: Option<String> = prow.get(1);
            let stock_dec = stock
                .as_deref()
                .and_then(|s| s.parse::<sqlx::types::Decimal>().ok());

            if delta < sqlx::types::Decimal::ZERO {
                if let Some(st) = stock_dec {
                    if st + delta < sqlx::types::Decimal::ZERO {
                        let needed = (-delta).to_string();
                        let avail = stock.unwrap_or_default();
                        return Err(WriteError::BadRequest(format!(
                            "Недостатньо товару '{title}' на складі. Доступно: {avail}, потрібно: {needed}"
                        )));
                    }
                }
            }

            sqlx::query(
                "UPDATE products SET stock = stock + $2::numeric, \
                 updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1",
            )
            .bind(product_id)
            .bind(delta.to_string())
            .execute(&mut **tx)
            .await
            .wr()?;
        }
        Ok(())
    }

    /// Завантажує items + summary для інвентаризації (GET-формат, scale БД).
    async fn load_inventory_details(&self, inv: InventoryRow) -> Result<InventoryDto, WriteError> {
        let item_rows = sqlx::query(
            "SELECT ii.id, ii.inventory_id, ii.product_id, p.title, p.barcode, \
             ii.actual_quantity::text, ii.accounting_quantity::text, ii.difference::text, \
             ii.cost_price::text, ii.price::text, ii.created_at \
             FROM inventory_items ii \
             LEFT JOIN products p ON p.id = ii.product_id \
             WHERE ii.inventory_id = $1 ORDER BY ii.created_at, ii.id",
        )
        .bind(inv.id)
        .fetch_all(&self.pool)
        .await
        .wr()?;
        let items = item_rows
            .into_iter()
            .map(|r| InventoryItemDto {
                id: r.get(0),
                inventory_id: r.get(1),
                product_id: r.get(2),
                product: Some(ProductBriefDto {
                    id: r.get(2),
                    title: r.get::<Option<String>, _>(3).unwrap_or_default(),
                    barcode: r.get(4),
                }),
                actual_quantity: r.get(5),
                accounting_quantity: r.get(6),
                difference: r.get(7),
                cost_price: r.get(8),
                price: r.get(9),
                total_cost: 0,
                total_selling: 0,
                created_at: r.get(10),
            })
            .collect();
        let summary = self.compute_summary(inv.id).await?;
        Ok(InventoryDto {
            id: inv.id,
            number: inv.number,
            location: inv.location,
            inventory_date: inv.inventory_date,
            status: inv.status,
            notes: inv.notes,
            created_at: inv.created_at,
            updated_at: inv.updated_at,
            items,
            summary,
        })
    }

    /// Підсумки з ВХІДНИХ значень (POST/PUT-відповідь): Python рахує зі
    /// scale вхідних Decimal (identity map), а не з БД-колонок.
    fn summary_from_inputs(items: &[InventoryItemInput]) -> InventorySummaryDto {
        use std::str::FromStr;
        let mut total_cost = sqlx::types::Decimal::ZERO;
        let mut total_selling = sqlx::types::Decimal::ZERO;
        let mut total_deviation = sqlx::types::Decimal::ZERO;
        for it in items {
            let actual = sqlx::types::Decimal::from_str(&it.actual_quantity).unwrap_or_default();
            let diff = sqlx::types::Decimal::from_str(&it.difference).unwrap_or_default();
            let cost = sqlx::types::Decimal::from_str(&it.cost_price).unwrap_or_default();
            let price = sqlx::types::Decimal::from_str(&it.price).unwrap_or_default();
            total_cost += actual * cost;
            total_selling += actual * price;
            total_deviation += diff * cost;
        }
        InventorySummaryDto {
            total_cost: total_cost.to_string(),
            total_selling: total_selling.to_string(),
            total_deviation: total_deviation.to_string(),
        }
    }

    /// Підсумки інвентаризації: суми зі scale добутку (як Python Decimal).
    async fn compute_summary(&self, inventory_id: Uuid) -> Result<InventorySummaryDto, WriteError> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(actual_quantity * cost_price), 0)::text, \
             COALESCE(SUM(actual_quantity * price), 0)::text, \
             COALESCE(SUM(difference * cost_price), 0)::text \
             FROM inventory_items WHERE inventory_id = $1",
        )
        .bind(inventory_id)
        .fetch_one(&self.pool)
        .await
        .wr()?;
        Ok(InventorySummaryDto {
            total_cost: row.get(0),
            total_selling: row.get(1),
            total_deviation: row.get(2),
        })
    }

    /// Коротка інформація про товари (для POST-відповіді інвентаризації).
    async fn fetch_products_brief(
        &self,
        items: &[InventoryItemInput],
    ) -> Result<std::collections::HashMap<Uuid, ProductBriefDto>, WriteError> {
        let mut map = std::collections::HashMap::new();
        if items.is_empty() {
            return Ok(map);
        }
        let ids: Vec<Uuid> = items.iter().map(|i| i.product_id).collect();
        let rows = sqlx::query("SELECT id, title, barcode FROM products WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(&self.pool)
            .await
            .wr()?;
        for r in rows {
            map.insert(
                r.get(0),
                ProductBriefDto {
                    id: r.get(0),
                    title: r.get(1),
                    barcode: r.get(2),
                },
            );
        }
        Ok(map)
    }
}

/// Перевіряє роль користувача в БД (Python `AuthService.require_admin`).
///
/// Повертає 403 `"Доступ заборонено: потрібна роль адміністратора"`,
/// якщо користувач не існує, деактивований або роль != admin.
pub async fn require_admin_role(pool: &PgPool, user_id: Uuid) -> Result<(), WriteError> {
    let row = sqlx::query("SELECT role::text, is_active FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| WriteError::Infrastructure(e.to_string()))?;
    let Some(r) = row else {
        return Err(WriteError::Forbidden(
            "Доступ заборонено: потрібна роль адміністратора".to_string(),
        ));
    };
    let role: String = r.get(0);
    let is_active: bool = r.get(1);
    if !is_active {
        return Err(WriteError::Forbidden(
            "Користувач деактивований".to_string(),
        ));
    }
    if role != "admin" {
        return Err(WriteError::Forbidden(
            "Доступ заборонено: потрібна роль адміністратора".to_string(),
        ));
    }
    Ok(())
}
