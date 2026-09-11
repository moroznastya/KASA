//! `OutboxPurchaseOrders` — standby-адаптер замовлень постачальнику
//! (ADR-0007 §10, §11.1, §11.7.9.7; Фаза 3.3a).
//!
//! До виправлення: на standby `state.purchase_orders` = `SqlxPurchaseOrders`
//! (репліка), тому `POST /api/v1/purchase-orders` падав з PG-помилкою
//! `cannot execute INSERT in a read-only transaction` → сирий **500**.
//! §11.1 класифікує `purchase_order` як **`LocalOutbox`**: агрегат + outbox +
//! stock-ефект локально (`transactions::enqueue_transaction`), доставка —
//! push-цикл (приймач `sync_receivers::accept_purchase_order`, UNIQUE 0013).
//!
//! Правила адаптера (як `outbox_pos`): читання → `inner` (репліка, §10);
//! створення документа → SQLite-черга; `PgPool` у структурі немає;
//! update/delete/confirm (їх не вміє ні черга, ні приймач) — явна відмова.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use torgashka_domain::purchase_orders::{
    PurchaseOrderCreateInput, PurchaseOrderDto, PurchaseOrderItemDto, PurchaseOrderListDto,
    PurchaseOrderUpdateInput, PurchaseOrdersError, PurchaseOrdersService,
};

use crate::offline::transactions::{self, EnqueuedTransaction};

use super::outbox_local as q;

/// Payload замовлення для черги: саме ті ключі, які читає приймач
/// `sync_receivers::accept_purchase_order` (`supplier_id`, `order_date`,
/// `expected_date`, `notes`, `is_fiscal`, `items[]`) і локальний stock-ефект
/// (`stock::parse_items` → `product_id` + `quantity`).
fn payload_of(input: &PurchaseOrderCreateInput) -> Value {
    let items: Vec<Value> = input
        .items
        .iter()
        .map(|it| {
            json!({
                "product_id": it.product_id,
                "quantity": q::num(&it.quantity),
                "price": q::num(&it.price),
                "total": q::num(&it.total),
            })
        })
        .collect();
    json!({
        "number": input.number,
        "supplier_id": input.supplier_id,
        "order_date": input.order_date.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "expected_date": input
            .expected_date
            .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string()),
        "is_fiscal": input.is_fiscal,
        "notes": input.notes,
        "total_amount": input.total_amount,
        "items": items,
    })
}

/// DTO черги: `id` = `client_uuid` конверта (той самий ключ, під яким
/// документ ляже на primary — ідемпотентність push).
fn queued_dto(input: &PurchaseOrderCreateInput, out: EnqueuedTransaction) -> PurchaseOrderDto {
    let now = Utc::now().naive_utc();
    let doc_id = q::uuid_of(&out.client_uuid);
    let total = input.total_amount.clone().unwrap_or_else(|| {
        q::dec_sum(input.items.iter().map(|it| it.total.clone()))
    });
    PurchaseOrderDto {
        id: doc_id,
        number: input
            .number
            .clone()
            .unwrap_or_else(|| format!("ЧЕРГА-{}", out.id)),
        supplier_id: input.supplier_id,
        supplier_name: None,
        order_date: input.order_date,
        expected_date: input.expected_date,
        status: q::QUEUED_STATUS.to_string(),
        is_fiscal: input.is_fiscal,
        notes: input.notes.clone(),
        total_amount: Some(total),
        invoice_id: None,
        invoice: None,
        created_at: now,
        updated_at: now,
        items: input
            .items
            .iter()
            .map(|it| PurchaseOrderItemDto {
                id: Uuid::new_v4(),
                purchase_order_id: doc_id,
                product_id: it.product_id,
                product: None,
                quantity: it.quantity.clone(),
                price: it.price.clone(),
                total: it.total.clone(),
                created_at: now,
            })
            .collect(),
    }
}

/// Standby-адаптер замовлень постачальнику: читання → `inner`, створення → черга.
pub struct OutboxPurchaseOrders {
    inner: Arc<dyn PurchaseOrdersService + Send + Sync>,
}

impl OutboxPurchaseOrders {
    pub fn new(inner: Arc<dyn PurchaseOrdersService + Send + Sync>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl PurchaseOrdersService for OutboxPurchaseOrders {
    async fn list(
        &self,
        page: i64,
        size: i64,
    ) -> Result<PurchaseOrderListDto, PurchaseOrdersError> {
        self.inner.list(page, size).await
    }

    async fn get(&self, id: Uuid) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        self.inner.get(id).await
    }

    async fn create(
        &self,
        input: &PurchaseOrderCreateInput,
        _user_id: Uuid,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        let out = q::enqueue(
            "створення замовлення постачальнику",
            transactions::TYPE_PURCHASE_ORDER,
            payload_of(input),
        )
        .await
        .map_err(PurchaseOrdersError::Infrastructure)?;
        Ok(queued_dto(input, out))
    }

    async fn update(
        &self,
        _id: Uuid,
        _input: &PurchaseOrderUpdateInput,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        Err(PurchaseOrdersError::BadRequest(q::unavailable(
            "редагування замовлення",
        )))
    }

    async fn delete(&self, _id: Uuid) -> Result<(), PurchaseOrdersError> {
        Err(PurchaseOrdersError::BadRequest(q::unavailable(
            "видалення замовлення",
        )))
    }

    async fn confirm(
        &self,
        _id: Uuid,
        _status: &str,
        _user_id: Uuid,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        Err(PurchaseOrdersError::BadRequest(q::unavailable(
            "проведення/скасування замовлення",
        )))
    }
}
