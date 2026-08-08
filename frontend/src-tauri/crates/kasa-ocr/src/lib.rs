//! Kasa POS — OCR-модуль (етап 9 міграції → Rust).
//!
//! Розпізнавання прибуткових накладних через Gemini API:
//! - [`gemini`] — HTTP-клієнт `models.generateContent` (reqwest),
//!   ретраї/ротація ключів 1:1 Python `OCRService`;
//! - [`ocr_service`] — `analyze_invoice_image` (промпт + зображення → JSON);
//! - [`invoice_ocr`] — `analyze_and_match` (OCR + зіставлення товарів з БД);
//! - [`repository`] — абстракція БД (barcode/name lookup) + InMemory.
//!
//! Стратегія: ADR-014. Еталон: `backend/app/infrastructure/services/
//! ocr_service.py` + `invoice_ocr_service.py` (Python).

pub mod gemini;
pub mod invoice_ocr;
pub mod models;
pub mod ocr_service;
pub mod repository;

pub use gemini::GeminiClient;
pub use invoice_ocr::InvoiceOcrService;
pub use models::{InvoiceData, InvoiceItem};
pub use ocr_service::{OcrError, OcrService};
pub use repository::{InMemoryOcrRepository, OcrRepoError, OcrRepository, ProductLookup};
