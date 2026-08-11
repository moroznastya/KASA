//! SqlxOcrRepository — PostgreSQL-реалізація `torgashka_ocr::repository::OcrRepository`.
//! 1:1 Python `InvoiceOCRService._find_product_by_barcode` / `_find_product_by_name`
//! (таблиці `barcodes` + `products`).

use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx::PgPool;
use torgashka_ocr::{OcrRepoError, OcrRepository, ProductLookup};
use uuid::Uuid;

/// sqlx-рядок products (поля, потрібні invoice_ocr).
#[derive(sqlx::FromRow, Clone)]
struct ProductRow {
    id: Uuid,
    title: String,
    barcode: Option<String>,
    /// NUMERIC(5,2) — Decimal у sqlx; конвертація у f64 (Python float(product.markup)).
    markup: Option<Decimal>,
}

impl From<ProductRow> for ProductLookup {
    fn from(r: ProductRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            barcode: r.barcode,
            markup: r.markup.map(|d| d.to_f64().unwrap_or(0.0)),
        }
    }
}

/// PostgreSQL-реалізація пошуку товарів для OCR-зіставлення.
#[derive(Debug, Clone)]
pub struct SqlxOcrRepository {
    pool: PgPool,
}

impl SqlxOcrRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OcrRepository for SqlxOcrRepository {
    async fn find_product_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<ProductLookup>, OcrRepoError> {
        // Спроба 1: таблиця Barcode (barcodes.product_id → products).
        // Python: select(Barcode).where(barcode==...) → scalar_one_or_none →
        //         select(Product).where(id==barcode_record.product_id).
        // ix_barcodes_barcode UNIQUE → дублікатів немає → LIMIT 1 еквівалентно.
        let row: Option<ProductRow> = sqlx::query_as(
            "SELECT p.id, p.title, p.barcode, p.markup              FROM barcodes b JOIN products p ON p.id = b.product_id              WHERE b.barcode = $1 LIMIT 1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| OcrRepoError::Db(e.to_string()))?;
        if let Some(r) = row {
            return Ok(Some(r.into()));
        }

        // Спроба 2: products.barcode.
        let row: Option<ProductRow> = sqlx::query_as(
            "SELECT id, title, barcode, markup FROM products WHERE barcode = $1 LIMIT 1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| OcrRepoError::Db(e.to_string()))?;
        if let Some(r) = row {
            return Ok(Some(r.into()));
        }

        // Спроба 3: чистий barcode (без пробілів/дефісів/підкреслень) — рекурсія.
        let clean = barcode.replace([' ', '-', '_'], "");
        if clean != barcode {
            return self.find_product_by_barcode(&clean).await;
        }

        Ok(None)
    }

    async fn find_product_by_name(
        &self,
        name: &str,
    ) -> Result<Option<ProductLookup>, OcrRepoError> {
        let clean_name = name.trim();
        if clean_name.is_empty() {
            return Ok(None);
        }

        // Спроба 1: точний збіг (case-insensitive) — Python title.ilike(clean_name).
        let row: Option<ProductRow> = sqlx::query_as(
            "SELECT id, title, barcode, markup FROM products WHERE title ILIKE $1 LIMIT 1",
        )
        .bind(clean_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| OcrRepoError::Db(e.to_string()))?;
        if let Some(r) = row {
            return Ok(Some(r.into()));
        }

        // Спроба 2: частковий збіг — Python title.ilike(f"%{clean_name}%").
        let pattern = format!("%{clean_name}%");
        let row: Option<ProductRow> = sqlx::query_as(
            "SELECT id, title, barcode, markup FROM products WHERE title ILIKE $1 LIMIT 1",
        )
        .bind(&pattern)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| OcrRepoError::Db(e.to_string()))?;
        if let Some(r) = row {
            return Ok(Some(r.into()));
        }

        // Спроба 3: пошук за окремими словами — Python or_(*[ilike %word%])
        // → всі збіги → best_match = max за кількістю слів у title.
        let words: Vec<&str> = clean_name.split_whitespace().collect();
        if words.len() > 1 {
            // Python: or_(*[ilike %word%]) — будь-яке слово. SQL: ILIKE ANY(масив).
            let patterns: Vec<String> = words
                .iter()
                .map(|w| format!("%{}%", w.to_lowercase()))
                .collect();
            let rows: Vec<ProductRow> = sqlx::query_as(
                "SELECT id, title, barcode, markup FROM products WHERE title ILIKE ANY($1::text[]) LIMIT 50",
            )
            .bind(&patterns)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| OcrRepoError::Db(e.to_string()))?;
            if !rows.is_empty() {
                // best_match = max за кількістю слів, присутніх у title (case-insensitive).
                let best = rows
                    .iter()
                    .max_by_key(|r| {
                        let title_lower = r.title.to_lowercase();
                        words
                            .iter()
                            .filter(|w| title_lower.contains(&w.to_lowercase()))
                            .count()
                    })
                    .cloned();
                if let Some(r) = best {
                    return Ok(Some(r.into()));
                }
            }
        }

        Ok(None)
    }
}
