//! HTTP-клієнт Gemini API — 1:1 Python `google.genai` (models.generateContent).
//!
//! Формат запиту (SDK `genai.Client(...).models.generate_content`):
//! ```text
//! POST {base}/{api_version}/models/{model}:generateContent?key={api_key}
//! {"contents": [{"parts": [
//!     {"text": INVOICE_PROMPT},
//!     {"inline_data": {"mime_type": "image/jpeg", "data": "<base64>"}}
//! ]}]}
//! ```
//! Відповідь: `candidates[0].content.parts[].text` (Python `response.text`).
//! Base URL: env `KASA_OCR_BASE_URL` (аналог `GOOGLE_GEMINI_BASE_URL`),
//! дефолт `https://generativelanguage.googleapis.com/`.

use base64::Engine;
use serde_json::json;
use thiserror::Error;

/// Дефолтний base URL Gemini API (як у SDK `_api_client.py`).
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/";
/// Версія API (як у SDK для Gemini).
pub const DEFAULT_API_VERSION: &str = "v1beta";
/// Модель — як у Python `OCRService.analyze_invoice_image`.
pub const DEFAULT_MODEL: &str = "gemini-3.5-flash";
/// Кількість спроб на ключ (Python `max_retries = 3`).
pub const DEFAULT_MAX_RETRIES: usize = 3;
/// Затримка між звичайними запитами (Python `REQUEST_DELAY_SECONDS = 5`).
pub const REQUEST_DELAY_SECONDS: u64 = 5;
/// Затримка після 503 (Python `RETRY_AFTER_503_DELAY_SECONDS = 15`).
pub const RETRY_AFTER_503_DELAY_SECONDS: u64 = 15;
/// Таймаут HTTP-запиту (SDK за замовчуванням — 60с).
pub const HTTP_TIMEOUT_SECONDS: u64 = 60;

/// Промпт для Gemini — ТОЧНА копія `INVOICE_PROMPT` з `ocr_service.py`.
pub const INVOICE_PROMPT: &str = r###"Ти — асистент для розпізнавання прибуткових накладних. 
Проаналізуй зображення накладної та поверни ТІЛЬКИ JSON без додаткового тексту.

Формат JSON:
{
  "document_number": "рядок або null",
  "invoice_date": "рядок у форматі YYYY-MM-DD або null",
  "is_fiscal": true/false,
  "supplier_name": "рядок або null",
  "payment_method": "credit" | "bank_transfer" | "cash" | "other" | null,
  "items": [
    {
      "product_name": "рядок",
      "quantity": число,
      "cost_price": число,
      "barcode": "рядок або null"
    }
  ]
}

Правила:
- document_number: номер накладної (наприклад "ПН-00123")
- invoice_date: дата накладної
- is_fiscal: true якщо накладна фіскальна
- supplier_name: назва постачальника
- payment_method: спосіб оплати (credit - в борг, bank_transfer - перерахунок, cash - готівка, other - інше)
- items: масив товарів з назвою, кількістю та ціною з ПДВ (cost_price)
- cost_price: ціна товару з ПДВ (вартість з ПДВ за одиницю)
- barcode: штрих-код товару (EAN-13, 8-14 цифр). Якщо штрих-код чітко видно на накладній — обов'язково поверни його. Якщо не видно або сумніваєшся — null.
- Не повертай price (ціну без ПДВ) — вона не потрібна
- Якщо якесь поле відсутнє на зображенні, використовуй null
- Якщо товарів немає, поверни порожній масив items"###;

/// Класифікація помилки — 1:1 Python `str(e).lower()` пошук підрядків.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    /// 429 / too many requests / rate limit → ротація ключа (exhausted).
    RateLimit,
    /// 503 / service unavailable → затримка 15с, повтор (до max_retries).
    Unavailable503,
    /// 502 / timeout / deadline / unavailable → затримка 5с, повтор.
    Retryable,
    /// Інша помилка → ротація ключа.
    Other,
}

/// Класифікує повідомлення помилки — 1:1 Python `OCRService.analyze_invoice_image`.
pub fn classify_error(msg: &str) -> RetryKind {
    let s = msg.to_lowercase();
    if s.contains("429") || s.contains("too many requests") || s.contains("rate limit") {
        RetryKind::RateLimit
    } else if s.contains("503") || s.contains("service unavailable") {
        RetryKind::Unavailable503
    } else if s.contains("502")
        || s.contains("timeout")
        || s.contains("timed out")
        || s.contains("deadline")
        || s.contains("unavailable")
    {
        RetryKind::Retryable
    } else {
        RetryKind::Other
    }
}

/// Помилка клієнта Gemini (повідомлення зберігає коди статусів для classify).
#[derive(Debug, Error)]
pub enum GeminiError {
    #[error("{0}")]
    Api(String),
    /// HTTP-статус != 200: повідомлення містить код (для 1:1 класифікації).
    #[error("HTTP {0}: {1}")]
    Http(u16, String),
    #[error("таймаут: {0}")]
    Timeout(String),
    #[error("мережа: {0}")]
    Transport(String),
    /// Відповідь не містить кандидатів (Python ValueError).
    #[error("Відповідь Gemini не містить кандидатів")]
    NoCandidates,
    /// Порожній текст у відповіді (Python ValueError).
    #[error("Порожній текст у відповіді Gemini")]
    EmptyText,
}

/// HTTP-клієнт Gemini — 1:1 `google.genai` (один запит generateContent).
pub struct GeminiClient {
    base_url: String,
    api_version: String,
    model: String,
    api_keys: Vec<String>,
    current_key_index: std::sync::atomic::AtomicUsize,
    pub max_retries: usize,
    keys_file: std::path::PathBuf,
    http: reqwest::Client,
}

impl Default for GeminiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiClient {
    /// Створює клієнт: env `KASA_OCR_BASE_URL` / `KASA_OCR_KEYS_FILE` /
    /// `KASA_OCR_MODEL`, дефолти — як у Python. Завантажує ключі з keys.txt.
    pub fn new() -> Self {
        let base_url =
            std::env::var("KASA_OCR_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("KASA_OCR_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let keys_file = std::env::var("KASA_OCR_KEYS_FILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                // Python: Path(__file__).parent*4 / "keys.txt" = backend/keys.txt
                let from_backend = std::path::Path::new("backend/keys.txt");
                if from_backend.exists() {
                    from_backend.to_path_buf()
                } else {
                    std::path::PathBuf::from("keys.txt")
                }
            });
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECONDS))
            .build()
            .expect("reqwest client");
        let mut client = Self {
            base_url,
            api_version: DEFAULT_API_VERSION.to_string(),
            model,
            api_keys: Vec::new(),
            current_key_index: std::sync::atomic::AtomicUsize::new(0),
            max_retries: DEFAULT_MAX_RETRIES,
            keys_file,
            http,
        };
        client.load_api_keys();
        client
    }

    /// Завантажує ключі з keys.txt — 1:1 Python `_load_api_keys`
    /// (пропускає коментарі # та порожні рядки).
    pub fn load_api_keys(&mut self) {
        self.api_keys.clear();
        self.current_key_index
            .store(0, std::sync::atomic::Ordering::Relaxed);
        let Ok(content) = std::fs::read_to_string(&self.keys_file) else {
            tracing::warn!("Файл ключів не знайдено: {:?}", self.keys_file);
            return;
        };
        for line in content.lines() {
            let stripped = line.trim();
            if !stripped.is_empty() && !stripped.starts_with('#') {
                self.api_keys.push(stripped.to_string());
            }
        }
        tracing::info!("Завантажено {} ключ(ів) Gemini API", self.api_keys.len());
    }

    /// Поточний ключ (1:1 Python `_get_current_key`).
    pub fn current_key(&self) -> Option<String> {
        let idx = self
            .current_key_index
            .load(std::sync::atomic::Ordering::Relaxed);
        self.api_keys.get(idx).cloned()
    }

    /// Перемикає на наступний ключ (1:1 Python `_rotate_key`).
    pub fn rotate(&self) {
        let idx = self
            .current_key_index
            .load(std::sync::atomic::Ordering::Relaxed);
        self.current_key_index
            .store(idx + 1, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("Ротація ключа: перехід до індексу {}", idx + 1);
    }

    /// Скидає індекс ключа (Python `_reset_key_index`).
    pub fn reset_key_index(&self) {
        self.current_key_index
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Кількість ключів.
    pub fn key_count(&self) -> usize {
        self.api_keys.len()
    }

    /// Шлях до файлу ключів (для діагностики).
    pub fn keys_file_hint(&self) -> String {
        self.keys_file.display().to_string()
    }

    /// Один запит generateContent — повертає текст відповіді (response.text).
    /// 1:1 Python: `client.models.generate_content(model, contents=[prompt, part])`.
    pub async fn generate_content(&self, image_data: &[u8]) -> Result<String, GeminiError> {
        let api_key = self
            .current_key()
            .ok_or_else(|| GeminiError::Api("немає жодного ключа Gemini API".to_string()))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_data);
        let body = json!({
            "contents": [{
                "parts": [
                    {"text": INVOICE_PROMPT},
                    {"inline_data": {"mime_type": "image/jpeg", "data": b64}}
                ]
            }]
        });
        let url = format!(
            "{}{}/models/{}:generateContent?key={}",
            self.base_url, self.api_version, self.model, api_key
        );
        let resp = match self.http.post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() {
                    return Err(GeminiError::Timeout(e.to_string()));
                }
                return Err(GeminiError::Transport(e.to_string()));
            }
        };
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| GeminiError::Transport(e.to_string()))?;
        if status != reqwest::StatusCode::OK {
            // Обрізаємо тіло, щоб повідомлення містило код для classify 1:1.
            let truncated: String = text.chars().take(500).collect();
            return Err(GeminiError::Http(status.as_u16(), truncated));
        }
        // Парсинг candidates[0].content.parts[].text — 1:1 Python response.text.
        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| GeminiError::Api(format!("невалідний JSON від Gemini: {e}")))?;
        let candidates = v.get("candidates").and_then(|c| c.as_array());
        let Some(candidates) = candidates else {
            return Err(GeminiError::NoCandidates);
        };
        if candidates.is_empty() {
            return Err(GeminiError::NoCandidates);
        }
        let parts = candidates[0]
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array());
        let mut response_text = String::new();
        if let Some(parts) = parts {
            for part in parts {
                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                    response_text.push_str(t);
                }
            }
        }
        if response_text.is_empty() {
            return Err(GeminiError::EmptyText);
        }
        Ok(response_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_429() {
        assert_eq!(
            classify_error("429 RESOURCE_EXHAUSTED: quota exceeded"),
            RetryKind::RateLimit
        );
        assert_eq!(classify_error("too many requests"), RetryKind::RateLimit);
        assert_eq!(classify_error("rate limit exceeded"), RetryKind::RateLimit);
    }

    #[test]
    fn classify_503() {
        assert_eq!(
            classify_error("503 Service Unavailable"),
            RetryKind::Unavailable503
        );
        assert_eq!(
            classify_error("service unavailable, retry later"),
            RetryKind::Unavailable503
        );
    }

    #[test]
    fn classify_retryable() {
        assert_eq!(classify_error("502 Bad Gateway"), RetryKind::Retryable);
        assert_eq!(classify_error("request timeout"), RetryKind::Retryable);
        assert_eq!(classify_error("deadline exceeded"), RetryKind::Retryable);
        assert_eq!(
            classify_error("connection unavailable"),
            RetryKind::Retryable
        );
    }

    #[test]
    fn classify_other() {
        assert_eq!(classify_error("400 InvalidArgument"), RetryKind::Other);
        assert_eq!(classify_error("permission denied"), RetryKind::Other);
    }
}
