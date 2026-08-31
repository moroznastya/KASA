//! Репозиторій OCR — абстракція БД для зіставлення товарів накладної.
//! 1:1 Python `InvoiceOCRService._find_product_by_barcode` / `_find_product_by_name`.
//! Реалізації: `InMemoryOcrRepository` (тести), sqlx — у torgashka-infrastructure.

use async_trait::async_trait;
use uuid::Uuid;

/// Товар з БД для зіставлення (поля, які використовує invoice_ocr).
#[derive(Debug, Clone)]
pub struct ProductLookup {
    pub id: Uuid,
    pub title: String,
    pub barcode: Option<String>,
    /// markup (націнка, %) — Python `product.markup` (може бути None → 0.0).
    pub markup: Option<f64>,
}

/// Помилка репозиторію OCR (ізольована від sqlx — torgashka-ocr не залежить від БД).
#[derive(Debug, thiserror::Error)]
pub enum OcrRepoError {
    #[error("помилка БД: {0}")]
    Db(String),
}

/// Контракт репозиторію OCR — 1:1 Python пошукові методи InvoiceOCRService.
#[async_trait]
pub trait OcrRepository: Send + Sync {
    /// Спроба 1: таблиця `barcodes` → product_id → products;
    /// Спроба 2: `products.barcode`;
    /// Спроба 3: чистий barcode (без пробілів/дефісів/підкреслень) — рекурсія.
    async fn find_product_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<ProductLookup>, OcrRepoError>;
    /// Спроба 1: точний збіг title ILIKE name;
    /// Спроба 2: частковий (%name%);
    /// Спроба 3: по окремих словах (best match за кількістю збігів).
    async fn find_product_by_name(&self, name: &str)
        -> Result<Option<ProductLookup>, OcrRepoError>;
}

/// InMemory-реалізація для тестів (case-insensitive — аналог ILIKE).
#[derive(Debug, Default)]
pub struct InMemoryOcrRepository {
    /// (title, barcode, markup) — barcode у products.
    pub products: Vec<(Uuid, String, Option<String>, Option<f64>)>,
    /// Додаткові штрих-коди з таблиці `barcodes` (barcode, product_id).
    pub extra_barcodes: Vec<(String, Uuid)>,
}

impl InMemoryOcrRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl OcrRepository for InMemoryOcrRepository {
    async fn find_product_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<ProductLookup>, OcrRepoError> {
        // Спроба 1: таблиця barcodes (додаткові штрих-коди).
        if let Some((_, pid)) = self.extra_barcodes.iter().find(|(b, _)| b == barcode) {
            if let Some(p) = self.products.iter().find(|(id, _, _, _)| id == pid) {
                return Ok(Some(ProductLookup {
                    id: p.0,
                    title: p.1.clone(),
                    barcode: p.2.clone(),
                    markup: p.3,
                }));
            }
        }
        // Спроба 2: products.barcode.
        if let Some(p) = self
            .products
            .iter()
            .find(|(_, _, b, _)| b.as_deref() == Some(barcode))
        {
            return Ok(Some(ProductLookup {
                id: p.0,
                title: p.1.clone(),
                barcode: p.2.clone(),
                markup: p.3,
            }));
        }
        // Спроба 3: чистий barcode — рекурсія.
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
        let lower = clean_name.to_lowercase();
        // Спроба 1: точний (case-insensitive).
        if let Some(p) = self
            .products
            .iter()
            .find(|(_, t, _, _)| t.to_lowercase() == lower)
        {
            return Ok(Some(ProductLookup {
                id: p.0,
                title: p.1.clone(),
                barcode: p.2.clone(),
                markup: p.3,
            }));
        }
        // Спроба 2: частковий (назва містить запит).
        if let Some(p) = self
            .products
            .iter()
            .find(|(_, t, _, _)| t.to_lowercase().contains(&lower))
        {
            return Ok(Some(ProductLookup {
                id: p.0,
                title: p.1.clone(),
                barcode: p.2.clone(),
                markup: p.3,
            }));
        }
        // Спроба 3: по окремих словах (best match за кількістю збігів).
        let words: Vec<&str> = clean_name.split_whitespace().collect();
        if words.len() > 1 {
            let mut best: Option<(&(Uuid, String, Option<String>, Option<f64>), usize)> = None;
            for p in &self.products {
                let title_lower = p.1.to_lowercase();
                let count = words
                    .iter()
                    .filter(|w| title_lower.contains(&w.to_lowercase()))
                    .count();
                if count > 0 && best.as_ref().map(|(_, c)| count > *c).unwrap_or(true) {
                    best = Some((p, count));
                }
            }
            if let Some((p, _)) = best {
                return Ok(Some(ProductLookup {
                    id: p.0,
                    title: p.1.clone(),
                    barcode: p.2.clone(),
                    markup: p.3,
                }));
            }
        }
        Ok(None)
    }
}
