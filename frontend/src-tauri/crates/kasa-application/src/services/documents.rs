//! Сервіси application-шару (етап 8 — група 2: документи).
//!
//! [`DocumentsServiceFacade`] — тонкий фасад над портом
//! [`kasa_domain::DocumentsService`]. Валідація вхідних даних — на рівні API.

use kasa_domain::{
    BatchConfirmInput, BatchConfirmResultDto, DocListDto, DocListQuery, DocPrintDto,
    DocumentsError, DocumentsService as DocumentsPort, ExportData, ExportQuery,
};
use serde_json::Value;
use uuid::Uuid;

/// Фасад операцій з документами. Параметризується реалізацією [`DocumentsPort`].
pub struct DocumentsServiceFacade<R> {
    repo: R,
}

impl<R: DocumentsPort> DocumentsServiceFacade<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn list(&self, q: &DocListQuery) -> Result<DocListDto, DocumentsError> {
        self.repo.list_documents(q).await
    }
    pub async fn batch_confirm(
        &self,
        input: &BatchConfirmInput,
        user_id: Uuid,
    ) -> Result<BatchConfirmResultDto, DocumentsError> {
        self.repo.batch_confirm(input, user_id).await
    }
    pub async fn delete(&self, id: Uuid, document_type: &str) -> Result<(), DocumentsError> {
        self.repo.delete_document(id, document_type).await
    }
    pub async fn copy(
        &self,
        id: Uuid,
        document_type: &str,
        user_id: Uuid,
    ) -> Result<Value, DocumentsError> {
        self.repo.copy_document(id, document_type, user_id).await
    }
    pub async fn export(&self, q: &ExportQuery) -> Result<ExportData, DocumentsError> {
        self.repo.export_documents(q).await
    }
    pub async fn print(
        &self,
        id: Uuid,
        document_type: &str,
    ) -> Result<DocPrintDto, DocumentsError> {
        self.repo.print_document(id, document_type).await
    }
}
