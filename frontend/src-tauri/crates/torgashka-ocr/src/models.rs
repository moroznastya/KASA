//! Моделі OCR — 1:1 Python-формат `ocr_service.py` / `invoice_ocr_service.py`.

use serde::{Deserialize, Serialize};

/// Товар з накладної (як його розпізнав Gemini).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvoiceItem {
    #[serde(default)]
    pub product_name: String,
    #[serde(default = "default_quantity")]
    pub quantity: f64,
    #[serde(default)]
    pub cost_price: f64,
    #[serde(default)]
    pub barcode: Option<String>,
}

fn default_quantity() -> f64 {
    1.0
}

/// Розпізнана накладна — 1:1 `analyze_invoice_image` (Python dict).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvoiceData {
    #[serde(default)]
    pub document_number: Option<String>,
    #[serde(default)]
    pub invoice_date: Option<String>,
    #[serde(default)]
    pub is_fiscal: bool,
    #[serde(default)]
    pub supplier_name: Option<String>,
    #[serde(default)]
    pub payment_method: Option<String>,
    #[serde(default)]
    pub items: Vec<InvoiceItem>,
}
