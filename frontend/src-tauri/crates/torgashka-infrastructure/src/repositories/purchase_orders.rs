//! SQL-репозиторій замовлень постачальнику (етап 8, група 5).
//!
//! 1:1 з Python api/v1/purchase_orders.py (6 роутів, 416 рядків):
//!   list, get, create, update, delete, confirm (confirmed → створює Invoice
//!   DRAFT з копією позицій; cancelled → статус CANCELLED).
//!
//! SESSION-семантика Python (SQLAlchemy identity map): create/update
//! відповідь містить ВХІДНІ Decimal-значення (без scale колонки), бо Python
//! перезавантажує той самий ORM-об'єкт з сесії. get/list/confirm — значення
//! з БД (scale колонки). Rust відтворює це перезаписом DTO вхідними даними.

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use sqlx::Row;
use crate::store_ctx::{current_store_ctx, StorePool};
use uuid::Uuid;

use torgashka_domain::purchase_orders::{
    InvoiceBriefDto, ProductBriefDto, PurchaseOrderCreateInput, PurchaseOrderDto,
    PurchaseOrderItemDto, PurchaseOrderItemInput, PurchaseOrderListDto, PurchaseOrderUpdateInput,
    PurchaseOrdersError,
};

/// Decimal з рядка (Python Decimal(str)).
fn rdec(s: &str) -> Decimal {
    s.parse::<Decimal>().unwrap_or_default()
}

const ORDER_COLS: &str = "po.id, po.number, po.supplier_id, po.order_date, \
     po.expected_date, po.status::text, po.is_fiscal, po.notes, \
     po.total_amount::text, po.invoice_id, po.created_at, po.updated_at, \
     s.name AS supplier_name, inv.number AS inv_number, inv.total_amount::text AS inv_total";

const ITEM_COLS: &str = "poi.id, poi.purchase_order_id, poi.product_id, \
     poi.quantity::text, poi.price::text, poi.total::text, poi.created_at, \
     p.title, p.barcode";

/// Репозиторій замовлень постачальнику.
pub struct SqlxPurchaseOrders {
    pool: StorePool,
}

impl SqlxPurchaseOrders {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }

    /// Автономер ЗАМ-{YYYYMMDD}-{NNN} (Python generate_order_number: max по рядку).
    async fn next_order_number(&self, today: &str) -> Result<String, PurchaseOrdersError> {
        let prefix = format!("ЗАМ-{today}-");
        let row = sqlx::query("SELECT max(number) AS m FROM purchase_orders WHERE number LIKE $1")
            .bind(format!("{prefix}%"))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let max_number: Option<String> = row.get("m");
        let new_seq = match max_number {
            Some(n) => n[n.len().saturating_sub(3)..].parse::<i32>().unwrap_or(0) + 1,
            None => 1,
        };
        Ok(format!("{}{:03}", prefix, new_seq))
    }

    /// Автономер ПН-{YYYYMMDD}-{NNN} для прибуткової накладної при confirm.
    async fn next_invoice_number(&self, today: &str) -> Result<String, PurchaseOrdersError> {
        let prefix = format!("ПН-{today}-");
        let row = sqlx::query("SELECT max(number) AS m FROM invoices WHERE number LIKE $1")
            .bind(format!("{prefix}%"))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let max_number: Option<String> = row.get("m");
        let new_seq = match max_number {
            Some(n) => n[n.len().saturating_sub(3)..].parse::<i32>().unwrap_or(0) + 1,
            None => 1,
        };
        Ok(format!("{}{:03}", prefix, new_seq))
    }

    /// Завантажує замовлення зі зв'язками (supplier, items+product, invoice).
    async fn fetch(&self, id: Uuid) -> Result<Option<PurchaseOrderDto>, PurchaseOrdersError> {
        let row = sqlx::query(&format!(
            "SELECT {ORDER_COLS} FROM purchase_orders po \
             LEFT JOIN suppliers s ON s.id = po.supplier_id \
             LEFT JOIN invoices inv ON inv.id = po.invoice_id \
             WHERE po.id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let Some(r) = row else {
            return Ok(None);
        };
        let order_id: Uuid = r.get("id");
        let items = self.fetch_items(order_id).await?;
        let invoice = r
            .get::<Option<String>, _>("inv_number")
            .map(|num| InvoiceBriefDto {
                id: r.get("invoice_id"),
                number: num,
                total_amount: r.get("inv_total"),
            });
        Ok(Some(PurchaseOrderDto {
            id: order_id,
            number: r.get("number"),
            supplier_id: r.get("supplier_id"),
            supplier_name: r.get("supplier_name"),
            order_date: r.get("order_date"),
            expected_date: r.get("expected_date"),
            status: r.get("status"),
            is_fiscal: r.get("is_fiscal"),
            notes: r.get("notes"),
            total_amount: r.get("total_amount"),
            invoice_id: r.get("invoice_id"),
            invoice,
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            items,
        }))
    }

    async fn fetch_items(
        &self,
        order_id: Uuid,
    ) -> Result<Vec<PurchaseOrderItemDto>, PurchaseOrdersError> {
        let rows = sqlx::query(&format!(
            "SELECT {ITEM_COLS} FROM purchase_order_items poi \
             LEFT JOIN products p ON p.id = poi.product_id \
             WHERE poi.purchase_order_id = $1 ORDER BY poi.created_at"
        ))
        .bind(order_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|r| PurchaseOrderItemDto {
                id: r.get("id"),
                purchase_order_id: r.get("purchase_order_id"),
                product_id: r.get("product_id"),
                product: Some(ProductBriefDto {
                    id: r.get("product_id"),
                    title: r.get("title"),
                    barcode: r.get("barcode"),
                }),
                quantity: r.get("quantity"),
                price: r.get("price"),
                total: r.get("total"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// Вставка позицій (Python session.add(item) для кожного item_data).
    async fn insert_items(
        &self,
        order_id: Uuid,
        items: &[PurchaseOrderItemInput],
    ) -> Result<(), PurchaseOrdersError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| PurchaseOrdersError::BadRequest(
                "Відсутній контекст точки (X-Store-Id)".to_string(),
            ))?;
        for it in items {
            sqlx::query(
                "INSERT INTO purchase_order_items \
                 (id, purchase_order_id, product_id, quantity, price, total, store_id, created_at) \
                 VALUES ($1,$2,$3,$4::numeric,$5::numeric,$6::numeric,$7, now())",
            )
            .bind(Uuid::new_v4())
            .bind(order_id)
            .bind(it.product_id)
            .bind(&it.quantity)
            .bind(&it.price)
            .bind(&it.total)
            .bind(store_id)
            .execute(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl torgashka_domain::purchase_orders::PurchaseOrdersService for SqlxPurchaseOrders {
    async fn list(
        &self,
        page: i64,
        size: i64,
    ) -> Result<PurchaseOrderListDto, PurchaseOrdersError> {
        let row = sqlx::query("SELECT count(*)::bigint AS c FROM purchase_orders")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let total: i64 = row.get("c");
        let offset = (page - 1) * size;
        let rows = sqlx::query(&format!(
            "SELECT {ORDER_COLS} FROM purchase_orders po \
             LEFT JOIN suppliers s ON s.id = po.supplier_id \
             LEFT JOIN invoices inv ON inv.id = po.invoice_id \
             ORDER BY po.created_at DESC LIMIT $1 OFFSET $2"
        ))
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        let mut items = Vec::with_capacity(rows.len());
        for r in &rows {
            let order_id: Uuid = r.get("id");
            let order_items = self.fetch_items(order_id).await?;
            let invoice = r
                .get::<Option<String>, _>("inv_number")
                .map(|num| InvoiceBriefDto {
                    id: r.get("invoice_id"),
                    number: num,
                    total_amount: r.get("inv_total"),
                });
            items.push(PurchaseOrderDto {
                id: order_id,
                number: r.get("number"),
                supplier_id: r.get("supplier_id"),
                supplier_name: r.get("supplier_name"),
                order_date: r.get("order_date"),
                expected_date: r.get("expected_date"),
                status: r.get("status"),
                is_fiscal: r.get("is_fiscal"),
                notes: r.get("notes"),
                total_amount: r.get("total_amount"),
                invoice_id: r.get("invoice_id"),
                invoice,
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
                items: order_items,
            });
        }
        Ok(PurchaseOrderListDto {
            items,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn get(&self, id: Uuid) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        match self.fetch(id).await? {
            Some(dto) => Ok(dto),
            None => Err(PurchaseOrdersError::NotFound(format!(
                "Замовлення з ID '{id}' не знайдено"
            ))),
        }
    }

    async fn create(
        &self,
        input: &PurchaseOrderCreateInput,
        user_id: Uuid,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        let number = match &input.number {
            Some(n) => n.clone(),
            None => {
                let today = chrono::Utc::now().format("%Y%m%d").to_string();
                self.next_order_number(&today).await?
            }
        };
        // Python: total_amount = sum(item.total), якщо не передано і є items.
        let total_amount = match &input.total_amount {
            Some(t) => Some(t.clone()),
            None if !input.items.is_empty() => {
                let mut t = Decimal::ZERO;
                for it in &input.items {
                    t += rdec(&it.total);
                }
                Some(t.to_string())
            }
            None => None,
        };
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| PurchaseOrdersError::BadRequest(
                "Відсутній контекст точки (X-Store-Id)".to_string(),
            ))?;
        let new_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO purchase_orders \
             (id, number, supplier_id, order_date, expected_date, is_fiscal, notes, \
              total_amount, status, created_by_id, store_id, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8::numeric,'draft',$9,$10, now(), now())",
        )
        .bind(new_id)
        .bind(&number)
        .bind(input.supplier_id)
        .bind(input.order_date)
        .bind(input.expected_date)
        .bind(input.is_fiscal)
        .bind(input.notes.as_deref())
        .bind(total_amount.as_deref())
        .bind(user_id)
        .bind(store_id)
        .execute(&self.pool)
        .await
        .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        self.insert_items(new_id, &input.items).await?;
        let mut dto = match self.fetch(new_id).await? {
            Some(v) => v,
            None => {
                return Err(PurchaseOrdersError::Infrastructure(
                    "створене замовлення не знайдене".into(),
                ))
            }
        };
        // Python (identity map): session-значення — вхідні Decimal без scale.
        dto.total_amount = total_amount;
        for (dto_item, in_item) in dto.items.iter_mut().zip(input.items.iter()) {
            dto_item.quantity = in_item.quantity.clone();
            dto_item.price = in_item.price.clone();
            dto_item.total = in_item.total.clone();
        }
        Ok(dto)
    }

    async fn update(
        &self,
        id: Uuid,
        input: &PurchaseOrderUpdateInput,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        let row = sqlx::query("SELECT status::text AS st FROM purchase_orders WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let Some(r) = row else {
            return Err(PurchaseOrdersError::NotFound(format!(
                "Замовлення з ID '{id}' не знайдено"
            )));
        };
        let status: String = r.get("st");
        if status != "draft" {
            return Err(PurchaseOrdersError::BadRequest(
                "Можна редагувати тільки чернетки".to_string(),
            ));
        }
        // Python: model_dump(exclude_unset=True, exclude={"items"}) — тільки
        // задані поля; окремі UPDATE (значення з input).
        if let Some(v) = &input.number {
            sqlx::query("UPDATE purchase_orders SET number = $1, updated_at = now() WHERE id = $2")
                .bind(v)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        if let Some(v) = input.supplier_id {
            sqlx::query(
                "UPDATE purchase_orders SET supplier_id = $1, updated_at = now() WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        if let Some(v) = input.order_date {
            sqlx::query(
                "UPDATE purchase_orders SET order_date = $1, updated_at = now() WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        if let Some(v) = input.expected_date {
            sqlx::query(
                "UPDATE purchase_orders SET expected_date = $1, updated_at = now() WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        if let Some(v) = input.is_fiscal {
            sqlx::query(
                "UPDATE purchase_orders SET is_fiscal = $1, updated_at = now() WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        if let Some(v) = &input.notes {
            sqlx::query("UPDATE purchase_orders SET notes = $1, updated_at = now() WHERE id = $2")
                .bind(v)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        if let Some(v) = &input.total_amount {
            sqlx::query(
                "UPDATE purchase_orders SET total_amount = $1::numeric, updated_at = now() \
                 WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        }
        if let Some(items) = &input.items {
            sqlx::query("DELETE FROM purchase_order_items WHERE purchase_order_id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
            self.insert_items(id, items).await?;
        }
        let mut dto = match self.fetch(id).await? {
            Some(v) => v,
            None => {
                return Err(PurchaseOrdersError::Infrastructure(
                    "замовлення не знайдене після оновлення".into(),
                ))
            }
        };
        // Python (identity map): total_amount та items — session-значення.
        if let Some(t) = &input.total_amount {
            dto.total_amount = Some(t.clone());
        }
        if let Some(items) = &input.items {
            for (dto_item, in_item) in dto.items.iter_mut().zip(items.iter()) {
                dto_item.quantity = in_item.quantity.clone();
                dto_item.price = in_item.price.clone();
                dto_item.total = in_item.total.clone();
            }
        }
        Ok(dto)
    }

    async fn delete(&self, id: Uuid) -> Result<(), PurchaseOrdersError> {
        let row = sqlx::query("SELECT status::text AS st FROM purchase_orders WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let Some(r) = row else {
            return Err(PurchaseOrdersError::NotFound(format!(
                "Замовлення з ID '{id}' не знайдено"
            )));
        };
        let status: String = r.get("st");
        if status != "draft" {
            return Err(PurchaseOrdersError::BadRequest(
                "Можна видалити тільки чернетку".to_string(),
            ));
        }
        sqlx::query("DELETE FROM purchase_orders WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        Ok(())
    }

    async fn confirm(
        &self,
        id: Uuid,
        status: &str,
        user_id: Uuid,
    ) -> Result<PurchaseOrderDto, PurchaseOrdersError> {
        let row = sqlx::query(&format!(
            "SELECT {ORDER_COLS} FROM purchase_orders po \
             LEFT JOIN suppliers s ON s.id = po.supplier_id \
             LEFT JOIN invoices inv ON inv.id = po.invoice_id \
             WHERE po.id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
        let Some(r) = row else {
            return Err(PurchaseOrdersError::NotFound(format!(
                "Замовлення з ID '{id}' не знайдено"
            )));
        };
        let current_status: String = r.get("status");
        if current_status != "draft" {
            return Err(PurchaseOrdersError::BadRequest(format!(
                "Замовлення вже має статус '{current_status}'"
            )));
        }
        match status {
            "confirmed" => {
                let items = self.fetch_items(id).await?;
                let today = chrono::Utc::now().format("%Y%m%d").to_string();
                let invoice_number = self.next_invoice_number(&today).await?;
                let order_number: String = r.get("number");
                let supplier_id: Uuid = r.get("supplier_id");
                let order_date: NaiveDateTime = r.get("order_date");
                let is_fiscal: bool = r.get("is_fiscal");
                let total_amount: Option<String> = r.get("total_amount");
                let new_invoice_id = Uuid::new_v4();
                sqlx::query(
                    "INSERT INTO invoices \
                     (id, number, supplier_id, invoice_date, payment_method, is_fiscal, notes, \
                      total_amount, status, created_by_id, created_at, updated_at) \
                     VALUES ($1,$2,$3,$4,'credit'::payment_method,$5,$6,$7::numeric,\
                             'draft'::invoice_status,$8, now(), now())",
                )
                .bind(new_invoice_id)
                .bind(&invoice_number)
                .bind(supplier_id)
                .bind(order_date)
                .bind(is_fiscal)
                .bind(format!(
                    "Автоматично створено із замовлення №{order_number}"
                ))
                .bind(total_amount.as_deref())
                .bind(user_id)
                .execute(&self.pool)
                .await
                .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
                for it in &items {
                    sqlx::query(
                        "INSERT INTO invoice_items \
                         (invoice_id, product_id, quantity, price, total, created_at) \
                         VALUES ($1,$2,$3::numeric,$4::numeric,$5::numeric, now())",
                    )
                    .bind(new_invoice_id)
                    .bind(it.product_id)
                    .bind(&it.quantity)
                    .bind(&it.price)
                    .bind(&it.total)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
                }
                sqlx::query(
                    "UPDATE purchase_orders SET invoice_id = $1, status = 'confirmed', \
                     updated_at = now() WHERE id = $2",
                )
                .bind(new_invoice_id)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
            }
            "cancelled" => {
                sqlx::query(
                    "UPDATE purchase_orders SET status = 'cancelled', updated_at = now() \
                     WHERE id = $1",
                )
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| PurchaseOrdersError::Infrastructure(e.to_string()))?;
            }
            other => {
                return Err(PurchaseOrdersError::BadRequest(format!(
                    "Невірний статус. Використовуйте 'confirmed' або 'cancelled' (отримано: {other})"
                )));
            }
        }
        let mut dto = match self.fetch(id).await? {
            Some(dto) => dto,
            None => {
                return Err(PurchaseOrdersError::Infrastructure(
                    "замовлення не знайдене після confirm".into(),
                ))
            }
        };
        // АНОМАЛІЯ PYTHON (1:1): confirm віддає invoice: null навіть коли
        // invoice_id заповнений — SQLAlchemy post_update + identity map не
        // перезавантажує relationship invoice у тій самій сесії. get/list
        // (нова сесія) віддають invoice brief. Rust відтворює 1:1.
        dto.invoice = None;
        Ok(dto)
    }
}
