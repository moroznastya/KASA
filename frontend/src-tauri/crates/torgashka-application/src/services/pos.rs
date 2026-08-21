//! Сервіси application-шару (етап 3 — POS: чеки, сесії, списання, переміщення).
//!
//! [`PosServiceFacade`] — тонкий фасад над портом [`torgashka_domain::PosService`].
//! Валідація вхідних даних — на рівні API (torgashka-api); тут лише делегування.

use torgashka_domain::{
    CashOperationCreateInput, CashOperationDto, CashOperationsListDto,
    MySessionsDto, PosError, PosService as PosPort, ProductRecentSalesDto, PrroShiftDto,
    ReceiptCreateInput, ReceiptDto, ReceiptItemDetailDto, ReceiptListDto, ReceiptListQuery,
    ReceiptSearchDto, ReceiptSearchQuery, ReceiptStatsDto, ReceiptV1CreateInput, ReceiptV1Dto,
    ReceiptV1ItemDto, ReceiptV1ListDto, ReceiptV1ListQuery, ReceiptV1SearchDto, ReturnableQtyDto,
    ShiftListDto, TransferCreateInput, TransferDto, TransferListDto, TransferUpdateInput,
    UserSessionsDto, WorkReportDto, WriteOffCreateInput, WriteOffDto, WriteOffListDto,
    WriteOffReasonItem, WriteOffReasonsListDto, WriteOffUpdateInput,
};
use uuid::Uuid;

/// Фасад POS-операцій. Параметризується реалізацією [`PosPort`].
pub struct PosServiceFacade<R> {
    repo: R,
}

impl<R: PosPort> PosServiceFacade<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn create_sale_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        self.repo.create_sale_receipt(input).await
    }
    pub async fn create_return_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        self.repo.create_return_receipt(input).await
    }
    pub async fn create_receipt_v1(
        &self,
        input: &ReceiptV1CreateInput,
    ) -> Result<ReceiptV1Dto, PosError> {
        self.repo.create_receipt_v1(input).await
    }
    pub async fn get_receipt(&self, id: Uuid) -> Result<ReceiptDto, PosError> {
        self.repo.get_receipt(id).await
    }
    pub async fn list_receipts(&self, q: &ReceiptListQuery) -> Result<ReceiptListDto, PosError> {
        self.repo.list_receipts(q).await
    }
    pub async fn today_stats(&self) -> Result<ReceiptStatsDto, PosError> {
        self.repo.today_stats().await
    }
    pub async fn search_receipts(
        &self,
        q: &ReceiptSearchQuery,
    ) -> Result<ReceiptSearchDto, PosError> {
        self.repo.search_receipts(q).await
    }
    pub async fn recent_sales_by_product(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ProductRecentSalesDto>, PosError> {
        self.repo.recent_sales_by_product(query, limit).await
    }
    pub async fn returnable_quantity(
        &self,
        product_id: Uuid,
    ) -> Result<ReturnableQtyDto, PosError> {
        self.repo.returnable_quantity(product_id).await
    }
    pub async fn receipt_items(
        &self,
        receipt_id: Uuid,
    ) -> Result<Vec<ReceiptItemDetailDto>, PosError> {
        self.repo.receipt_items(receipt_id).await
    }
    pub async fn list_receipts_v1(
        &self,
        q: &ReceiptV1ListQuery,
    ) -> Result<ReceiptV1ListDto, PosError> {
        self.repo.list_receipts_v1(q).await
    }
    pub async fn get_receipt_v1(&self, id: Uuid) -> Result<ReceiptV1Dto, PosError> {
        self.repo.get_receipt_v1(id).await
    }
    pub async fn receipt_items_v1(
        &self,
        receipt_id: Uuid,
    ) -> Result<Vec<ReceiptV1ItemDto>, PosError> {
        self.repo.receipt_items_v1(receipt_id).await
    }
    pub async fn search_receipts_v1(
        &self,
        q: &ReceiptSearchQuery,
    ) -> Result<ReceiptV1SearchDto, PosError> {
        self.repo.search_receipts_v1(q).await
    }

    pub async fn my_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<MySessionsDto, PosError> {
        self.repo.my_sessions(user_id, month, year).await
    }
    pub async fn work_report(&self, month: i64, year: i64) -> Result<WorkReportDto, PosError> {
        self.repo.work_report(month, year).await
    }
    pub async fn user_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<UserSessionsDto, PosError> {
        self.repo.user_sessions(user_id, month, year).await
    }
    pub async fn list_write_offs(&self, page: i64, size: i64) -> Result<WriteOffListDto, PosError> {
        self.repo.list_write_offs(page, size).await
    }
    pub async fn get_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError> {
        self.repo.get_write_off(id).await
    }
    pub async fn create_write_off(
        &self,
        input: &WriteOffCreateInput,
    ) -> Result<WriteOffDto, PosError> {
        self.repo.create_write_off(input).await
    }
    pub async fn update_write_off(
        &self,
        id: Uuid,
        input: &WriteOffUpdateInput,
    ) -> Result<WriteOffDto, PosError> {
        self.repo.update_write_off(id, input).await
    }
    pub async fn delete_write_off(&self, id: Uuid) -> Result<(), PosError> {
        self.repo.delete_write_off(id).await
    }
    pub async fn confirm_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError> {
        self.repo.confirm_write_off(id).await
    }
    pub async fn list_write_off_reasons(&self) -> Result<WriteOffReasonsListDto, PosError> {
        self.repo.list_write_off_reasons().await
    }
    pub async fn create_write_off_reason(
        &self,
        name: &str,
    ) -> Result<WriteOffReasonItem, PosError> {
        self.repo.create_write_off_reason(name).await
    }
    pub async fn list_transfers(&self, page: i64, size: i64) -> Result<TransferListDto, PosError> {
        self.repo.list_transfers(page, size).await
    }
    pub async fn get_transfer(&self, id: Uuid) -> Result<TransferDto, PosError> {
        self.repo.get_transfer(id).await
    }
    pub async fn create_transfer(
        &self,
        input: &TransferCreateInput,
    ) -> Result<TransferDto, PosError> {
        self.repo.create_transfer(input).await
    }
    pub async fn update_transfer(
        &self,
        id: Uuid,
        input: &TransferUpdateInput,
    ) -> Result<TransferDto, PosError> {
        self.repo.update_transfer(id, input).await
    }
    pub async fn delete_transfer(&self, id: Uuid) -> Result<(), PosError> {
        self.repo.delete_transfer(id).await
    }
    pub async fn confirm_transfer(&self, id: Uuid, status: &str) -> Result<TransferDto, PosError> {
        self.repo.confirm_transfer(id, status).await
    }
    pub async fn list_shifts(&self, page: i64, size: i64) -> Result<ShiftListDto, PosError> {
        self.repo.list_shifts(page, size).await
    }
    pub async fn open_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        self.repo.open_shift(comment).await
    }
    pub async fn close_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        self.repo.close_shift(comment).await
    }
    pub async fn create_cash_operation(
        &self,
        store_id: Uuid,
        user_id: Uuid,
        input: &CashOperationCreateInput,
    ) -> Result<CashOperationDto, PosError> {
        self.repo.create_cash_operation(store_id, user_id, input).await
    }
    pub async fn list_cash_operations(&self, store_id: Uuid) -> Result<CashOperationsListDto, PosError> {
        self.repo.list_cash_operations(store_id).await
    }
}
