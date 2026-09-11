//! `OutboxInvoicesV1` / `OutboxInvoicesV2` — standby-адаптери прибуткової
//! накладної каси (ADR-0007 §10, §11.1, §11.7.9.7; Фаза 3.3a).
//!
//! До виправлення: на standby `state.invoices_v1/v2` = `SqlxInvoices(репліка)`,
//! тому `POST /api/v1/invoices` та `POST /api/v2/invoices` падали з PG-помилкою
//! `cannot execute INSERT in a read-only transaction` → сирий **500**.
//! §11.1 класифікує `invoice` як **`LocalOutbox`**: документ каси кладеться в
//! SQLite-чергу (`offline::transactions::enqueue_invoice`) тією самою
//! транзакцією, що й локальний stock-ефект; доставку робить push-цикл
//! (приймач — `sync.rs::accept_invoice_kind`, partial UNIQUE 0016).
//!
//! Правила адаптера (як `outbox_pos`):
//!   * ЧИТАННЯ — делегат `inner` (репліка, §10);
//!   * ЗАПИС — черга SQLite; `PgPool` у структурі немає;
//!   * update/delete/confirm/cancel документа — явна людська відмова
//!     (`transactions.rs` не має типу «update», а приймач знає лише INSERT;
//!     «update новим INSERT» = дубль документа на primary + подвійний stock).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{NaiveDateTime, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use torgashka_domain::invoices::{
    InvoiceCreateV1Input, InvoiceCreateV2Input, InvoiceItemV1Dto, InvoiceItemV2Dto,
    InvoicePaymentInfoV1Dto, InvoicePaymentInfoV2Dto, InvoicePrintDto, InvoicePrintRequest,
    InvoiceUpdateV1Input, InvoiceUpdateV2Input, InvoiceV1Dto, InvoiceV1ListDto, InvoiceV2Dto,
    InvoiceV2ListDto, InvoicesError, InvoicesV1Service, InvoicesV2Service, PriceChangeItemDto,
};

use crate::offline::transactions::EnqueuedTransaction;

use super::outbox_local as q;

/// Payload накладної для черги: формат `InvoiceCreateV1Input` — приймач
/// розбирає його тим самим `serde_json::from_value::<InvoiceCreateV1Input>`
/// (`sync.rs:760`), тож v1-поверхня передається як є, без перекодувань.
fn v1_payload(input: &InvoiceCreateV1Input) -> Value {
    let items: Vec<Value> = input
        .items
        .iter()
        .map(|it| {
            let mut o = json!({
                "product_id": it.product_id,
                "quantity": q::num(&it.quantity),
                "price": q::num(&it.price),
                "total": q::num(&it.total),
            });
            if let Some(c) = &it.cost_price {
                o["cost_price"] = q::num(c);
            }
            if let Some(m) = &it.markup_percent {
                o["markup_percent"] = q::num(m);
            }
            o
        })
        .collect();
    json!({
        "number": input.number,
        "supplier_id": input.supplier_id,
        "invoice_date": input.invoice_date.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "payment_method": input.payment_method,
        "is_fiscal": input.is_fiscal,
        "notes": input.notes,
        "total_amount": input.total_amount,
        "items": items,
    })
}

/// `/api/v2/invoices` (створення) має власний вхідний DTO; приймач накладної
/// на primary ОДИН — v1. Тому v2 приводиться до v1-формату: `tax_rate` v2 має
/// значення за замовчуванням і в v1-контурі (та в локальній таблиці
/// `invoice_items`) поля немає — втрати документа немає, лише це поле.
fn v2_as_v1_payload(input: &InvoiceCreateV2Input) -> Value {
    let items: Vec<Value> = input
        .items
        .iter()
        .map(|it| {
            json!({
                "product_id": it.product_id,
                "quantity": it.quantity,
                "price": it.price,
                "total": it.quantity * it.price,
            })
        })
        .collect();
    json!({
        "number": input.number,
        "supplier_id": input.supplier_id,
        // v1-приймач вимагає `invoice_date`; v2-вхід його не має → дата каси = now().
        "invoice_date": Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        "is_fiscal": false,
        "notes": input.notes,
        "items": items,
    })
}

fn v1_items_dto(input: &InvoiceCreateV1Input, doc_id: Uuid, now: NaiveDateTime) -> Vec<InvoiceItemV1Dto> {
    input
        .items
        .iter()
        .map(|it| InvoiceItemV1Dto {
            id: Uuid::new_v4(),
            invoice_id: doc_id,
            product_id: it.product_id,
            product: None,
            quantity: it.quantity.clone(),
            price: it.price.clone(),
            total: it.total.clone(),
            cost_price: it.cost_price.clone(),
            markup_percent: it.markup_percent.clone(),
            previous_price: None,
            created_at: now,
        })
        .collect()
}

/// DTO черги (v1): ідентифікатор = `client_uuid` конверта, тобто той самий
/// ключ, під яким документ ляже на primary (ідемпотентність push, 0016).
fn queued_v1_dto(input: &InvoiceCreateV1Input, out: EnqueuedTransaction) -> InvoiceV1Dto {
    let now = Utc::now().naive_utc();
    let doc_id = q::uuid_of(&out.client_uuid);
    let total = input.total_amount.clone().unwrap_or_else(|| {
        q::dec_sum(input.items.iter().map(|it| it.total.clone()))
    });
    InvoiceV1Dto {
        id: doc_id,
        number: input
            .number
            .clone()
            .unwrap_or_else(|| format!("ЧЕРГА-{}", out.id)),
        supplier_id: input.supplier_id,
        supplier_name: None,
        invoice_date: input.invoice_date,
        status: q::QUEUED_STATUS.to_string(),
        payment_method: input.payment_method.clone(),
        is_fiscal: input.is_fiscal,
        notes: input.notes.clone(),
        total_amount: Some(total),
        paid_amount: None,
        remaining: None,
        created_at: now,
        updated_at: now,
        items: v1_items_dto(input, doc_id, now),
    }
}

/// DTO черги (v2).
fn queued_v2_dto(input: &InvoiceCreateV2Input, out: EnqueuedTransaction) -> InvoiceV2Dto {
    let now = Utc::now().naive_utc();
    InvoiceV2Dto {
        id: q::uuid_of(&out.client_uuid),
        number: input.number.clone(),
        supplier_id: input.supplier_id,
        items: input
            .items
            .iter()
            .map(|it| InvoiceItemV2Dto {
                product_id: it.product_id,
                quantity: it.quantity,
                price: it.price,
                tax_rate: it.tax_rate,
                name: it.name.clone(),
            })
            .collect(),
        total: Some(input.items.iter().map(|it| it.quantity * it.price).sum()),
        status: q::QUEUED_STATUS.to_string(),
        created_at: Some(now),
        confirmed_at: None,
        notes: input.notes.clone(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// v1-адаптер
// ─────────────────────────────────────────────────────────────────────────────

/// Standby-адаптер v1-накладних: читання → `inner`, створення → черга.
pub struct OutboxInvoicesV1 {
    inner: Arc<dyn InvoicesV1Service + Send + Sync>,
}

impl OutboxInvoicesV1 {
    pub fn new(inner: Arc<dyn InvoicesV1Service + Send + Sync>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl InvoicesV1Service for OutboxInvoicesV1 {
    async fn list_v1(
        &self,
        supplier_id: Option<Uuid>,
        page: i64,
        size: i64,
    ) -> Result<InvoiceV1ListDto, InvoicesError> {
        self.inner.list_v1(supplier_id, page, size).await
    }

    async fn get_v1(&self, id: Uuid) -> Result<InvoiceV1Dto, InvoicesError> {
        self.inner.get_v1(id).await
    }

    async fn create_v1(
        &self,
        input: &InvoiceCreateV1Input,
        _user_id: Uuid,
    ) -> Result<InvoiceV1Dto, InvoicesError> {
        let out = q::enqueue_invoice_payload("створення накладної", v1_payload(input))
            .await
            .map_err(InvoicesError::Infrastructure)?;
        Ok(queued_v1_dto(input, out))
    }

    async fn update_v1(
        &self,
        _id: Uuid,
        _input: &InvoiceUpdateV1Input,
    ) -> Result<InvoiceV1Dto, InvoicesError> {
        Err(InvoicesError::BadRequest(q::unavailable(
            "редагування накладної",
        )))
    }

    async fn delete_v1(&self, _id: Uuid) -> Result<(), InvoicesError> {
        Err(InvoicesError::BadRequest(q::unavailable(
            "видалення накладної",
        )))
    }

    async fn payment_info_v1(&self, id: Uuid) -> Result<InvoicePaymentInfoV1Dto, InvoicesError> {
        self.inner.payment_info_v1(id).await
    }

    async fn confirm_v1(&self, _id: Uuid, _status: &str) -> Result<InvoiceV1Dto, InvoicesError> {
        Err(InvoicesError::BadRequest(q::unavailable(
            "проведення/скасування накладної",
        )))
    }

    async fn price_changes(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError> {
        self.inner.price_changes(id).await
    }

    async fn print_items(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError> {
        self.inner.print_items(id, req).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// v2-адаптер
// ─────────────────────────────────────────────────────────────────────────────

/// Standby-адаптер v2-накладних: читання → `inner`, створення → черга.
pub struct OutboxInvoicesV2 {
    inner: Arc<dyn InvoicesV2Service + Send + Sync>,
}

impl OutboxInvoicesV2 {
    pub fn new(inner: Arc<dyn InvoicesV2Service + Send + Sync>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl InvoicesV2Service for OutboxInvoicesV2 {
    async fn list_v2(
        &self,
        search: Option<String>,
        supplier_id: Option<Uuid>,
        status: Option<String>,
        date_from: Option<NaiveDateTime>,
        date_to: Option<NaiveDateTime>,
        page: i64,
        size: i64,
    ) -> Result<InvoiceV2ListDto, InvoicesError> {
        self.inner
            .list_v2(search, supplier_id, status, date_from, date_to, page, size)
            .await
    }

    async fn get_v2(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        self.inner.get_v2(id).await
    }

    async fn create_v2(&self, input: &InvoiceCreateV2Input) -> Result<InvoiceV2Dto, InvoicesError> {
        let out = q::enqueue_invoice_payload("створення накладної", v2_as_v1_payload(input))
            .await
            .map_err(InvoicesError::Infrastructure)?;
        Ok(queued_v2_dto(input, out))
    }

    async fn confirm_v2(&self, _id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        Err(InvoicesError::BadRequest(q::unavailable(
            "проведення накладної",
        )))
    }

    async fn update_v2(
        &self,
        _id: Uuid,
        _input: &InvoiceUpdateV2Input,
    ) -> Result<InvoiceV2Dto, InvoicesError> {
        Err(InvoicesError::BadRequest(q::unavailable(
            "редагування накладної",
        )))
    }

    async fn delete_v2(&self, _id: Uuid) -> Result<(), InvoicesError> {
        Err(InvoicesError::BadRequest(q::unavailable(
            "видалення накладної",
        )))
    }

    async fn payment_info_v2(&self, id: Uuid) -> Result<InvoicePaymentInfoV2Dto, InvoicesError> {
        self.inner.payment_info_v2(id).await
    }

    async fn price_changes_v2(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError> {
        self.inner.price_changes_v2(id).await
    }

    async fn print_items_v2(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError> {
        self.inner.print_items_v2(id, req).await
    }

    async fn cancel_v2(&self, _id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        Err(InvoicesError::BadRequest(q::unavailable(
            "скасування накладної",
        )))
    }
}
