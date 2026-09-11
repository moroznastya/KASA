//! Standby-адаптер БОРЖНИКІВ (`state.debtors`) — ADR-0007 §11.6.4 варіант 1,
//! §11.7.9.7 (Фаза 3.3b).
//!
//! Рішення NIKO: боргові сутності — клас `LocalOutbox` (борг стає можливим на
//! standby). Реалізовано **оплату боргу** (`pay`) як документ черги
//! `TYPE_DEBTOR_PAYMENT`: локальний агрегат `debtors_ledger` (міграція 0006) +
//! похідний стан `debtor_balances` (0012, сума ще не прийнятих primary оплат) +
//! outbox-запис — в ОДНІЙ SQLite-транзакції.
//!
//! Відповідь `pay` — DTO боржника з репліки (читання дозволене, §10) з боргом,
//! ЗМЕНШЕНИМ на суму локальних (ще не пушнутих) оплат: каса бачить той борг,
//! який сама щойно погасила. Авторитетне значення — на primary після push.
//!
//! `create`/`update` боржника — явна людська відмова (§11.6.2-клас): у черзі
//! немає дії над довідником, а «створити новим INSERT» = дубль боржника на
//! primary (немає ключа ідемпотентності, як `client_uuid` у документа).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use torgashka_domain::{
    DebtorCreateInput, DebtorDto, DebtorError, DebtorListDto, DebtorPayInput, DebtorPaymentDto,
    DebtorReceiptDto, DebtorSearchQuery, DebtorService, DebtorUpdateInput,
};

use crate::offline::transactions::TYPE_DEBTOR_PAYMENT;

use super::outbox_local as q;

/// Standby-адаптер боржників: читання → `inner`, оплата боргу → черга.
pub struct OutboxDebtors {
    inner: Arc<dyn DebtorService + Send + Sync>,
}

impl OutboxDebtors {
    pub fn new(inner: Arc<dyn DebtorService + Send + Sync>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl DebtorService for OutboxDebtors {
    async fn search(&self, q: &DebtorSearchQuery) -> Result<Vec<DebtorDto>, DebtorError> {
        self.inner.search(q).await
    }

    async fn list(&self, page: i64, size: i64) -> Result<DebtorListDto, DebtorError> {
        self.inner.list(page, size).await
    }

    async fn create(&self, _input: &DebtorCreateInput) -> Result<DebtorDto, DebtorError> {
        Err(DebtorError::BadRequest(q::unavailable(
            "створення боржника",
        )))
    }

    async fn get(&self, id: Uuid) -> Result<DebtorDto, DebtorError> {
        self.inner.get(id).await
    }

    async fn update(
        &self,
        _id: Uuid,
        _input: &DebtorUpdateInput,
    ) -> Result<DebtorDto, DebtorError> {
        Err(DebtorError::BadRequest(q::unavailable(
            "редагування боржника",
        )))
    }

    async fn pay(&self, id: Uuid, input: &DebtorPayInput) -> Result<DebtorDto, DebtorError> {
        // 1. Базовий борг — репліка (404 — як Primary-сервіс).
        let mut dto = self.inner.get(id).await?;
        let base_cents = q::parse_cents2(&dto.total_debt).unwrap_or(0);
        // 2. Локальний борг = борг репліки − оплати, які знає лише каса.
        let pending = q::pending_debtor_cents(&id.to_string())
            .await
            .map_err(DebtorError::Infrastructure)?;
        let local_cents = (base_cents - pending).max(0);
        let amount_cents = q::parse_cents2(&input.amount).unwrap_or(0);
        if amount_cents <= 0 {
            return Err(DebtorError::BadRequest(
                "Сума оплати мусить бути більше 0".to_string(),
            ));
        }
        if local_cents <= 0 {
            return Err(DebtorError::BadRequest(
                "У боржника немає боргу".to_string(),
            ));
        }
        if amount_cents > local_cents {
            return Err(DebtorError::BadRequest(format!(
                "Сума оплати ({}) перевищує поточний борг ({})",
                input.amount,
                q::format_cents2(local_cents)
            )));
        }
        // 3. Документ у чергу: агрегат + outbox + похідний борг (одна транзакція).
        q::enqueue(
            "оплата боргу",
            TYPE_DEBTOR_PAYMENT,
            json!({
                "debtor_id": id,
                "amount": input.amount,
                "payment_method": input.payment_method,
            }),
        )
        .await
        .map_err(DebtorError::Infrastructure)?;
        // 4. Відповідь — борг ПІСЛЯ локальної оплати (оцінка каси).
        dto.total_debt = q::format_cents2(local_cents - amount_cents);
        dto.updated_at = Utc::now().naive_utc();
        Ok(dto)
    }

    async fn receipts(&self, id: Uuid) -> Result<Vec<DebtorReceiptDto>, DebtorError> {
        self.inner.receipts(id).await
    }

    async fn payments(&self, id: Uuid) -> Result<Vec<DebtorPaymentDto>, DebtorError> {
        self.inner.payments(id).await
    }
}
