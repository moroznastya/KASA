//! SQL-репозиторій товарів v2 (етап 8 — група 7): зображення + штрих-коди.
//!
//! 1:1 з Python product_repository.py (SQLAlchemy) — семантика операцій:
//!   - list: пошук ILIKE по title/barcode/sku (БЕЗ додаткових barcodes —
//!     це v1-відмінність), фільтр category_id, пагінація offset/limit БЕЗ
//!     ORDER BY (як Python — порядок визначає PostgreSQL).
//!   - add_image: is_main=true → скидає прапорець з інших зображень товару,
//!     sort_order = поточна кількість зображень.
//!   - add_barcode: дублікат → 409 (Python ValueError «вже існує»);
//!     is_primary=true → скидає з інших штрих-кодів товару.
//!   - delete_product: stock != 0 → 400 «Неможливо видалити товар ...»
//!     (Python float-формат: "5.0", БЕЗ суфікса «Спочатку списати» v1).

use sqlx::{PgPool, Row};
use uuid::Uuid;

use torgashka_domain::{
    ProductCreateV2Input, ProductImageV2Dto, ProductListV2Dto, ProductUpdateV2Input, ProductV2Dto,
    ProductsV2Error, ProductsV2Service,
};

/// SQL-репозиторій товарів v2.
pub struct SqlxProductsV2 {
    pool: PgPool,
}

impl SqlxProductsV2 {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Перетворення text-числа numeric → f64 (Python Decimal → float).
fn to_f64(text: Option<String>) -> Option<f64> {
    text.as_deref().and_then(|s| s.parse::<f64>().ok())
}

/// f64 → sqlx Decimal (numeric колонка; Python Decimal(str(value))).
fn py_decimal(v: f64) -> sqlx::types::Decimal {
    v.to_string().parse().unwrap_or_default()
}

/// Python float → str: 5.0 → "5.0", 5.5 → "5.5" (для detail delete).
fn py_float_str(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

impl SqlxProductsV2 {
    /// SELECT повного рядка товару за id → опційний ProductV2Dto.
    async fn row_by_id(&self, id: Uuid) -> Result<Option<ProductV2Dto>, ProductsV2Error> {
        let row = sqlx::query(
            "SELECT p.id, p.barcode, p.sku, p.title, p.description,
                    p.price::text, p.cost_price::text, p.stock::text,
                    COALESCE(p.unit, 'шт') AS unit, p.category_id, p.supplier_id
             FROM products p WHERE p.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(|r| dto_from_row(&r)))
    }
}

#[async_trait::async_trait]
impl ProductsV2Service for SqlxProductsV2 {
    async fn list(
        &self,
        page: i64,
        size: i64,
        search: Option<&str>,
        category_id: Option<Uuid>,
    ) -> Result<ProductListV2Dto, ProductsV2Error> {
        // WHERE-умови (без barcodes-join — Python v2 search лише по products).
        // QueryBuilder: правильний WHERE/AND, bind Uuid як uuid (не text).
        let mut wb = sqlx::QueryBuilder::new("SELECT count(*) FROM products p");
        let mut first = true;
        if let Some(q) = search {
            let pattern = format!("%{q}%");
            wb.push(" WHERE (p.title ILIKE ").push_bind(pattern.clone());
            wb.push(" OR p.barcode ILIKE ").push_bind(pattern.clone());
            wb.push(" OR p.sku ILIKE ").push_bind(pattern);
            wb.push(")");
            first = false;
        }
        if let Some(cid) = category_id {
            if first {
                wb.push(" WHERE p.category_id = ").push_bind(cid);
            } else {
                wb.push(" AND p.category_id = ").push_bind(cid);
            }
        }

        // total: count окремим запитом (як Python count_stmt).
        let total: i64 = wb
            .build()
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)
            .map(|r| r.get::<i64, _>(0))?;

        // items: offset/limit без ORDER BY (як Python).
        let offset = (page - 1) * size;
        let mut ib = sqlx::QueryBuilder::new(
            "SELECT p.id, p.barcode, p.sku, p.title, p.description,
                    p.price::text, p.cost_price::text, p.stock::text,
                    COALESCE(p.unit, 'шт') AS unit, p.category_id, p.supplier_id
             FROM products p",
        );
        let mut first = true;
        if let Some(q) = search {
            let pattern = format!("%{q}%");
            ib.push(" WHERE (p.title ILIKE ").push_bind(pattern.clone());
            ib.push(" OR p.barcode ILIKE ").push_bind(pattern.clone());
            ib.push(" OR p.sku ILIKE ").push_bind(pattern);
            ib.push(")");
            first = false;
        }
        if let Some(cid) = category_id {
            if first {
                ib.push(" WHERE p.category_id = ").push_bind(cid);
            } else {
                ib.push(" AND p.category_id = ").push_bind(cid);
            }
        }
        ib.push(" OFFSET ")
            .push_bind(offset)
            .push(" LIMIT ")
            .push_bind(size);
        let rows = ib.build().fetch_all(&self.pool).await.map_err(db_err)?;
        let items = rows.iter().map(dto_from_row).collect();
        Ok(ProductListV2Dto {
            items,
            total,
            page,
            size,
        })
    }

    async fn get_by_barcode(&self, barcode: &str) -> Result<ProductV2Dto, ProductsV2Error> {
        // Спочатку основний штрих-код (products.barcode), потім додаткові.
        let row = sqlx::query(
            "SELECT p.id, p.barcode, p.sku, p.title, p.description,
                    p.price::text, p.cost_price::text, p.stock::text,
                    COALESCE(p.unit, 'шт') AS unit, p.category_id, p.supplier_id
             FROM products p WHERE p.barcode = $1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let row = match row {
            Some(r) => r,
            None => sqlx::query(
                "SELECT p.id, p.barcode, p.sku, p.title, p.description,
                        p.price::text, p.cost_price::text, p.stock::text,
                        COALESCE(p.unit, 'шт') AS unit, p.category_id, p.supplier_id
                 FROM products p JOIN barcodes b ON b.product_id = p.id
                 WHERE b.barcode = $1",
            )
            .bind(barcode)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?
            .ok_or_else(|| {
                ProductsV2Error::NotFound(format!("Товар зі штрих-кодом '{barcode}' не знайдено"))
            })?,
        };
        Ok(dto_from_row(&row))
    }

    async fn create(&self, input: &ProductCreateV2Input) -> Result<ProductV2Dto, ProductsV2Error> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;

        // 400: унікальність штрих-коду (Python ValueError → HTTP 400).
        if let Some(bc) = input.barcode.as_deref().filter(|s| !s.is_empty()) {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE barcode = $1)")
                    .bind(bc)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_err)?;
            if exists {
                return Err(ProductsV2Error::BadRequest(format!(
                    "Товар з штрих-кодом '{bc}' вже існує"
                )));
            }
        }
        // 400: унікальність артикулу.
        if let Some(sku) = input.sku.as_deref().filter(|s| !s.is_empty()) {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE sku = $1)")
                    .bind(sku)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_err)?;
            if exists {
                return Err(ProductsV2Error::BadRequest(format!(
                    "Товар з артикулом '{sku}' вже існує"
                )));
            }
        }

        let id = Uuid::new_v4();
        // quantity: Python `if dto.stock else None`; нуль → ORM пропускає колонку
        // → БД DEFAULT 0.000 (Python відповідь quantity 0.0, у БД 0.000 не NULL).
        let stock = input
            .quantity
            .filter(|q| *q != 0.0)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "0.000".into());
        // price/cost_price: None → ORM пропускає → БД DEFAULT 0.00 (Python 0.0).
        let price = input
            .price
            .map(|p| p.to_string())
            .unwrap_or_else(|| "0.00".into());
        let cost_price = input
            .cost_price
            .map(|p| p.to_string())
            .unwrap_or_else(|| "0.00".into());
        // sku/description: Python DTO default "" — не NULL.
        let sku = input.sku.clone().unwrap_or_default();
        let description = input.description.clone().unwrap_or_default();
        let title = input.name.clone().unwrap_or_default();

        sqlx::query(
            "INSERT INTO products \
             (id, barcode, sku, title, description, price, cost_price, stock, \
              unit, category_id, supplier_id, created_at, updated_at) \
             VALUES ($1, $2::varchar, $3::varchar, $4, $5, $6::numeric, $7::numeric, \
                     $8::numeric, $9::varchar, $10, $11, now(), now())",
        )
        .bind(id)
        .bind(input.barcode.as_deref().filter(|s| !s.is_empty()))
        .bind(&sku)
        .bind(&title)
        .bind(&description)
        .bind(&price)
        .bind(&cost_price)
        .bind(&stock)
        .bind(
            input
                .unit
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("шт"),
        )
        .bind(input.category_id)
        .bind(input.supplier_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;

        let dto = self
            .row_by_id(id)
            .await?
            .ok_or_else(|| ProductsV2Error::Infrastructure("створений товар не знайдено".into()))?;
        Ok(dto)
    }

    async fn update(
        &self,
        id: Uuid,
        input: &ProductUpdateV2Input,
    ) -> Result<ProductV2Dto, ProductsV2Error> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;

        // 404: товар існує.
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE id = $1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        if !exists {
            return Err(ProductsV2Error::NotFound(format!(
                "Товар з ID '{id}' не знайдено"
            )));
        }

        // 400: унікальність штрих-коду (якщо змінюється, exclude_id).
        if let Some(bc) = input.barcode.as_deref().filter(|s| !s.is_empty()) {
            let dup: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM products WHERE barcode = $1 AND id <> $2)",
            )
            .bind(bc)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
            if dup {
                return Err(ProductsV2Error::BadRequest(format!(
                    "Товар з штрих-кодом '{bc}' вже існує"
                )));
            }
        }
        // 400: унікальність артикулу (якщо змінюється, exclude_id).
        if let Some(sku) = input.sku.as_deref().filter(|s| !s.is_empty()) {
            let dup: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM products WHERE sku = $1 AND id <> $2)",
            )
            .bind(sku)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
            if dup {
                return Err(ProductsV2Error::BadRequest(format!(
                    "Товар з артикулом '{sku}' вже існує"
                )));
            }
        }

        // Динамічний UPDATE (exclude_unset-семантика: None = не оновлювати).
        let mut q = sqlx::QueryBuilder::new("UPDATE products SET ");
        let mut first = true;
        if let Some(name) = &input.name {
            if !first {
                q.push(", ");
            }
            q.push("title = ").push_bind(name);
            first = false;
        }
        // barcode: Python `dto.barcode or None` — "" → NULL.
        if let Some(bc) = &input.barcode {
            if !first {
                q.push(", ");
            }
            q.push("barcode = ").push_bind(bc.as_str());
            first = false;
        }
        if let Some(p) = input.price {
            if !first {
                q.push(", ");
            }
            q.push("price = ").push_bind(py_decimal(p));
            first = false;
        }
        if let Some(cp) = input.cost_price {
            if !first {
                q.push(", ");
            }
            q.push("cost_price = ").push_bind(py_decimal(cp));
            first = false;
        }
        if let Some(u) = &input.unit {
            if !first {
                q.push(", ");
            }
            q.push("unit = ").push_bind(u.as_str());
            first = false;
        }
        if let Some(sku) = &input.sku {
            if !first {
                q.push(", ");
            }
            q.push("sku = ").push_bind(sku.as_str());
            first = false;
        }
        if let Some(d) = &input.description {
            if !first {
                q.push(", ");
            }
            q.push("description = ").push_bind(d.as_str());
            first = false;
        }
        // is_active: колонки НЕМАЄ в БД — Python ORM-атрибут не маппінг, no-op.

        if !first {
            q.push(", updated_at = now() WHERE id = ").push_bind(id);
            q.build().execute(&mut *tx).await.map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;

        self.row_by_id(id)
            .await?
            .ok_or_else(|| ProductsV2Error::NotFound(format!("Товар з ID '{id}' не знайдено")))
    }

    async fn delete(&self, id: Uuid) -> Result<(), ProductsV2Error> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let row = sqlx::query("SELECT title, stock::text FROM products WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
        let Some(row) = row else {
            return Err(ProductsV2Error::NotFound(format!(
                "Товар з ID '{id}' не знайдено"
            )));
        };
        let title: String = row.get(0);
        let stock: Option<String> = row.get(1);

        // 400: товар з ненульовим залишком (Python float-формат у detail).
        if let Some(s) = stock.as_deref() {
            if let Ok(v) = s.parse::<f64>() {
                if v != 0.0 {
                    return Err(ProductsV2Error::BadRequest(format!(
                        "Неможливо видалити товар '{title}': залишок на складі {} шт.",
                        py_float_str(v)
                    )));
                }
            }
        }
        sqlx::query("DELETE FROM products WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<ProductV2Dto, ProductsV2Error> {
        self.row_by_id(id)
            .await?
            .ok_or_else(|| ProductsV2Error::NotFound(format!("Товар з ID '{id}' не знайдено")))
    }

    async fn add_image(
        &self,
        product_id: Uuid,
        url: &str,
        is_main: bool,
    ) -> Result<ProductImageV2Dto, ProductsV2Error> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;

        // 404: товар існує.
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE id = $1)")
                .bind(product_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        if !exists {
            return Err(ProductsV2Error::NotFound(format!(
                "Товар з ID '{product_id}' не знайдено"
            )));
        }

        // is_main → скинути прапорець з інших зображень товару.
        if is_main {
            sqlx::query("UPDATE product_images SET is_main = false WHERE product_id = $1")
                .bind(product_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }

        // sort_order = поточна кількість зображень (Python count).
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM product_images WHERE product_id = $1")
                .bind(product_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;

        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO product_images \
             (id, product_id, url, is_main, sort_order, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, now(), now()) \
             RETURNING id, product_id, url, is_main, sort_order, created_at",
        )
        .bind(id)
        .bind(product_id)
        .bind(url)
        .bind(is_main)
        .bind(count as i32)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;

        Ok(ProductImageV2Dto {
            id: row.get("id"),
            product_id: row.get("product_id"),
            url: row.get("url"),
            is_main: row.get("is_main"),
            sort_order: row.get("sort_order"),
            created_at: row.get("created_at"),
        })
    }

    async fn delete_image(&self, image_id: Uuid) -> Result<(), ProductsV2Error> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM product_images WHERE id = $1)")
                .bind(image_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        if !exists {
            return Err(ProductsV2Error::NotFound(format!(
                "Зображення з ID '{image_id}' не знайдено"
            )));
        }
        sqlx::query("DELETE FROM product_images WHERE id = $1")
            .bind(image_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    async fn add_barcode(
        &self,
        product_id: Uuid,
        barcode: &str,
        is_primary: bool,
    ) -> Result<torgashka_domain::ProductBarcodeV2Dto, ProductsV2Error> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;

        // 404: товар існує.
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE id = $1)")
                .bind(product_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        if !exists {
            return Err(ProductsV2Error::NotFound(format!(
                "Товар з ID '{product_id}' не знайдено"
            )));
        }

        // 409: дублікат штрих-коду (Python ValueError «вже існує» → 409).
        let dup: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM barcodes WHERE barcode = $1)")
                .bind(barcode)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        if dup {
            return Err(ProductsV2Error::Conflict(format!(
                "Штрих-код '{barcode}' вже існує"
            )));
        }

        // is_primary → скинути з інших штрих-кодів товару.
        if is_primary {
            sqlx::query("UPDATE barcodes SET is_primary = false WHERE product_id = $1")
                .bind(product_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }

        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO barcodes \
             (id, product_id, barcode, is_primary, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, now(), now()) \
             RETURNING id, product_id, barcode, is_primary",
        )
        .bind(id)
        .bind(product_id)
        .bind(barcode)
        .bind(is_primary)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;

        Ok(torgashka_domain::ProductBarcodeV2Dto {
            id: row.get("id"),
            product_id: row.get("product_id"),
            barcode: row.get("barcode"),
            is_primary: row.get("is_primary"),
        })
    }

    async fn delete_barcode(&self, barcode_id: Uuid) -> Result<(), ProductsV2Error> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM barcodes WHERE id = $1)")
                .bind(barcode_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        if !exists {
            return Err(ProductsV2Error::NotFound(format!(
                "Штрих-код з ID '{barcode_id}' не знайдено"
            )));
        }
        sqlx::query("DELETE FROM barcodes WHERE id = $1")
            .bind(barcode_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }
}

/// Конвертація SQL-рядка → ProductV2Dto (Python entity_to_dto).
fn dto_from_row(r: &sqlx::postgres::PgRow) -> ProductV2Dto {
    ProductV2Dto {
        id: r.get("id"),
        name: r.get("title"),
        barcode: r.get("barcode"),
        price: to_f64(r.get("price")),
        cost_price: to_f64(r.get("cost_price")),
        quantity: to_f64(r.get("stock")).unwrap_or(0.0),
        unit: r.get("unit"),
        category_id: r.get("category_id"),
        supplier_id: r.get("supplier_id"),
        // У БД немає колонки is_active — Python getattr(entity, "is_active", True).
        is_active: true,
        sku: r.get::<Option<String>, _>("sku").unwrap_or_default(),
        description: r
            .get::<Option<String>, _>("description")
            .unwrap_or_default(),
    }
}

/// Помилка БД → Infrastructure (500).
fn db_err(e: sqlx::Error) -> ProductsV2Error {
    ProductsV2Error::Infrastructure(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn py_float_str_formats_like_python() {
        // Python str(float): 5.0 → "5.0", 5.5 → "5.5", 0.0 → "0.0".
        assert_eq!(py_float_str(5.0), "5.0");
        assert_eq!(py_float_str(5.5), "5.5");
        assert_eq!(py_float_str(0.0), "0.0");
        assert_eq!(py_float_str(1234.0), "1234.0");
        assert_eq!(py_float_str(2.25), "2.25");
    }

    #[test]
    fn to_f64_parses_numeric_text() {
        assert_eq!(to_f64(Some("150.50".into())), Some(150.5));
        assert_eq!(to_f64(Some("5.000".into())), Some(5.0));
        assert_eq!(to_f64(None), None);
        assert_eq!(to_f64(Some("abc".into())), None);
    }

    #[test]
    fn to_f64_quantity_default_zero() {
        // Python quantity default 0 (stock NULL → 0.0).
        assert_eq!(to_f64(None).unwrap_or(0.0), 0.0);
        assert_eq!(to_f64(Some("7.250".into())).unwrap_or(0.0), 7.25);
    }
}
