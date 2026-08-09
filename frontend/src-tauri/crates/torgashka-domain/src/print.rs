//! Друк: цінники/етикетки/тестовий друк (етап 8 — група 6) + шаблони друку.
//!
//! 1:1 з Python:
//!   - backend/app/api/v1/print.py (4 роути, 719 рядків): price-tags/render,
//!     labels/render, printers (CUPS lpstat -e), test (receipt/price_tag/label)
//!   - backend/app/api/v1/print_templates.py (9 роутів, 315 рядків): CRUD
//!     шаблонів + default/set-default/render
//!   - services: price_tag_print_service.py (850 — вже мігровано у
//!     repositories/price_tag.rs, група 3), print_template_service.py (117),
//!     print_font_service.py (227), use_cases/invoice_print_use_cases.py (277 —
//!     вже мігровано у price_tag.rs, група 3)
//!
//! РІШЕННЯ щодо Jinja2: у БД усі 9 шаблонів (receipt_58mm/80mm,
//! return_receipt_58mm, price_tag, label, custom) використовують ТІЛЬКИ
//! `{{variable}}`-плейсхолдери. Python PrintTemplateService.render_template —
//! простий str.replace("{{key}}", value) у порядку dict, БЕЗ Jinja2-фіч
//! (цикли/макроси/filters відсутні). Тому minijinja НЕ потрібен: рендер —
//! 1:1 replace.
//!
//! ESC/POS: у групі 6 Python НЕ генерує ESC/POS-байти — усе HTML.
//! print_mode=escpos лише обмежує ширину етикетки (48 мм) у
//! render_labels_sequential (вже мігровано в price_tag.rs, група 3).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Помилки друку/шаблонів (1:1 з HTTP-статусами Python).
#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    /// 404.
    #[error("{0}")]
    NotFound(String),
    /// 400 — бізнес-валідація.
    #[error("{0}")]
    BadRequest(String),
    /// 500 — помилка БД.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

// ─── DTO шаблонів друку (Python schemas/print_template.py) ────────────────

/// Шаблон друку (Python PrintTemplateResponse).
#[derive(Debug, Clone, Serialize)]
pub struct PrintTemplateDto {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub content: String,
    pub variables: Option<Value>,
    pub is_default: bool,
    pub is_active: bool,
    /// Python Pydantic UTC: "2026-07-26T00:06:20.863242Z" (6 цифр мікросекунд).
    pub created_at: String,
    pub updated_at: String,
}

/// Список активних шаблонів з пагінацією (Python list_active_templates).
#[derive(Debug, Clone, Serialize)]
pub struct PrintTemplateListDto {
    pub items: Vec<PrintTemplateDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

/// Створення шаблону (Python PrintTemplateCreate).
#[derive(Debug, Clone, Deserialize)]
pub struct PrintTemplateCreateInput {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub type_: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub variables: Option<Value>,
    #[serde(default)]
    pub is_default: bool,
}

/// Оновлення шаблону (Python PrintTemplateUpdate, exclude_unset).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PrintTemplateUpdateInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
}

/// Запит рендеру шаблону (Python TemplateRenderRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct TemplateRenderInput {
    pub data: Value,
}

/// Відповідь рендеру (Python TemplateRenderResponse).
#[derive(Debug, Clone, Serialize)]
pub struct TemplateRenderDto {
    pub html: String,
}

// ─── DTO цінників/етикеток (Python schemas/print.py) ──────────────────────

/// Товар для друку цінника/етикетки (Python PriceTagProduct).
#[derive(Debug, Clone, Deserialize)]
pub struct PrintProductInput {
    pub id: Uuid,
    pub title: String,
    pub price: String,
    #[serde(default)]
    pub barcode: Option<String>,
    #[serde(default)]
    pub article: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub created_date: Option<String>,
    #[serde(default = "default_copies")]
    pub copies: i64,
}

fn default_copies() -> i64 {
    1
}

/// Рендер цінників A4 (Python PriceTagRenderRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct PriceTagRenderInput {
    pub template_id: Uuid,
    pub products: Vec<PrintProductInput>,
    #[serde(default = "d40")]
    pub width_mm: f64,
    #[serde(default = "d25")]
    pub height_mm: f64,
    #[serde(default = "d3")]
    pub gap_mm: f64,
    #[serde(default = "d10")]
    pub margin_mm: f64,
    #[serde(default = "d_code128")]
    pub barcode_type: String,
    #[serde(default = "d12")]
    pub barcode_height_mm: f64,
}

/// Відповідь рендеру цінників (Python PriceTagRenderResponse).
#[derive(Debug, Clone, Serialize)]
pub struct PriceTagRenderDto {
    pub html: String,
    pub total_pages: i64,
    pub total_labels: i64,
}

/// Рендер етикеток (Python LabelRenderRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct LabelRenderInput {
    pub template_id: Uuid,
    pub products: Vec<PrintProductInput>,
    #[serde(default = "d58")]
    pub width_mm: f64,
    #[serde(default = "d40")]
    pub height_mm: f64,
    #[serde(default = "d2")]
    pub gap_mm: f64,
    #[serde(default = "d_code128")]
    pub barcode_type: String,
    #[serde(default = "d12")]
    pub barcode_height_mm: f64,
    #[serde(default = "d_system")]
    pub print_mode: String,
}

/// Відповідь рендеру етикеток (Python LabelRenderResponse).
#[derive(Debug, Clone, Serialize)]
pub struct LabelRenderDto {
    pub html: String,
    pub total_labels: i64,
}

/// Тестовий друк (Python TestPrintRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct TestPrintInput {
    #[serde(default = "d_receipt")]
    pub print_type: String,
    #[serde(default)]
    pub printer_name: String,
    #[serde(default = "d_receipt_58")]
    pub template_type: String,
    #[serde(default)]
    pub template_id: Option<Uuid>,
    #[serde(default)]
    pub width_mm: Option<f64>,
    #[serde(default)]
    pub height_mm: Option<f64>,
    #[serde(default)]
    pub gap_mm: Option<f64>,
    #[serde(default)]
    pub margin_mm: Option<f64>,
    #[serde(default = "d_code128")]
    pub barcode_type: String,
    #[serde(default = "d12")]
    pub barcode_height_mm: f64,
}

/// Відповідь тестового друку (Python TestPrintResponse).
#[derive(Debug, Clone, Serialize)]
pub struct TestPrintDto {
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_name: Option<String>,
}

fn d40() -> f64 {
    40.0
}
fn d25() -> f64 {
    25.0
}
fn d3() -> f64 {
    3.0
}
fn d10() -> f64 {
    10.0
}
fn d58() -> f64 {
    58.0
}
fn d2() -> f64 {
    2.0
}
fn d12() -> f64 {
    12.0
}
fn d_code128() -> String {
    "code128".to_string()
}
fn d_system() -> String {
    "system".to_string()
}
fn d_receipt() -> String {
    "receipt".to_string()
}
fn d_receipt_58() -> String {
    "receipt_58mm".to_string()
}

// ─── Контракт ───────────────────────────────────────────────────────────────

/// Сервіс шаблонів друку + рендерів (1:1 Python print.py + print_templates.py).
#[async_trait::async_trait]
pub trait PrintTemplatesService: Send + Sync {
    /// GET /print-templates (list active, пагінація).
    async fn list_active(&self, page: i64, size: i64) -> Result<PrintTemplateListDto, PrintError>;
    /// GET /print-templates/all (admin; всі, включаючи неактивні).
    async fn list_all(&self) -> Result<Vec<PrintTemplateDto>, PrintError>;
    /// GET /print-templates/default?type= (is_default → перший активний).
    async fn get_default(&self, type_: &str) -> Result<PrintTemplateDto, PrintError>;
    /// GET /print-templates/{id}.
    async fn get(&self, id: Uuid) -> Result<PrintTemplateDto, PrintError>;
    /// POST /print-templates (201).
    async fn create(
        &self,
        input: &PrintTemplateCreateInput,
    ) -> Result<PrintTemplateDto, PrintError>;
    /// PUT /print-templates/{id} (exclude_unset).
    async fn update(
        &self,
        id: Uuid,
        input: &PrintTemplateUpdateInput,
    ) -> Result<PrintTemplateDto, PrintError>;
    /// DELETE /print-templates/{id} (soft delete, 204).
    async fn delete(&self, id: Uuid) -> Result<(), PrintError>;
    /// POST /print-templates/{id}/set-default.
    async fn set_default(&self, id: Uuid) -> Result<PrintTemplateDto, PrintError>;
    /// POST /print-templates/{id}/render (replace {{key}} + font).
    async fn render_template(
        &self,
        id: Uuid,
        data: &Value,
    ) -> Result<TemplateRenderDto, PrintError>;
    /// POST /print/price-tags/render.
    async fn render_price_tags(
        &self,
        input: &PriceTagRenderInput,
    ) -> Result<PriceTagRenderDto, PrintError>;
    /// POST /print/labels/render.
    async fn render_labels(&self, input: &LabelRenderInput) -> Result<LabelRenderDto, PrintError>;
    /// POST /print/test.
    async fn test_print(&self, input: &TestPrintInput) -> Result<TestPrintDto, PrintError>;
}
