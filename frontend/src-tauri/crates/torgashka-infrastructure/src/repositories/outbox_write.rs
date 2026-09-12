//! `OutboxWrite` — адаптер write-гілки довідників та інвентаризації, який
//! застосовується, коли запис у локальну БД недоступний (історично — вузол
//! read-only).
//!
//! Тип `WriteDirectories` покриває ДВА різні класи ADR (§11.7.9):
//!   * **`inventory` → `LocalOutbox`** (§11.1): перерахунок проводить каса —
//!     агрегат + outbox + абсолютний stock-рівень локально
//!     (`transactions::enqueue_transaction(TYPE_INVENTORY)`), доставка —
//!     push-цикл (приймач `sync_receivers::accept_inventory`).
//!   * **products / categories / suppliers → відмова з поясненням**: ці
//!     довідники належать мережі, тож локальний адаптер їх не проводить.
//!     Якщо виклик дійшов сюди — це явна відмова, а не запис у локальну БД.
//!
//! `PgPool` у структурі немає — адаптер фізично не має чим писати в PG.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use torgashka_domain::{
    CategoryCreateInput, CategoryDto, CategoryUpdateInput, InventoryCountsDto,
    InventoryCreateInput, InventoryDto, InventoryItemDto, InventorySummaryDto,
    InventoryUpdateInput, Page, ProductCreateInput, ProductDto, ProductUpdateInput,
    SupplierCreateInput, SupplierDto, SupplierUpdateInput, WriteDirectories, WriteError,
};

use crate::offline::transactions::{self, EnqueuedTransaction};

use super::outbox_local as q;

/// Payload інвентаризації для черги: `location`/`inventory_date`/`notes`/`items`
/// читає приймач `sync_receivers::accept_inventory`; `quantity` = факт
/// (локальний `stock::set_stock_level` розуміє саме `quantity`, приймач —
/// `quantity` з fallback на `actual_quantity`).
fn payload_of(input: &InventoryCreateInput) -> Value {
    let items: Vec<Value> = input
        .items
        .iter()
        .map(|it| {
            json!({
                "product_id": it.product_id,
                "quantity": q::num(&it.actual_quantity),
                "actual_quantity": q::num(&it.actual_quantity),
                "accounting_quantity": q::num(&it.accounting_quantity),
                "difference": q::num(&it.difference),
                "cost_price": q::num(&it.cost_price),
                "price": q::num(&it.price),
            })
        })
        .collect();
    json!({
        "number": input.number,
        "location": input.location,
        "inventory_date": input.inventory_date.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "notes": input.notes,
        "created_by": input.created_by,
        "items": items,
    })
}

/// DTO черги: `id` = `client_uuid` конверта (ключ ідемпотентності push),
/// `status = queued` — бізнес-статус «primary ще не створив документ».
fn queued_dto(input: &InventoryCreateInput, out: EnqueuedTransaction) -> InventoryDto {
    let now = Utc::now().naive_utc();
    let doc_id = q::uuid_of(&out.client_uuid);
    let items: Vec<InventoryItemDto> = input
        .items
        .iter()
        .map(|it| {
            let cost = q::f64_of(&it.cost_price);
            let price = q::f64_of(&it.price);
            let fact = q::f64_of(&it.actual_quantity);
            InventoryItemDto {
                id: Uuid::new_v4(),
                inventory_id: doc_id,
                product_id: it.product_id,
                product: None,
                actual_quantity: it.actual_quantity.clone(),
                accounting_quantity: it.accounting_quantity.clone(),
                difference: it.difference.clone(),
                cost_price: it.cost_price.clone(),
                price: it.price.clone(),
                total_cost: (cost * fact * 100.0).round() as i64,
                total_selling: (price * fact * 100.0).round() as i64,
                created_at: now,
            }
        })
        .collect();
    let total_cost = q::dec_sum(
        input
            .items
            .iter()
            .map(|it| q::dec_mul(&it.actual_quantity, &it.cost_price)),
    );
    let total_selling = q::dec_sum(
        input
            .items
            .iter()
            .map(|it| q::dec_mul(&it.actual_quantity, &it.price)),
    );
    InventoryDto {
        id: doc_id,
        number: input
            .number
            .clone()
            .unwrap_or_else(|| format!("ЧЕРГА-{}", out.id)),
        location: input.location.clone().unwrap_or_default(),
        inventory_date: input.inventory_date,
        status: q::QUEUED_STATUS.to_string(),
        notes: input.notes.clone(),
        created_at: now,
        updated_at: now,
        items,
        summary: InventorySummaryDto {
            total_cost,
            total_selling,
            total_deviation: q::dec_sum(input.items.iter().map(|it| it.difference.clone())),
        },
    }
}

/// Standby-адаптер write-гілки: читання → `inner`, інвентаризація → черга,
/// довідники (proxy-клас) → явна відмова.
pub struct OutboxWrite {
    inner: Arc<dyn WriteDirectories + Send + Sync>,
}

impl OutboxWrite {
    pub fn new(inner: Arc<dyn WriteDirectories + Send + Sync>) -> Self {
        Self { inner }
    }

    /// Довідники виконуються на primary через гейт (`ProxyToPrimary`):
    /// прямий виклик на standby означав би запис у репліку.
    fn proxy_only(op: &str) -> WriteError {
        WriteError::BadRequest(q::unavailable(op))
    }
}

#[async_trait]
impl WriteDirectories for OutboxWrite {
    // ─── Products / Categories / Suppliers → ProxyToPrimary (§11.7.9.1) ──────

    async fn create_product(&self, _input: &ProductCreateInput) -> Result<ProductDto, WriteError> {
        Err(Self::proxy_only("створення товару"))
    }

    async fn update_product(
        &self,
        _id: Uuid,
        _input: &ProductUpdateInput,
    ) -> Result<ProductDto, WriteError> {
        Err(Self::proxy_only("редагування товару"))
    }

    async fn delete_product(&self, _id: Uuid) -> Result<(), WriteError> {
        Err(Self::proxy_only("видалення товару"))
    }

    async fn create_category(
        &self,
        _input: &CategoryCreateInput,
    ) -> Result<CategoryDto, WriteError> {
        Err(Self::proxy_only("створення категорії"))
    }

    async fn update_category(
        &self,
        _id: Uuid,
        _input: &CategoryUpdateInput,
    ) -> Result<CategoryDto, WriteError> {
        Err(Self::proxy_only("редагування категорії"))
    }

    async fn delete_category(&self, _id: Uuid) -> Result<(), WriteError> {
        Err(Self::proxy_only("видалення категорії"))
    }

    async fn create_supplier(
        &self,
        _input: &SupplierCreateInput,
    ) -> Result<SupplierDto, WriteError> {
        Err(Self::proxy_only("створення постачальника"))
    }

    async fn update_supplier(
        &self,
        _id: Uuid,
        _input: &SupplierUpdateInput,
    ) -> Result<SupplierDto, WriteError> {
        Err(Self::proxy_only("редагування постачальника"))
    }

    async fn delete_supplier(&self, _id: Uuid) -> Result<(), WriteError> {
        Err(Self::proxy_only("видалення постачальника"))
    }

    // ─── Читання (репліка — дозволене джерело, §10) ─────────────────────────

    async fn category_name_exists(
        &self,
        name: &str,
        exclude_id: Option<Uuid>,
    ) -> Result<bool, WriteError> {
        self.inner.category_name_exists(name, exclude_id).await
    }

    async fn list_inventories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<InventoryDto>, WriteError> {
        self.inner.list_inventories(page, size).await
    }

    async fn inventory_counts(&self) -> Result<InventoryCountsDto, WriteError> {
        self.inner.inventory_counts().await
    }

    async fn get_inventory(&self, id: Uuid) -> Result<InventoryDto, WriteError> {
        self.inner.get_inventory(id).await
    }

    // ─── Inventory → LocalOutbox (§11.1) ────────────────────────────────────

    async fn create_inventory(
        &self,
        input: &InventoryCreateInput,
    ) -> Result<InventoryDto, WriteError> {
        let out = q::enqueue(
            "створення інвентаризації",
            transactions::TYPE_INVENTORY,
            payload_of(input),
        )
        .await
        .map_err(WriteError::Infrastructure)?;
        Ok(queued_dto(input, out))
    }

    async fn update_inventory(
        &self,
        _id: Uuid,
        _input: &InventoryUpdateInput,
    ) -> Result<InventoryDto, WriteError> {
        Err(WriteError::BadRequest(q::unavailable(
            "редагування інвентаризації",
        )))
    }

    async fn delete_inventory(&self, _id: Uuid) -> Result<(), WriteError> {
        Err(WriteError::BadRequest(q::unavailable(
            "видалення інвентаризації",
        )))
    }

    async fn confirm_inventory(&self, _id: Uuid) -> Result<InventoryDto, WriteError> {
        Err(WriteError::BadRequest(q::unavailable(
            "проведення інвентаризації",
        )))
    }

    async fn cancel_inventory(&self, _id: Uuid) -> Result<InventoryDto, WriteError> {
        Err(WriteError::BadRequest(q::unavailable(
            "скасування інвентаризації",
        )))
    }
}
