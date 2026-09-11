//! Сервіси application-шару (етап 4 — ledger: журнал взаєморозрахунків).
//!
//! [`LedgerServiceFacade`] — тонкий фасад над портом [`torgashka_domain::LedgerService`].
//! Валідація вхідних даних — на рівні API (torgashka-api); тут лише делегування.

use torgashka_domain::{
    LedgerBalanceV1Dto, LedgerBalanceV2Dto, LedgerEntriesQuery, LedgerEntryInput, LedgerEntryV1Dto,
    LedgerEntryV2Dto, LedgerError, LedgerHistoryV1Dto, LedgerListV2Dto,
    LedgerService as LedgerPort, SupplierBalanceV2Dto,
};
use uuid::Uuid;

/// Фасад ledger-операцій. Параметризується реалізацією [`LedgerPort`].
pub struct LedgerServiceFacade<R> {
    repo: R,
}

impl<R: LedgerPort> LedgerServiceFacade<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn create_entry_v1(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV1Dto, LedgerError> {
        self.repo.create_entry_v1(input).await
    }
    pub async fn history_v1(
        &self,
        supplier_id: Uuid,
        page: i64,
        size: i64,
    ) -> Result<LedgerHistoryV1Dto, LedgerError> {
        self.repo.history_v1(supplier_id, page, size).await
    }
    pub async fn balance_v1(&self, supplier_id: Uuid) -> Result<LedgerBalanceV1Dto, LedgerError> {
        self.repo.balance_v1(supplier_id).await
    }
    pub async fn list_entries_v2(
        &self,
        q: &LedgerEntriesQuery,
    ) -> Result<LedgerListV2Dto, LedgerError> {
        self.repo.list_entries_v2(q).await
    }
    pub async fn create_entry_v2(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV2Dto, LedgerError> {
        self.repo.create_entry_v2(input).await
    }
    pub async fn balance_v2(&self, supplier_id: Uuid) -> Result<LedgerBalanceV2Dto, LedgerError> {
        self.repo.balance_v2(supplier_id).await
    }
    pub async fn all_balances_v2(&self) -> Result<Vec<SupplierBalanceV2Dto>, LedgerError> {
        self.repo.all_balances_v2().await
    }
}
