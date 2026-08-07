//! Сервіси application-шару (етап 8 — група 1: боржники).
//!
//! [`DebtorServiceFacade`] — тонкий фасад над портом [`kasa_domain::DebtorService`].
//! Валідація вхідних даних — на рівні API (kasa-api); тут лише делегування.

use kasa_domain::{
    DebtorCreateInput, DebtorDto, DebtorError, DebtorListDto, DebtorPayInput, DebtorPaymentDto,
    DebtorReceiptDto, DebtorSearchQuery, DebtorService as DebtorPort, DebtorUpdateInput,
};
use uuid::Uuid;

/// Фасад операцій з боржниками. Параметризується реалізацією [`DebtorPort`].
pub struct DebtorServiceFacade<R> {
    repo: R,
}

impl<R: DebtorPort> DebtorServiceFacade<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn search(&self, q: &DebtorSearchQuery) -> Result<Vec<DebtorDto>, DebtorError> {
        self.repo.search(q).await
    }
    pub async fn list(&self, page: i64, size: i64) -> Result<DebtorListDto, DebtorError> {
        self.repo.list(page, size).await
    }
    pub async fn create(&self, input: &DebtorCreateInput) -> Result<DebtorDto, DebtorError> {
        self.repo.create(input).await
    }
    pub async fn get(&self, id: Uuid) -> Result<DebtorDto, DebtorError> {
        self.repo.get(id).await
    }
    pub async fn update(
        &self,
        id: Uuid,
        input: &DebtorUpdateInput,
    ) -> Result<DebtorDto, DebtorError> {
        self.repo.update(id, input).await
    }
    pub async fn pay(&self, id: Uuid, input: &DebtorPayInput) -> Result<DebtorDto, DebtorError> {
        self.repo.pay(id, input).await
    }
    pub async fn receipts(&self, id: Uuid) -> Result<Vec<DebtorReceiptDto>, DebtorError> {
        self.repo.receipts(id).await
    }
    pub async fn payments(&self, id: Uuid) -> Result<Vec<DebtorPaymentDto>, DebtorError> {
        self.repo.payments(id).await
    }
}
