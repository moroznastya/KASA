//! OCR роути — 1:1 Python `ocr.py` + `invoice_ocr.py` (група 9/9).
//!
//! - `POST /api/v1/ocr/invoice` — аналіз зображення накладної (Gemini);
//! - `POST /api/v1/invoice-ocr/analyze` — аналіз + зіставлення товарів з БД.
//!
//! Формат відповіді (успіх): `{"success": true, "data": {...}}`;
//! помилка: `{"success": false, "error": "..."}` (1:1 Python).

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    Extension, Json,
};
use serde_json::{json, Value};

use crate::AppState;

/// Дозволені MIME-типи — відсортовано як Python `sorted(ALLOWED_IMAGE_TYPES)`.
const ALLOWED_IMAGE_TYPES: [&str; 5] = [
    "image/bmp",
    "image/jpeg",
    "image/png",
    "image/tiff",
    "image/webp",
];

/// Перевірка MIME — 1:1 Python `file.content_type not in ALLOWED_IMAGE_TYPES`.
pub fn is_allowed_image_type(content_type: &str) -> bool {
    ALLOWED_IMAGE_TYPES.contains(&content_type)
}

/// Витягує (content_type, bytes) файлу з multipart — 1:1 Python `UploadFile`.
async fn read_image_field(
    multipart: &mut Multipart,
) -> Result<(String, Vec<u8>), (StatusCode, String)> {
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.file_name().is_some() {
            let ct = field.content_type().unwrap_or("").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Помилка читання файлу: {e}"),
                    )
                })?
                .to_vec();
            return Ok((ct, bytes));
        }
    }
    Err((
        StatusCode::BAD_REQUEST,
        "Файл не передано (очікується multipart поле file)".to_string(),
    ))
}

/// Валідація MIME + порожнього файлу — 1:1 Python кроки 1–2.
fn validate_image(content_type: &str, data: &[u8]) -> Result<(), (StatusCode, String)> {
    if !is_allowed_image_type(content_type) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Непідтримуваний тип файлу '{content_type}'. Дозволені типи: {}",
                ALLOWED_IMAGE_TYPES.join(", ")
            ),
        ));
    }
    if data.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Файл порожній".to_string()));
    }
    Ok(())
}

/// POST /api/v1/ocr/invoice — 1:1 Python `ocr.analyze_invoice`.
pub async fn analyze_invoice(
    State(state): State<AppState>,
    Extension(_claims): Extension<crate::auth::Claims>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, String)> {
    let ocr = state.ocr.as_ref().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "OCR не увімкнено (TORGASHKA_RUST_OCR=1)".to_string(),
        )
    })?;
    let (content_type, image_data) = read_image_field(&mut multipart).await?;
    validate_image(&content_type, &image_data)?;

    match ocr.analyze_invoice_image(&content_type, &image_data).await {
        Ok(data) => Ok(Json(json!({ "success": true, "data": data }))),
        Err(torgashka_ocr::OcrError::Runtime(msg)) => {
            // 1:1 Python except RuntimeError → {"success": false, "error": str(e)}
            Ok(Json(json!({ "success": false, "error": msg })))
        }
        Err(e) => {
            // 1:1 Python except Exception → "Внутрішня помилка сервера: {e}"
            Ok(Json(json!({
                "success": false,
                "error": format!("Внутрішня помилка сервера: {e}")
            })))
        }
    }
}

/// POST /api/v1/invoice-ocr/analyze — 1:1 Python `invoice_ocr.analyze_invoice_with_matching`.
pub async fn analyze_with_matching(
    State(state): State<AppState>,
    Extension(_claims): Extension<crate::auth::Claims>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, String)> {
    let pool = state.ocr_pool.clone().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "OCR не увімкнено (TORGASHKA_RUST_OCR=1)".to_string(),
        )
    })?;
    let (content_type, image_data) = read_image_field(&mut multipart).await?;
    validate_image(&content_type, &image_data)?;

    let repo = torgashka_infrastructure::ocr::SqlxOcrRepository::new(pool);
    let service = torgashka_ocr::InvoiceOcrService::new(repo);
    match service.analyze_and_match(&content_type, &image_data).await {
        Ok(v) => Ok(Json(v)),
        Err(torgashka_ocr::OcrError::Runtime(msg)) => {
            Ok(Json(json!({ "success": false, "error": msg })))
        }
        Err(e) => Ok(Json(json!({
            "success": false,
            "error": format!("Внутрішня помилка сервера: {e}")
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_allowed() {
        assert!(is_allowed_image_type("image/jpeg"));
        assert!(is_allowed_image_type("image/png"));
        assert!(is_allowed_image_type("image/webp"));
        assert!(is_allowed_image_type("image/bmp"));
        assert!(is_allowed_image_type("image/tiff"));
    }

    #[test]
    fn mime_rejected() {
        assert!(!is_allowed_image_type("application/pdf"));
        assert!(!is_allowed_image_type("text/plain"));
        assert!(!is_allowed_image_type(""));
        assert!(!is_allowed_image_type("image/gif"));
    }

    #[test]
    fn allowed_types_sorted_like_python() {
        // Python: ", ".join(sorted(ALLOWED_IMAGE_TYPES))
        let joined = ALLOWED_IMAGE_TYPES.join(", ");
        assert_eq!(
            joined,
            "image/bmp, image/jpeg, image/png, image/tiff, image/webp"
        );
    }
}
