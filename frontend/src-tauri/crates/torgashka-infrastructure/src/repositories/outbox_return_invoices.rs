//! Standby-адаптер ПОВЕРНЕННЯ ПОСТАЧАЛЬНИКУ (`state.return_invoices`) —
//! ADR-0007 §10, §11.7.9.7 (Фаза 3.3b).
//!
//! НЕ плутати з чеком повернення ПОКУПЦЯ (`return_receipt` → `receipts`,
//! `OutboxPos::create_return_receipt`): тут — документ `return_invoices`
//! власника магазину (товар іде назад постачальнику, stock **−qty**),
//! який на standby раніше давав сирий `500` (INSERT у read-only репліку).
//!
//! Правила адаптера (як `outbox_invoices`/`outbox_pos`):
//!   * ЧИТАННЯ — делегат `inner` (репліка — дозволене джерело читань, §10);
//!   * ЗАПИС — черга SQLite (`transactions::enqueue_transaction`,
//!     `TYPE_RETURN_INVOICE`): агрегат `return_invoices` + outbox-запис +
//!     stock-ефект −qty в ОДНІЙ транзакції; `PgPool` у структурі НЕМАЄ;
//!   * update/delete/confirm/cancel — явна людська відмова (§11.6.2): у
//!     черзі немає дії над документом, а «update новим INSERT» = дубль.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use torgashka_domain::return_invoices::{
    ReturnInvoiceConfirmInput, ReturnInvoiceCreateInput, ReturnInvoiceDto, ReturnInvoiceItemDto,
    ReturnInvoiceListDto, ReturnInvoiceUpdateInput, ReturnInvoicesError, ReturnInvoicesService,
};

use crate::offline::transactions::{self, TYPE_RETURN_INVOICE};

use super::outbox_local as q;

/// Payload документа для черги — вхідний DTO як є: приймач на primary
/// розбирає його ТИМ САМИМ serde-контрактом (`ReturnInvoiceCreateInput`),
/// тому жодних перекодувань немає. `client_uuid` дає конверт (`envelope`).
fn payload(input: &ReturnInvoiceCreateInput) -> Value {
    serde_json::to_value(input).unwrap_or_else(|_| json!({}))
}

/// DTO черги: ідентифікатор = `client_uuid` конверта (той самий, що в outbox і
/// в `return_invoices.client_uuid` на primary), бізнес-статус — `queued`.
fn queued_dto(
    input: &ReturnInvoiceCreateInput,
    out: transactions::EnqueuedTransaction,
) -> ReturnInvoiceDto {
    let now = Utc::now();
    let now_str = now.format("%Y-%m-%dT%H:%M:%S").to_string();
    let doc_id = q::uuid_of(&out.client_uuid);
    let items: Vec<ReturnInvoiceItemDto> = input
        .items
        .iter()
        .map(|it| ReturnInvoiceItemDto {
            id: Uuid::new_v4(),
            return_invoice_id: doc_id,
            product_id: it.product_id,
            product: None,
            quantity: it.quantity.clone(),
            price: it.price.clone(),
            cost_price: it.cost_price.clone(),
            markup_percent: None,
            total: it.total.clone(),
            created_at: now_str.clone(),
        })
        .collect();
    let total_amount = input
        .total_amount
        .clone()
        .unwrap_or_else(|| q::dec_sum(input.items.iter().map(|it| it.total.clone())));
    let number = input
        .number
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("черга-{}", &out.client_uuid[..8]));
    ReturnInvoiceDto {
        id: doc_id,
        number,
        supplier_id: input.supplier_id,
        supplier_name: None,
        return_date: input.return_date.format("%Y-%m-%dT%H:%M:%S").to_string(),
        status: q::QUEUED_STATUS.to_string(),
        return_action: input.return_action.clone(),
        is_fiscal: input.is_fiscal,
        notes: input.notes.clone(),
        total_amount: Some(total_amount),
        exchange_invoice_id: None,
        exchange_invoice: None,
        source_invoice_id: input.source_invoice_id,
        created_at: now_str.clone(),
        updated_at: now_str,
        items,
    }
}

/// Standby-адаптер повернень постачальнику: читання → `inner`, створення → черга.
pub struct OutboxReturnInvoices {
    inner: Arc<dyn ReturnInvoicesService + Send + Sync>,
}

impl OutboxReturnInvoices {
    pub fn new(inner: Arc<dyn ReturnInvoicesService + Send + Sync>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ReturnInvoicesService for OutboxReturnInvoices {
    async fn list(
        &self,
        page: i64,
        size: i64,
    ) -> Result<ReturnInvoiceListDto, ReturnInvoicesError> {
        self.inner.list(page, size).await
    }

    async fn get(&self, id: Uuid) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        self.inner.get(id).await
    }

    async fn create(
        &self,
        input: &ReturnInvoiceCreateInput,
        _user_id: Uuid,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        let out = q::enqueue(
            "створення повернення постачальнику",
            TYPE_RETURN_INVOICE,
            payload(input),
        )
        .await
        .map_err(|e| {
            // Відмова валідації каталогу (ADR-0007 §5 AT-14) — бізнес-помилка.
            if crate::offline::catalog::is_catalog_rejection(&e) {
                ReturnInvoicesError::BadRequest(e)
            } else {
                ReturnInvoicesError::Infrastructure(e)
            }
        })?;
        Ok(queued_dto(input, out))
    }

    async fn update(
        &self,
        _id: Uuid,
        _input: &ReturnInvoiceUpdateInput,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        Err(ReturnInvoicesError::BadRequest(q::unavailable(
            "редагування повернення",
        )))
    }

    async fn delete(&self, _id: Uuid) -> Result<(), ReturnInvoicesError> {
        Err(ReturnInvoicesError::BadRequest(q::unavailable(
            "видалення повернення",
        )))
    }

    async fn confirm(
        &self,
        _id: Uuid,
        _input: &ReturnInvoiceConfirmInput,
        _user_id: Uuid,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        Err(ReturnInvoicesError::BadRequest(q::unavailable(
            "проведення/скасування повернення",
        )))
    }

    async fn cancel(&self, _id: Uuid) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        Err(ReturnInvoicesError::BadRequest(q::unavailable(
            "скасування повернення",
        )))
    }
}
