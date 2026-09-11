//! Документи (етап 8 — група 2): об'єднаний список, batch-confirm, delete,
//! copy, export (Excel/CSV), print.
//!
//! 1:1 з Python v1/documents.py (1793 рядки):
//!   - GET    /documents                     — об'єднаний список 6 типів
//!   - POST   /documents/batch-confirm       — пакетне підтвердження
//!   - DELETE /documents/{id}?document_type= — видалення чернетки
//!   - POST   /documents/{id}/copy?document_type= — копіювання документа
//!   - GET    /documents/export              — Excel/CSV експорт (flat+detailed)
//!   - GET    /documents/{id}/print?document_type= — дані для друку
//!
//! Грошові поля — String (Decimal), як у решті Rust-портів; для list —
//! f64 (Python list_documents конвертує Decimal у float).
//! Складні відповіді (copy/print) — serde_json::Value (1:1 з Python dict).

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Помилки документів (1:1 з HTTP-статусами Python).
#[derive(Debug, thiserror::Error)]
pub enum DocumentsError {
    /// 404 — документ не знайдено.
    #[error("{0}")]
    NotFound(String),
    /// 400 — бізнес-валідація.
    #[error("{0}")]
    BadRequest(String),
    /// 500 — помилка БД.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

/// Елемент об'єднаного списку документів (Python list_documents dict).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DocumentDto {
    pub id: String,
    pub document_type: String,
    pub document_number: String,
    pub status: String,
    /// Python: `float(x) if x else 0` → int 0 для нуля, float інакше.
    pub total_amount: serde_json::Value,
    pub purchase_total: Option<serde_json::Value>,
    pub supplier_name: String,
    pub supplier_id: Option<String>,
    pub created_at: Option<String>,
    pub created_by: String,
    pub created_by_name: String,
    /// Python inventory: deviation_total (тільки для document_type=inventory).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deviation_total: Option<f64>,
}

/// Параметри списку документів.
#[derive(Debug, Clone, Default)]
pub struct DocListQuery {
    pub page: i64,
    pub size: i64,
    pub status: Option<String>,
    pub document_type: Option<String>,
    pub search: Option<String>,
    pub date_from: Option<NaiveDateTime>,
    pub date_to: Option<NaiveDateTime>,
    pub supplier_id: Option<Uuid>,
    pub amount_from: Option<f64>,
    pub amount_to: Option<f64>,
}

/// Відповідь списку документів.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DocListDto {
    pub items: Vec<DocumentDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

/// Запит на пакетне підтвердження.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchConfirmInput {
    pub document_type: String,
    pub ids: Vec<String>,
}

/// Помилка по конкретному ID (Python batch-confirm errors list).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BatchConfirmErrorDto {
    pub id: String,
    pub error: String,
}

/// Відповідь пакетного підтвердження.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BatchConfirmResultDto {
    pub confirmed_count: i64,
    pub errors: Vec<BatchConfirmErrorDto>,
}

/// Параметри експорту.
#[derive(Debug, Clone, Default)]
pub struct ExportQuery {
    pub ids: Vec<Uuid>,
    pub status: Option<String>,
    pub document_type: Option<String>,
    pub search: Option<String>,
    pub date_from: Option<NaiveDateTime>,
    pub date_to: Option<NaiveDateTime>,
    pub supplier_id: Option<Uuid>,
    pub amount_from: Option<f64>,
    pub amount_to: Option<f64>,
    pub detailed: bool,
}

/// Дані експорту (заголовки + рядки).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Дані для друку (Python DocumentPrintData).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DocPrintDto {
    pub header: Value,
    pub items: Vec<Value>,
    pub footer: Value,
}

/// Контракт репозиторію/сервісу документів.
#[async_trait::async_trait]
pub trait DocumentsService: Send + Sync {
    async fn list_documents(&self, q: &DocListQuery) -> Result<DocListDto, DocumentsError>;
    async fn batch_confirm(
        &self,
        input: &BatchConfirmInput,
        user_id: Uuid,
    ) -> Result<BatchConfirmResultDto, DocumentsError>;
    async fn delete_document(&self, id: Uuid, document_type: &str) -> Result<(), DocumentsError>;
    async fn copy_document(
        &self,
        id: Uuid,
        document_type: &str,
        user_id: Uuid,
    ) -> Result<Value, DocumentsError>;
    async fn export_documents(&self, q: &ExportQuery) -> Result<ExportData, DocumentsError>;
    async fn print_document(
        &self,
        id: Uuid,
        document_type: &str,
    ) -> Result<DocPrintDto, DocumentsError>;
}

#[async_trait::async_trait]
impl<T: DocumentsService + ?Sized> DocumentsService for std::sync::Arc<T> {
    async fn list_documents(&self, q: &DocListQuery) -> Result<DocListDto, DocumentsError> {
        (**self).list_documents(q).await
    }
    async fn batch_confirm(
        &self,
        input: &BatchConfirmInput,
        user_id: Uuid,
    ) -> Result<BatchConfirmResultDto, DocumentsError> {
        (**self).batch_confirm(input, user_id).await
    }
    async fn delete_document(&self, id: Uuid, document_type: &str) -> Result<(), DocumentsError> {
        (**self).delete_document(id, document_type).await
    }
    async fn copy_document(
        &self,
        id: Uuid,
        document_type: &str,
        user_id: Uuid,
    ) -> Result<Value, DocumentsError> {
        (**self).copy_document(id, document_type, user_id).await
    }
    async fn export_documents(&self, q: &ExportQuery) -> Result<ExportData, DocumentsError> {
        (**self).export_documents(q).await
    }
    async fn print_document(
        &self,
        id: Uuid,
        document_type: &str,
    ) -> Result<DocPrintDto, DocumentsError> {
        (**self).print_document(id, document_type).await
    }
}
