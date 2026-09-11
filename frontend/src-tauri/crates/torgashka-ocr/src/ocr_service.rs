//! OCR-сервіс — 1:1 Python `OCRService` (ocr_service.py).
//!
//! `analyze_invoice_image`: цикл з ротацією ключів (429 → exhausted),
//! ретраями (503 → 15с, 502/timeout → 5с, max 3 спроби на ключ),
//! затримкою 5с між запитами та парсингом JSON з відповіді Gemini.

use std::collections::HashSet;

use regex::Regex;
use thiserror::Error;

use crate::gemini::{
    classify_error, GeminiClient, RetryKind, REQUEST_DELAY_SECONDS, RETRY_AFTER_503_DELAY_SECONDS,
};
use crate::models::InvoiceData;

/// Помилка OCR-сервісу.
///
/// [`Runtime`] відповідає Python `RuntimeError` (роут повертає `error: msg`),
/// [`Parse`] — Python `ValueError`/інші (`error: "Внутрішня помилка сервера: ..."`).
#[derive(Debug, Error)]
pub enum OcrError {
    /// 1:1 Python `RuntimeError` (вичерпано ключі / фінальна помилка).
    #[error("{0}")]
    Runtime(String),
    /// 1:1 Python `ValueError` та інші винятки (парсинг відповіді).
    #[error("{0}")]
    Parse(String),
}

impl OcrError {
    pub(crate) fn parse(msg: impl Into<String>) -> Self {
        OcrError::Parse(msg.into())
    }
}

/// Regex `(?s)\{.*\}` — inline-прапорець `s` (single-line) = Python `re.DOTALL`:
/// крапка матчить `\n`, тому багаторядковий JSON від Gemini знаходиться цілком.
fn json_regex() -> Regex {
    Regex::new(r"(?s)\{.*\}").expect("regex")
}

/// Обрізає текст до `max` символів (додає "…" якщо обрізано).
fn truncate_chars(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let mut out: String = chars[..max].iter().collect();
        out.push('…');
        out
    }
}

/// Парсить відповідь Gemini — 1:1 Python `_parse_gemini_response`.
///
/// Діагностика: при будь-якій помилці парсингу (окрім порожньої відповіді)
/// сирий текст Gemini логується через `tracing::error!` (обрізаний до ~2000
/// символів) і перші ~300 символів включаються в повідомлення помилки —
/// фронтенд показує користувачу причину.
pub fn parse_gemini_response(response_text: &str) -> Result<InvoiceData, OcrError> {
    if response_text.is_empty() {
        return Err(OcrError::parse("Порожня відповідь від Gemini"));
    }
    // Будує помилку парсингу: логує сирий текст + додає снипет у повідомлення.
    let parse_err = |context: &str| -> OcrError {
        tracing::error!(
            "Сирий текст відповіді Gemini: {:?}",
            truncate_chars(response_text, 2000)
        );
        OcrError::parse(format!(
            "{context}. Сирий текст відповіді Gemini: {}",
            truncate_chars(response_text, 300)
        ))
    };
    let json_match = json_regex()
        .find(response_text)
        .ok_or_else(|| parse_err("Не вдалося знайти JSON у відповіді Gemini"))?;
    let v: serde_json::Value = serde_json::from_str(json_match.as_str())
        .map_err(|e| parse_err(&format!("Невалідний JSON у відповіді Gemini: {e}")))?;
    if v.as_object().is_none() {
        return Err(parse_err("Відповідь Gemini не є об'єктом JSON"));
    }
    // Python: "items" not in data or not isinstance(items, list) → data["items"] = []
    // Замінюємо items на [] ПЕРЕД десереалізацією (інакше serde падає на не-list).
    let mut v = v;
    if !v.get("items").map(|i| i.is_array()).unwrap_or(false) {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("items".to_string(), serde_json::Value::Array(Vec::new()));
        }
    }
    let data: InvoiceData = serde_json::from_value(v)
        .map_err(|e| parse_err(&format!("Невалідний JSON у відповіді Gemini: {e}")))?;
    Ok(data)
}

/// Сервіс розпізнавання накладних через Gemini — 1:1 Python `OCRService`.
pub struct OcrService {
    client: GeminiClient,
}

impl Default for OcrService {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrService {
    pub fn new() -> Self {
        Self {
            client: GeminiClient::new(),
        }
    }

    /// Внутрішній доступ до клієнта (для тестів).
    pub fn client(&self) -> &GeminiClient {
        &self.client
    }

    /// Аналізує зображення накладної — 1:1 Python `analyze_invoice_image`.
    ///
    /// `content_type` — реальний MIME-тип файлу (вже валідований через
    /// ALLOWED_IMAGE_TYPES в API-роуті); передається в Gemini, щоб модель
    /// отримувала правильний заголовок для PNG/WebP/TIFF/BMP.
    pub async fn analyze_invoice_image(
        &self,
        content_type: &str,
        image_data: &[u8],
    ) -> Result<InvoiceData, OcrError> {
        let mut exhausted_keys: HashSet<String> = HashSet::new();
        let mut first_request = true;

        loop {
            let api_key = self.client.current_key();
            // Якщо поточний ключ вичерпано — переходимо до наступного.
            if let Some(k) = &api_key {
                if exhausted_keys.contains(k) {
                    self.client.rotate();
                    continue;
                }
            }
            let Some(api_key) = api_key else {
                // Всі ключі вичерпано.
                return Err(OcrError::Runtime(
                    "Всі ключі Gemini API вичерпано (всі повернули 429). Додайте нові ключі в keys.txt"
                        .to_string(),
                ));
            };

            for attempt in 1..=self.client.max_retries {
                // Затримка перед кожним запитом, крім найпершого.
                if !first_request {
                    tokio::time::sleep(std::time::Duration::from_secs(REQUEST_DELAY_SECONDS)).await;
                }
                first_request = false;

                match self.client.generate_content(content_type, image_data).await {
                    Ok(response_text) => {
                        let result = parse_gemini_response(&response_text)?;
                        tracing::info!("Успішно отримано та розпарсено відповідь Gemini");
                        return Ok(result);
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        tracing::error!("Помилка Gemini API: {error_str}");
                        match classify_error(&error_str) {
                            RetryKind::RateLimit => {
                                tracing::warn!("Помилка 429 (Too Many Requests) для ключа");
                                exhausted_keys.insert(api_key.clone());
                                tokio::time::sleep(std::time::Duration::from_secs(
                                    REQUEST_DELAY_SECONDS,
                                ))
                                .await;
                                self.client.rotate();
                                break;
                            }
                            RetryKind::Unavailable503 => {
                                if attempt < self.client.max_retries {
                                    tracing::warn!(
                                        "Помилка 503 (Service Unavailable) (спроба {attempt}/{})",
                                        self.client.max_retries
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(
                                        RETRY_AFTER_503_DELAY_SECONDS,
                                    ))
                                    .await;
                                    continue;
                                } else {
                                    tracing::error!("Всі спроби вичерпано після 503");
                                    tokio::time::sleep(std::time::Duration::from_secs(
                                        REQUEST_DELAY_SECONDS,
                                    ))
                                    .await;
                                    self.client.rotate();
                                    break;
                                }
                            }
                            RetryKind::Retryable => {
                                if attempt < self.client.max_retries {
                                    tracing::warn!(
                                        "Тимчасова помилка (спроба {attempt}/{})",
                                        self.client.max_retries
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(
                                        REQUEST_DELAY_SECONDS,
                                    ))
                                    .await;
                                    continue;
                                } else {
                                    tracing::error!(
                                        "Всі спроби вичерпано після тимчасової помилки"
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(
                                        REQUEST_DELAY_SECONDS,
                                    ))
                                    .await;
                                    self.client.rotate();
                                    break;
                                }
                            }
                            RetryKind::Other => {
                                tracing::error!("Помилка Gemini API, ротація ключа");
                                tokio::time::sleep(std::time::Duration::from_secs(
                                    REQUEST_DELAY_SECONDS,
                                ))
                                .await;
                                self.client.rotate();
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_json() {
        let text = r#"{"document_number":"ПН-00123","invoice_date":"2026-07-23","is_fiscal":false,"supplier_name":"ТОВ Постачальник","payment_method":"credit","items":[{"product_name":"Молоко","quantity":2,"cost_price":45.5,"barcode":"4820000000001"}]}"#;
        let d = parse_gemini_response(text).unwrap();
        assert_eq!(d.document_number.as_deref(), Some("ПН-00123"));
        assert_eq!(d.invoice_date.as_deref(), Some("2026-07-23"));
        assert!(!d.is_fiscal);
        assert_eq!(d.items.len(), 1);
        assert_eq!(d.items[0].barcode.as_deref(), Some("4820000000001"));
        assert_eq!(d.items[0].quantity, 2.0);
    }

    #[test]
    fn parse_with_surrounding_text() {
        let text = "Ось JSON:\n```json\n{\"document_number\":\"ПН-1\",\"items\":[]}\n```";
        let d = parse_gemini_response(text).unwrap();
        assert_eq!(d.document_number.as_deref(), Some("ПН-1"));
        assert!(d.items.is_empty());
    }

    #[test]
    fn parse_items_not_list_becomes_empty() {
        let text = r#"{"document_number":"x","items":"не список"}"#;
        let d = parse_gemini_response(text).unwrap();
        assert!(d.items.is_empty());
    }

    #[test]
    fn parse_no_items_key() {
        let text = r#"{"document_number":"x"}"#;
        let d = parse_gemini_response(text).unwrap();
        assert!(d.items.is_empty());
        assert!(!d.is_fiscal);
    }

    #[test]
    fn parse_empty_text() {
        assert!(parse_gemini_response("").is_err());
    }

    #[test]
    fn parse_no_json() {
        assert!(parse_gemini_response("просто текст").is_err());
    }

    // ─── Регресія: багаторядковий JSON (як реальна відповідь Gemini) ────

    #[test]
    fn parse_multiline_json_with_fences() {
        let text = "```json\n{\n  \"document_number\": \"ПН-00999\",\n  \"invoice_date\": \"2026-08-19\",\n  \"is_fiscal\": false,\n  \"supplier_name\": \"ТОВ Тест\",\n  \"payment_method\": \"cash\",\n  \"items\": [\n    {\"product_name\": \"Молоко\", \"quantity\": 10, \"cost_price\": 45.5, \"barcode\": \"4820000000001\"}\n  ]\n}\n```";
        let d = parse_gemini_response(text).unwrap();
        assert_eq!(d.document_number.as_deref(), Some("ПН-00999"));
        assert_eq!(d.invoice_date.as_deref(), Some("2026-08-19"));
        assert!(!d.is_fiscal);
        assert_eq!(d.supplier_name.as_deref(), Some("ТОВ Тест"));
        assert_eq!(d.payment_method.as_deref(), Some("cash"));
        assert_eq!(d.items.len(), 1);
        assert_eq!(d.items[0].product_name, "Молоко");
        assert_eq!(d.items[0].quantity, 10.0);
        assert_eq!(d.items[0].barcode.as_deref(), Some("4820000000001"));
    }

    #[test]
    fn parse_multiline_json_no_fences() {
        // Без ```json огорожі — сирий багаторядковий JSON з переносами.
        let text = "{\n  \"document_number\": \"ПН-001\",\n  \"items\": []\n}";
        let d = parse_gemini_response(text).unwrap();
        assert_eq!(d.document_number.as_deref(), Some("ПН-001"));
        assert!(d.items.is_empty());
    }

    #[test]
    fn parse_invalid_json() {
        assert!(parse_gemini_response(r#"{"a": не json}"#).is_err());
    }

    #[test]
    fn parse_not_object() {
        assert!(parse_gemini_response("[1,2,3]").is_err());
    }

    // ─── Діагностика: снипет сирого тексту у повідомленні ────────────────

    #[test]
    fn parse_no_json_message_includes_snippet() {
        let err = parse_gemini_response(
            "Вибачте, я не можу розпізнати зображення. Будь ласка, спробуйте ще раз.",
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Не вдалося знайти JSON у відповіді Gemini"),
            "{msg}"
        );
        assert!(
            msg.contains("Вибачте, я не можу розпізнати"),
            "повідомлення має містити сирий текст: {msg}"
        );
    }

    #[test]
    fn parse_invalid_json_message_includes_snippet() {
        let err = parse_gemini_response(r#"{"a": не json} щось"#).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Невалідний JSON"), "{msg}");
        assert!(msg.contains("не json"), "{msg}");
    }

    #[test]
    fn parse_empty_message_no_snippet() {
        let err = parse_gemini_response("").unwrap_err();
        assert_eq!(err.to_string(), "Порожня відповідь від Gemini");
    }

    #[test]
    fn truncate_long_text() {
        let long = "а".repeat(5000);
        let t = truncate_chars(&long, 2000);
        assert_eq!(t.chars().count(), 2001); // 2000 + "…"
        assert!(t.ends_with('…'));
        let short = truncate_chars("короткий", 2000);
        assert_eq!(short, "короткий");
    }
}
