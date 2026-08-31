//! HTTP-клієнт Gemini API — 1:1 Python `google.genai` (models.generateContent).
//!
//! Формат запиту (SDK `genai.Client(...).models.generate_content`):
//! ```text
//! POST {base}/{api_version}/models/{model}:generateContent?key={api_key}
//! {"contents": [{"parts": [
//!     {"text": INVOICE_PROMPT},
//!     {"inline_data": {"mime_type": "<реальний content_type>", "data": "<base64>"}}
//! ]}]}
//! ```
//! Відповідь: `candidates[0].content.parts[].text` (Python `response.text`).
//! Base URL: env `TORGASHKA_OCR_BASE_URL` (аналог `GOOGLE_GEMINI_BASE_URL`),
//! дефолт `https://generativelanguage.googleapis.com/`.
//!
//! Відмінність від Python (виправлення багів):
//! - `mime_type` береться з реального content_type файлу (не хардкод image/jpeg);
//! - обробляються `finishReason` (SAFETY/MAX_TOKENS/RECITATION) та
//!   `promptFeedback.blockReason` — замість порожньої помилки парсингу JSON
//!   користувач отримує змістовну причину.

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
    /// Відповідь заблокована safety-політикою: finishReason=SAFETY /
    /// PROHIBITED_CONTENT або promptFeedback.blockReason.
    #[error("Gemini заблокував відповідь (safety): {0}")]
    Blocked(String),
    /// Відповідь обрізана (finishReason=MAX_TOKENS).
    #[error("Відповідь Gemini обрізана (MAX_TOKENS)")]
    Truncated,
    /// Інший finishReason (RECITATION тощо).
    #[error("Gemini finishReason={0}")]
    FinishReason(String),
}

/// Будує тіло запиту generateContent — окремо для юніт-тестів.
///
/// `mime_type` — реальний content_type файлу (вже валідований через
/// ALLOWED_IMAGE_TYPES на рівні API). НЕ хардкодимо image/jpeg: PNG/WebP/
/// TIFF/BMP з JPEG-заголовком Gemini не розпізнає.
fn build_request_body(content_type: &str, image_data: &[u8]) -> serde_json::Value {
    let b64 = base64::engine::general_purpose::STANDARD.encode(image_data);
    json!({
        "contents": [{
            "parts": [
                {"text": INVOICE_PROMPT},
                {"inline_data": {"mime_type": content_type, "data": b64}}
            ]
        }]
    })
}

/// Збирає summary safetyRatings (MEDIUM/HIGH) для діагностики блокування.
fn safety_ratings_summary(candidate: &serde_json::Value) -> String {
    let Some(ratings) = candidate.get("safetyRatings").and_then(|s| s.as_array()) else {
        return String::new();
    };
    ratings
        .iter()
        .filter_map(|r| {
            let category = r.get("category").and_then(|c| c.as_str()).unwrap_or("?");
            let probability = r.get("probability").and_then(|p| p.as_str()).unwrap_or("?");
            // NEGLIGIBLE/LOW/UNKNOWN — не блокуючі; показуємо лише суттєві.
            if matches!(probability, "NEGLIGIBLE" | "LOW" | "UNKNOWN") {
                None
            } else {
                Some(format!("{category}={probability}"))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Витягує текст з JSON-відповіді Gemini — окремо для юніт-тестів.
///
/// Обробляє `promptFeedback.blockReason` (навіть без кандидатів) та
/// `candidates[0].finishReason` != STOP → змістовна помилка замість
/// порожнього тексту/NoCandidates.
fn extract_response_text(response_json: &str) -> Result<String, GeminiError> {
    let v: serde_json::Value = serde_json::from_str(response_json)
        .map_err(|e| GeminiError::Api(format!("невалідний JSON від Gemini: {e}")))?;
    // Блокування на рівні запиту (promptFeedback) — кандидатів може не бути.
    let block_reason = v
        .pointer("/promptFeedback/blockReason")
        .and_then(|b| b.as_str())
        .unwrap_or("");
    if !block_reason.is_empty() {
        return Err(GeminiError::Blocked(block_reason.to_string()));
    }
    let Some(candidates) = v.get("candidates").and_then(|c| c.as_array()) else {
        return Err(GeminiError::NoCandidates);
    };
    if candidates.is_empty() {
        return Err(GeminiError::NoCandidates);
    }
    let candidate = &candidates[0];
    // finishReason != STOP → змістовна помилка замість "не вдалося знайти JSON".
    if let Some(reason) = candidate.get("finishReason").and_then(|f| f.as_str()) {
        if reason != "STOP" {
            let safety = safety_ratings_summary(candidate);
            return Err(match reason {
                "SAFETY" | "PROHIBITED_CONTENT" => {
                    let mut detail = reason.to_string();
                    if !block_reason.is_empty() {
                        detail.push_str(&format!(" (blockReason: {block_reason})"));
                    }
                    if !safety.is_empty() {
                        detail.push_str(&format!(" (safety: {safety})"));
                    }
                    GeminiError::Blocked(detail)
                }
                "MAX_TOKENS" => GeminiError::Truncated,
                "RECITATION" => {
                    GeminiError::Api("Відповідь Gemini відхилена (recitation)".to_string())
                }
                other => GeminiError::FinishReason(other.to_string()),
            });
        }
    }
    let parts = candidate
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
    /// Створює клієнт: env `TORGASHKA_OCR_BASE_URL` / `TORGASHKA_OCR_KEYS_FILE` /
    /// `TORGASHKA_OCR_MODEL`, дефолти — як у Python. Завантажує ключі з keys.txt.
    pub fn new() -> Self {
        let base_url = std::env::var("TORGASHKA_OCR_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model =
            std::env::var("TORGASHKA_OCR_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let keys_file = std::env::var("TORGASHKA_OCR_KEYS_FILE")
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
    ///
    /// `content_type` — реальний MIME-тип файлу (image/jpeg, image/png, ...),
    /// передається в `inline_data.mime_type` замість хардкоду image/jpeg.
    pub async fn generate_content(
        &self,
        content_type: &str,
        image_data: &[u8],
    ) -> Result<String, GeminiError> {
        let api_key = self
            .current_key()
            .ok_or_else(|| GeminiError::Api("немає жодного ключа Gemini API".to_string()))?;
        let body = build_request_body(content_type, image_data);
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
        // Парсинг candidates[0].content.parts[].text + finishReason/blockReason.
        extract_response_text(&text)
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

    // ─── mime_type у тілі запиту ──────────────────────────────────────────

    #[test]
    fn build_body_uses_real_content_type_png() {
        let body = build_request_body("image/png", b"\x89PNG\r\n\x1a\n");
        let mime = body["contents"][0]["parts"][1]["inline_data"]["mime_type"]
            .as_str()
            .unwrap();
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn build_body_not_hardcoded_jpeg() {
        for ct in [
            "image/jpeg",
            "image/png",
            "image/webp",
            "image/bmp",
            "image/tiff",
        ] {
            let body = build_request_body(ct, b"data");
            let mime = body["contents"][0]["parts"][1]["inline_data"]["mime_type"]
                .as_str()
                .unwrap();
            assert_eq!(mime, ct, "mime_type має дорівнювати реальному content_type");
        }
    }

    #[test]
    fn build_body_base64_data() {
        let body = build_request_body("image/jpeg", b"\xff\xd8\xff\xe0");
        let data = body["contents"][0]["parts"][1]["inline_data"]["data"]
            .as_str()
            .unwrap();
        assert_eq!(
            data,
            base64::engine::general_purpose::STANDARD.encode(b"\xff\xd8\xff\xe0")
        );
    }

    #[test]
    fn build_body_prompt_is_first_part() {
        let body = build_request_body("image/png", b"x");
        let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
        assert!(text.contains("прибуткових накладних"));
    }

    // ─── finishReason / blockReason ───────────────────────────────────────

    #[test]
    fn extract_text_ok() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":"{\"a\":1}"}]},"finishReason":"STOP"}]}"#;
        assert_eq!(extract_response_text(json).unwrap(), r#"{"a":1}"#);
    }

    #[test]
    fn extract_text_ok_no_finish_reason() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":"hello"}]}}]}"#;
        assert_eq!(extract_response_text(json).unwrap(), "hello");
    }

    #[test]
    fn extract_finish_reason_safety() {
        let json = r#"{"candidates":[{"finishReason":"SAFETY","safetyRatings":[{"category":"HARM_CATEGORY_DANGEROUS_CONTENT","probability":"MEDIUM"},{"category":"HARM_CATEGORY_HARASSMENT","probability":"NEGLIGIBLE"}]}]}"#;
        let err = extract_response_text(json).unwrap_err();
        match err {
            GeminiError::Blocked(detail) => {
                assert!(detail.contains("SAFETY"), "detail={detail}");
                assert!(
                    detail.contains("HARM_CATEGORY_DANGEROUS_CONTENT=MEDIUM"),
                    "detail={detail}"
                );
            }
            other => panic!("очікувано Blocked, отримано {other:?}"),
        }
    }

    #[test]
    fn extract_finish_reason_prohibited_content() {
        let json = r#"{"candidates":[{"finishReason":"PROHIBITED_CONTENT"}]}"#;
        let err = extract_response_text(json).unwrap_err();
        assert!(matches!(err, GeminiError::Blocked(_)));
        assert!(err.to_string().contains("PROHIBITED_CONTENT"));
    }

    #[test]
    fn extract_finish_reason_max_tokens() {
        let json = r#"{"candidates":[{"finishReason":"MAX_TOKENS","content":{"parts":[{"text":"обрізано"}]}}]}"#;
        let err = extract_response_text(json).unwrap_err();
        assert!(matches!(err, GeminiError::Truncated));
        assert!(err.to_string().contains("MAX_TOKENS"));
    }

    #[test]
    fn extract_finish_reason_recitation() {
        let json = r#"{"candidates":[{"finishReason":"RECITATION"}]}"#;
        let err = extract_response_text(json).unwrap_err();
        assert!(err.to_string().contains("recitation"));
    }

    #[test]
    fn extract_finish_reason_unknown() {
        let json = r#"{"candidates":[{"finishReason":"MALFORMED_FUNCTION_CALL"}]}"#;
        let err = extract_response_text(json).unwrap_err();
        assert!(matches!(err, GeminiError::FinishReason(_)));
        assert!(err.to_string().contains("MALFORMED_FUNCTION_CALL"));
    }

    #[test]
    fn extract_block_reason_without_candidates() {
        let json = r#"{"promptFeedback":{"blockReason":"SAFETY"},"candidates":[]}"#;
        let err = extract_response_text(json).unwrap_err();
        match err {
            GeminiError::Blocked(reason) => assert_eq!(reason, "SAFETY"),
            other => panic!("очікувано Blocked, отримано {other:?}"),
        }
    }

    #[test]
    fn extract_no_candidates() {
        let json = r#"{"candidates":[]}"#;
        assert!(matches!(
            extract_response_text(json).unwrap_err(),
            GeminiError::NoCandidates
        ));
    }

    #[test]
    fn extract_empty_text() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":""}]}}]}"#;
        assert!(matches!(
            extract_response_text(json).unwrap_err(),
            GeminiError::EmptyText
        ));
    }
}
