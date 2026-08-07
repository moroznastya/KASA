//! Сервіси application-шару (етап 8 — група 3: інвойси).
//!
//! [`InvoicesV1Facade`] / [`InvoicesV2Facade`] — тонкі фасади над портами
//! [`kasa_domain::InvoicesV1Service`] / [`kasa_domain::InvoicesV2Service`].

use chrono::NaiveDateTime;
use kasa_domain::invoices::{
    InvoiceCreateV1Input, InvoiceCreateV2Input, InvoicePrintDto, InvoicePrintRequest,
    InvoiceUpdateV1Input, InvoiceUpdateV2Input, InvoiceV1Dto, InvoiceV1ListDto, InvoiceV2Dto,
    InvoiceV2ListDto, InvoicesError, InvoicesV1Service, InvoicesV2Service, PriceChangeItemDto,
};
use uuid::Uuid;

/// Фасад v1-інвойсів.
pub struct InvoicesV1Facade<R> {
    repo: R,
}

impl<R: InvoicesV1Service> InvoicesV1Facade<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn list(
        &self,
        supplier_id: Option<Uuid>,
        page: i64,
        size: i64,
    ) -> Result<InvoiceV1ListDto, InvoicesError> {
        self.repo.list_v1(supplier_id, page, size).await
    }
    pub async fn get(&self, id: Uuid) -> Result<InvoiceV1Dto, InvoicesError> {
        self.repo.get_v1(id).await
    }
    pub async fn create(
        &self,
        input: &InvoiceCreateV1Input,
        user_id: Uuid,
    ) -> Result<InvoiceV1Dto, InvoicesError> {
        self.repo.create_v1(input, user_id).await
    }
    pub async fn update(
        &self,
        id: Uuid,
        input: &InvoiceUpdateV1Input,
    ) -> Result<InvoiceV1Dto, InvoicesError> {
        self.repo.update_v1(id, input).await
    }
    pub async fn delete(&self, id: Uuid) -> Result<(), InvoicesError> {
        self.repo.delete_v1(id).await
    }
    pub async fn payment_info(
        &self,
        id: Uuid,
    ) -> Result<kasa_domain::invoices::InvoicePaymentInfoV1Dto, InvoicesError> {
        self.repo.payment_info_v1(id).await
    }
    pub async fn confirm(&self, id: Uuid, status: &str) -> Result<InvoiceV1Dto, InvoicesError> {
        self.repo.confirm_v1(id, status).await
    }
    pub async fn price_changes(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError> {
        self.repo.price_changes(id).await
    }
    pub async fn print_items(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError> {
        self.repo.print_items(id, req).await
    }
}

/// Фасад v2-інвойсів.
pub struct InvoicesV2Facade<R> {
    repo: R,
}

impl<R: InvoicesV2Service> InvoicesV2Facade<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list(
        &self,
        search: Option<String>,
        supplier_id: Option<Uuid>,
        status: Option<String>,
        date_from: Option<NaiveDateTime>,
        date_to: Option<NaiveDateTime>,
        page: i64,
        size: i64,
    ) -> Result<InvoiceV2ListDto, InvoicesError> {
        self.repo
            .list_v2(search, supplier_id, status, date_from, date_to, page, size)
            .await
    }
    pub async fn get(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        self.repo.get_v2(id).await
    }
    pub async fn create(
        &self,
        input: &InvoiceCreateV2Input,
    ) -> Result<InvoiceV2Dto, InvoicesError> {
        self.repo.create_v2(input).await
    }
    pub async fn confirm(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        self.repo.confirm_v2(id).await
    }
    pub async fn update(
        &self,
        id: Uuid,
        input: &InvoiceUpdateV2Input,
    ) -> Result<InvoiceV2Dto, InvoicesError> {
        self.repo.update_v2(id, input).await
    }
    pub async fn delete(&self, id: Uuid) -> Result<(), InvoicesError> {
        self.repo.delete_v2(id).await
    }
    pub async fn payment_info(
        &self,
        id: Uuid,
    ) -> Result<kasa_domain::invoices::InvoicePaymentInfoV2Dto, InvoicesError> {
        self.repo.payment_info_v2(id).await
    }
    pub async fn price_changes(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError> {
        self.repo.price_changes_v2(id).await
    }
    pub async fn print_items(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError> {
        self.repo.print_items_v2(id, req).await
    }
    pub async fn cancel(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        self.repo.cancel_v2(id).await
    }
}
