//! Standby-адаптер КНИГИ ПОСТАЧАЛЬНИКА (`state.ledger`) — ADR-0007 §11.7.9.7
//! (Фаза 3.3b).
//!
//! `POST /api/v1/ledger` і `POST /api/v2/ledger/entries` — **самостійний
//! документ** (ручний запис книги: `operation_type` + `amount` + `notes`), а не
//! похідна накладної чи повернення. Тому сутність має ВЛАСНИЙ тип черги
//! (`TYPE_SUPPLIER_LEDGER`) з локальним агрегатом `supplier_ledger` (0012) і
//! похідним станом `supplier_balances` — на standby запис стає можливим
//! офлайн, замість сирого `500` (INSERT у read-only репліку).
//!
//! `balance_after` у відповіді = баланс репліки (читання дозволене, §10) +
//! сума несинхронізованих записів каси (включно з цим). Це ТА САМА
//! арифметика, яку виконає приймач на primary (`SUM(amount) + amount`).
//! Похибка можлива лише якщо паралельний запит каси додасть запис між
//! читанням і записом — авторитетне значення рахує primary.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use torgashka_domain::{
    LedgerBalanceV1Dto, LedgerBalanceV2Dto, LedgerEntriesQuery, LedgerEntryInput, LedgerEntryV1Dto,
    LedgerEntryV2Dto, LedgerError, LedgerHistoryV1Dto, LedgerListV2Dto, LedgerService,
    SupplierBalanceV2Dto,
};

use crate::offline::transactions::TYPE_SUPPLIER_LEDGER;

use super::outbox_local as q;

/// Типи операцій v1 (еталон Python `LedgerService`): невідомий → 400.
const LEDGER_TYPES_V1: [&str; 4] = ["invoice", "payment", "return", "correction"];

/// Standby-адаптер книги постачальника: читання → `inner`, запис → черга.
pub struct OutboxLedger {
    inner: Arc<dyn LedgerService + Send + Sync>,
}

impl OutboxLedger {
    pub fn new(inner: Arc<dyn LedgerService + Send + Sync>) -> Self {
        Self { inner }
    }

    /// Спільний крок обох create: перевірка постачальника (404) + тип (400) +
    /// запис у чергу → `(client_uuid, balance_after_cents)`, як копійки.
    async fn enqueue_entry(
        &self,
        input: &LedgerEntryInput,
        types: &[&str],
    ) -> Result<(Uuid, i64), LedgerError> {
        if !types.contains(&input.operation_type.as_str()) {
            return Err(LedgerError::BadRequest(format!(
                "Невідомий тип операції: '{}'",
                input.operation_type
            )));
        }
        // 1. Постачальник мусить існувати (balance_v1 → 404 як сервіс).
        let balance = self.inner.balance_v1(input.supplier_id).await?;
        let base = q::parse_cents2(&balance.current_balance).unwrap_or(0);
        let amount = q::parse_cents2(&input.amount).ok_or_else(|| {
            LedgerError::BadRequest(format!("Некоректна сума: '{}'", input.amount))
        })?;
        if amount == 0 {
            return Err(LedgerError::BadRequest(
                "Сума запису мусить бути не нульовою".to_string(),
            ));
        }
        // 2. Сума несинхронізованих записів ДО цього (локальна оцінка каси).
        let pending = q::pending_ledger_cents(&input.supplier_id.to_string())
            .await
            .map_err(LedgerError::Infrastructure)?;
        // 3. Документ у чергу: агрегат + outbox + похідний баланс (одна транзакція).
        let out = q::enqueue(
            "запис книги постачальника",
            TYPE_SUPPLIER_LEDGER,
            json!({
                "supplier_id": input.supplier_id,
                "amount": input.amount,
                "operation_type": input.operation_type,
                "document_id": input.document_id,
                "document_number": input.document_number,
                "operation_date": input
                    .operation_date
                    .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string()),
                "notes": input.notes,
            }),
        )
        .await
        .map_err(LedgerError::Infrastructure)?;
        Ok((q::uuid_of(&out.client_uuid), base + pending + amount))
    }
}

#[async_trait]
impl LedgerService for OutboxLedger {
    async fn create_entry_v1(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV1Dto, LedgerError> {
        let (client_uuid, balance_after) = self.enqueue_entry(input, &LEDGER_TYPES_V1).await?;
        let op_date = input
            .operation_date
            .unwrap_or_else(|| Utc::now().naive_utc());
        Ok(LedgerEntryV1Dto {
            id: client_uuid,
            supplier_id: input.supplier_id,
            operation_type: input.operation_type.clone(),
            document_id: input.document_id,
            document_number: input.document_number.clone(),
            // Як сервіс: amount зі scale вводу.
            amount: input.amount.clone(),
            balance_after: q::format_cents2(balance_after),
            operation_date: op_date,
            notes: input.notes.clone(),
            created_at: Utc::now().naive_utc(),
        })
    }

    async fn history_v1(
        &self,
        supplier_id: Uuid,
        page: i64,
        size: i64,
    ) -> Result<LedgerHistoryV1Dto, LedgerError> {
        self.inner.history_v1(supplier_id, page, size).await
    }

    async fn balance_v1(&self, supplier_id: Uuid) -> Result<LedgerBalanceV1Dto, LedgerError> {
        self.inner.balance_v1(supplier_id).await
    }

    async fn list_entries_v2(
        &self,
        q: &LedgerEntriesQuery,
    ) -> Result<LedgerListV2Dto, LedgerError> {
        self.inner.list_entries_v2(q).await
    }

    async fn create_entry_v2(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV2Dto, LedgerError> {
        // v2 приймає на один тип більше (`write_off`) і не має operation_date.
        let (client_uuid, balance_after) = self
            .enqueue_entry(
                input,
                &["invoice", "payment", "return", "correction", "write_off"],
            )
            .await?;
        let amount = q::parse_cents2(&input.amount).unwrap_or(0) as f64 / 100.0;
        Ok(LedgerEntryV2Dto {
            id: client_uuid,
            supplier_id: input.supplier_id,
            amount,
            operation_type: input.operation_type.clone(),
            balance_after: Some(balance_after as f64 / 100.0),
            created_at: Some(Utc::now().naive_utc()),
            document_id: input.document_id,
            document_number: input.document_number.clone().unwrap_or_default(),
            notes: input.notes.clone().unwrap_or_default(),
        })
    }

    async fn balance_v2(&self, supplier_id: Uuid) -> Result<LedgerBalanceV2Dto, LedgerError> {
        self.inner.balance_v2(supplier_id).await
    }

    async fn all_balances_v2(&self) -> Result<Vec<SupplierBalanceV2Dto>, LedgerError> {
        self.inner.all_balances_v2().await
    }
}
