//! InvoiceOCR-сервіс — 1:1 Python `InvoiceOCRService` (invoice_ocr_service.py).
//!
//! Аналіз накладної через Gemini + автоматичне зіставлення товарів з БД:
//! 1. `analyze_invoice_image` (Gemini → товари накладної);
//! 2. для кожного товару — зіставлення за штрих-кодом (пріоритет),
//!    потім за назвою, інакше `not_found`;
//! 3. повернення `matched_product_id` / `matched_product_name` /
//!    `matched_barcode` / `markup_percent` / `match_source`.

use regex::Regex;
use serde_json::{json, Value};

use crate::models::InvoiceItem;
use crate::ocr_service::{OcrError, OcrService};
use crate::repository::{OcrRepository, ProductLookup};

/// Штрих-код у назві товару: "Назва (ШК: 12345)" — Python `r'[Шш][КкКк]\s*[:：]\s*(\d{8,14})'`.
fn barcode_in_name_re() -> Regex {
    Regex::new(r"[Шш][КкКк]\s*[:：]\s*(\d{8,14})").expect("regex")
}

/// Назва цілком є штрих-кодом — Python `r'^\d{8,14}$'`.
fn barcode_only_re() -> Regex {
    Regex::new(r"^\d{8,14}$").expect("regex")
}

/// Сервіс OCR + зіставлення з БД — 1:1 Python `InvoiceOCRService`.
pub struct InvoiceOcrService<R: OcrRepository> {
    repo: R,
    ocr: OcrService,
}

impl<R: OcrRepository> InvoiceOcrService<R> {
    pub fn new(repo: R) -> Self {
        Self {
            repo,
            ocr: OcrService::new(),
        }
    }

    /// Аналізує накладну та зіставляє товари з БД — 1:1 Python `analyze_and_match`.
    pub async fn analyze_and_match(
        &self,
        content_type: &str,
        image_data: &[u8],
    ) -> Result<Value, OcrError> {
        // Крок 1: Аналіз накладної через Gemini.
        let ocr_result = self
            .ocr
            .analyze_invoice_image(content_type, image_data)
            .await?;

        if ocr_result.items.is_empty() {
            // Python: return {"success": True, "data": {**ocr_result, "items": []}}
            let mut data = serde_json::to_value(&ocr_result)
                .map_err(|e| OcrError::parse(format!("серіалізація: {e}")))?;
            if let Some(obj) = data.as_object_mut() {
                obj.insert("items".to_string(), Value::Array(Vec::new()));
            }
            return Ok(json!({ "success": true, "data": data }));
        }

        // Крок 2: Зіставлення товарів з БД.
        let matched_items = self.match_items_with_db(&ocr_result.items).await?;

        let mut data = serde_json::to_value(&ocr_result)
            .map_err(|e| OcrError::parse(format!("серіалізація: {e}")))?;
        if let Some(obj) = data.as_object_mut() {
            obj.insert("items".to_string(), Value::Array(matched_items));
        }
        Ok(json!({ "success": true, "data": data }))
    }

    /// Зіставляє товари накладної з товарами БД — 1:1 Python `_match_items_with_db`.
    async fn match_items_with_db(&self, items: &[InvoiceItem]) -> Result<Vec<Value>, OcrError> {
        let mut matched_items = Vec::with_capacity(items.len());

        for item in items {
            let product_name = item.product_name.clone();
            let quantity = item.quantity;
            let cost_price = item.cost_price;
            let barcode_from_gemini = item.barcode.clone();

            // ─── Крок 2a: штрих-код (пріоритет) ─────────────────────────
            let mut barcode_to_search: Option<String> = None;

            // 1. Gemini повернув barcode.
            if let Some(b) = &barcode_from_gemini {
                let b = b.trim();
                if !b.is_empty() {
                    barcode_to_search = Some(b.to_string());
                }
            }
            // 2. Штрих-код у назві товару ("Назва (ШК: 12345)").
            if barcode_to_search.is_none() {
                if let Some(caps) = barcode_in_name_re().captures(&product_name) {
                    barcode_to_search = Some(caps[1].to_string());
                }
            }
            // 3. Сама назва є штрих-кодом (тільки цифри, 8-14 символів).
            if barcode_to_search.is_none() {
                let trimmed = product_name.trim();
                if barcode_only_re().is_match(trimmed) {
                    barcode_to_search = Some(trimmed.to_string());
                }
            }

            if let Some(bc) = &barcode_to_search {
                if let Ok(Some(product)) = self.repo.find_product_by_barcode(bc).await {
                    matched_items.push(make_result_item(
                        &product_name,
                        quantity,
                        cost_price,
                        &product,
                        "barcode",
                    ));
                    continue;
                }
            }

            // ─── Крок 2b: пошук за назвою ───────────────────────────────
            if let Ok(Some(product)) = self.repo.find_product_by_name(&product_name).await {
                matched_items.push(make_result_item(
                    &product_name,
                    quantity,
                    cost_price,
                    &product,
                    "name",
                ));
                continue;
            }

            // ─── Крок 2c: не знайдено → not_found ───────────────────────
            matched_items.push(json!({
                "product_name": product_name,
                "quantity": quantity,
                "cost_price": cost_price,
                "matched_product_id": Value::Null,
                "matched_product_name": Value::Null,
                "matched_barcode": Value::Null,
                "markup_percent": 0.0,
                "match_source": "not_found",
            }));
        }

        Ok(matched_items)
    }
}

/// Формує результат для знайденого товару — 1:1 Python `_make_result_item`.
fn make_result_item(
    product_name: &str,
    quantity: f64,
    cost_price: f64,
    product: &ProductLookup,
    match_source: &str,
) -> Value {
    json!({
        "product_name": product_name,
        "quantity": quantity,
        "cost_price": cost_price,
        "matched_product_id": product.id.to_string(),
        "matched_product_name": product.title,
        "matched_barcode": product.barcode,
        "markup_percent": product.markup.unwrap_or(0.0),
        "match_source": match_source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InvoiceData;
    use crate::repository::InMemoryOcrRepository;
    use uuid::Uuid;

    fn repo() -> InMemoryOcrRepository {
        let mut r = InMemoryOcrRepository::new();
        r.products.push((
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            "Молоко 2.6% с/п ТМ Селянське".to_string(),
            Some("4820000000001".to_string()),
            Some(20.0),
        ));
        r.products.push((
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            "Хліб Білий нарізний".to_string(),
            None,
            Some(15.0),
        ));
        r.extra_barcodes.push((
            "4821111111111".to_string(),
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
        ));
        r
    }

    fn item(name: &str, qty: f64, price: f64, barcode: Option<&str>) -> InvoiceItem {
        InvoiceItem {
            product_name: name.to_string(),
            quantity: qty,
            cost_price: price,
            barcode: barcode.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn match_by_barcode() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        let items = vec![item("Молоко", 2.0, 45.5, Some("4820000000001"))];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "barcode");
        assert_eq!(
            out[0]["matched_product_id"],
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(out[0]["markup_percent"], 20.0);
    }

    #[tokio::test]
    async fn match_by_extra_barcode_table() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        let items = vec![item("Молоко", 1.0, 45.5, Some("4821111111111"))];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "barcode");
        assert_eq!(
            out[0]["matched_product_id"],
            "11111111-1111-1111-1111-111111111111"
        );
    }

    #[tokio::test]
    async fn match_by_name_exact_case_insensitive() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        let items = vec![item("хліб білий нарізний", 3.0, 32.0, None)];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "name");
        assert_eq!(
            out[0]["matched_product_id"],
            "22222222-2222-2222-2222-222222222222"
        );
        assert_eq!(out[0]["markup_percent"], 15.0);
    }

    #[tokio::test]
    async fn match_by_name_partial() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        // Частковий збіг: назва товару містить запит.
        let items = vec![item("Молоко 2.6%", 2.0, 45.5, None)];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "name");
        assert_eq!(
            out[0]["matched_product_id"],
            "11111111-1111-1111-1111-111111111111"
        );
    }

    #[tokio::test]
    async fn match_by_words() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        // Пошук за окремими словами: "Хліб" і "Білий" обидва в назві.
        let items = vec![item("Хліб Білий", 1.0, 32.0, None)];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "name");
        assert_eq!(
            out[0]["matched_product_id"],
            "22222222-2222-2222-2222-222222222222"
        );
    }

    #[tokio::test]
    async fn not_found() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        let items = vec![item("Невідомий товар X", 5.0, 10.0, None)];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "not_found");
        assert_eq!(out[0]["matched_product_id"], Value::Null);
        assert_eq!(out[0]["markup_percent"], 0.0);
    }

    #[tokio::test]
    async fn barcode_in_name() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        let items = vec![item("Молоко (ШК: 4820000000001)", 1.0, 45.5, None)];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "barcode");
        assert_eq!(
            out[0]["matched_product_id"],
            "11111111-1111-1111-1111-111111111111"
        );
    }

    #[tokio::test]
    async fn name_is_barcode() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        let items = vec![item("4820000000001", 1.0, 45.5, None)];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "barcode");
    }

    #[tokio::test]
    async fn barcode_clean_recursion() {
        let r = repo();
        let svc = InvoiceOcrService::new(r);
        // barcode з пробілами/дефісами → очищений → збіг.
        let items = vec![item("Молоко", 1.0, 45.5, Some("482 0000-0000_01"))];
        let out = svc.match_items_with_db(&items).await.unwrap();
        assert_eq!(out[0]["match_source"], "barcode");
    }

    #[tokio::test]
    async fn empty_items_returns_empty() {
        let r = repo();
        let _svc = InvoiceOcrService::new(r);
        let data = InvoiceData {
            document_number: Some("ПН-1".to_string()),
            invoice_date: None,
            is_fiscal: false,
            supplier_name: None,
            payment_method: None,
            items: Vec::new(),
        };
        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["items"], Value::Array(vec![]));
        assert_eq!(value["document_number"], "ПН-1");
    }
}
