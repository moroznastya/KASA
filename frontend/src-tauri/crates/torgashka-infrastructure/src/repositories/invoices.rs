//! Репозиторій інвойсів (етап 8 — група 3): SqlxInvoices.
//!
//! 1:1 з Python:
//! - v1: api/v1/invoices.py + document_service.confirm/cancel_invoice
//! - v2: api/v2/invoices.py + invoice_use_cases (create/confirm/cancel —
//!   ЗАДУМАНА семантика, Python 500 через змішування entity/ORM)
//!
//! Грошові поля numeric читаються як `::text` і парсяться: v1 → String
//! (Decimal-рядок), v2 → f64.

use chrono::NaiveDateTime;
use sqlx::{Row};
use crate::store_ctx::{current_store_ctx, StorePool};
use uuid::Uuid;

use torgashka_domain::invoices::{
    InvoiceCreateV1Input, InvoiceCreateV2Input, InvoiceItemV1Dto, InvoiceItemV2Dto,
    InvoicePaymentInfoV1Dto, InvoicePaymentInfoV2Dto, InvoicePrintDto, InvoicePrintRequest,
    InvoiceUpdateV1Input, InvoiceUpdateV2Input, InvoiceV1Dto, InvoiceV1ListDto, InvoiceV2Dto,
    InvoiceV2ListDto, InvoicesError, InvoicesV1Service, InvoicesV2Service, PriceChangeItemDto,
    ProductBriefV1Dto,
};

fn de(s: String) -> InvoicesError {
    InvoicesError::Infrastructure(s)
}

/// f64 з numeric::text.
fn f64n(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// Decimal-рядок з numeric::text (нормалізує 10.50 → 10.5, як Python Decimal str).
fn den(s: &str) -> String {
    s.trim().to_string()
}

/// rust_decimal з рядка.
fn rdec(s: &str) -> rust_decimal::Decimal {
    s.trim()
        .parse::<rust_decimal::Decimal>()
        .unwrap_or_default()
}

const INVOICE_COLS: &str = "i.id, i.number, i.supplier_id, i.invoice_date, i.status::text, \
     i.payment_method::text, i.is_fiscal, i.notes, i.total_amount::text, \
     i.created_by_id, i.created_at, i.updated_at, s.name AS supplier_name, \
     COALESCE((SELECT SUM(ABS(sl.amount)) FROM supplier_ledger sl \
               WHERE sl.document_id = i.id AND sl.operation_type IN ('payment','return')), 0)::text AS paid_amount";

const ITEM_COLS: &str = "ii.id, ii.invoice_id, ii.product_id, ii.quantity::text, \
     ii.price::text, ii.total::text, ii.cost_price::text, ii.markup_percent::text, \
     ii.previous_price::text, ii.created_at, \
     p.title, p.barcode, p.price::text AS p_price, p.markup::text AS p_markup, \
     p.cost_price::text AS p_cost";

/// Репозиторій інвойсів (v1+v2).
pub struct SqlxInvoices {
    pool: StorePool,
}

impl SqlxInvoices {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }

    /// Доступ до пулу для суміжних модулів (price_tag).
    pub(crate) fn pg_pool(&self) -> &StorePool {
        &self.pool
    }

    /// Збирає позиції для списку invoice_id (Python selectinload).
    async fn items_for(
        &self,
        invoice_ids: &[Uuid],
    ) -> Result<Vec<InvoiceItemV1Dto>, InvoicesError> {
        if invoice_ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query(&format!(
            "SELECT {ITEM_COLS} FROM invoice_items ii \
                 LEFT JOIN products p ON p.id = ii.product_id \
                 WHERE ii.invoice_id = ANY($1) \
                 ORDER BY ii.created_at"
        ))
        .bind(invoice_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(item_v1_from_row(&r));
        }
        Ok(out)
    }

    /// Повна v1-накладна (Python get_invoice з selectinload).
    async fn fetch_v1(&self, id: Uuid) -> Result<Option<InvoiceV1Dto>, InvoicesError> {
        let row = sqlx::query(&format!(
            "SELECT {INVOICE_COLS} FROM invoices i \
             LEFT JOIN suppliers s ON s.id = i.supplier_id \
             WHERE i.id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else { return Ok(None) };
        let items = self.items_for(&[id]).await?;
        Ok(Some(invoice_v1_from_row(&r, items)))
    }

    /// v2-накладна (Python find_by_id + InvoiceMapper.entity_to_dto).
    async fn fetch_v2(&self, id: Uuid) -> Result<Option<InvoiceV2Dto>, InvoicesError> {
        let row = sqlx::query(
            "SELECT id, number, supplier_id, status::text, total_amount::text, \
             created_at, notes FROM invoices WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else { return Ok(None) };
        let items = sqlx::query(
            "SELECT product_id, quantity::text, price::text FROM invoice_items WHERE invoice_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut v2_items = Vec::with_capacity(items.len());
        for it in items {
            v2_items.push(InvoiceItemV2Dto {
                product_id: it.get("product_id"),
                quantity: f64n(&it.get::<String, _>("quantity")),
                price: f64n(&it.get::<String, _>("price")),
                tax_rate: 20,
                name: String::new(),
            });
        }
        let total: Option<String> = r.get("total_amount");
        Ok(Some(InvoiceV2Dto {
            id: r.get("id"),
            number: r.get("number"),
            supplier_id: r.get("supplier_id"),
            items: v2_items,
            total: total.as_deref().map(f64n),
            status: r.get("status"),
            created_at: r.get("created_at"),
            confirmed_at: None,
            notes: r.get::<Option<String>, _>("notes").unwrap_or_default(),
        }))
    }

    /// Python generate_invoice_number: ПН-{YYYYMMDD UTC}-{XXX}.
    async fn next_number(&self) -> Result<String, InvoicesError> {
        let today = chrono::Utc::now().format("%Y%m%d").to_string();
        let prefix = format!("ПН-{today}-");
        let row = sqlx::query(
            "SELECT number FROM invoices WHERE number LIKE $1 ORDER BY number DESC LIMIT 1",
        )
        .bind(format!("{prefix}%"))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let seq = match row {
            Some(r) => {
                let n: String = r.get("number");
                n[n.len().saturating_sub(3)..].parse::<i32>().unwrap_or(0) + 1
            }
            None => 1,
        };
        Ok(format!("{prefix}{seq:03}"))
    }

    /// Вставка позицій v1 (Python create/update: previous_price з product.price,
    /// cost_price = item.cost_price or price, markup_percent or 0).
    async fn insert_items_v1(
        &self,
        invoice_id: Uuid,
        items: &[torgashka_domain::invoices::InvoiceItemV1Input],
    ) -> Result<(), InvoicesError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        for it in items {
            let prev: Option<String> =
                sqlx::query("SELECT price::text AS p FROM products WHERE id = $1")
                    .bind(it.product_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| de(e.to_string()))?
                    .map(|r| r.get::<String, _>("p"));
            let cost = it.cost_price.clone().unwrap_or_else(|| it.price.clone());
            let markup = it.markup_percent.clone().unwrap_or_else(|| "0".to_string());
            sqlx::query(
                "INSERT INTO invoice_items \
                 (invoice_id, product_id, quantity, price, total, cost_price, markup_percent, \
                  previous_price, store_id, created_at) \
                 VALUES ($1,$2,$3::numeric,$4::numeric,$5::numeric,$6::numeric,$7::numeric,$8::numeric,$9, now())",
            )
            .bind(invoice_id)
            .bind(it.product_id)
            .bind(&it.quantity)
            .bind(&it.price)
            .bind(&it.total)
            .bind(&cost)
            .bind(&markup)
            .bind(prev.as_deref())
            .bind(store_id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        Ok(())
    }

    /// Python create_ledger_entry (INVOICE/CORRECTION): balance_after = current + amount.
    #[allow(clippy::too_many_arguments)]
    async fn ledger_entry(
        &self,
        supplier_id: Uuid,
        operation_type: &str,
        amount: &str,
        operation_date: NaiveDateTime,
        document_id: Uuid,
        document_number: &str,
        notes: &str,
    ) -> Result<(), InvoicesError> {
        // Поточний баланс постачальника (Python get_supplier_balance = sum(amount)).
        let cur: String = sqlx::query(
            "SELECT COALESCE(SUM(amount), 0)::text AS b FROM supplier_ledger WHERE supplier_id = $1",
        )
        .bind(supplier_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?
        .get("b");
        let new_bal = rdec(&cur) + rdec(amount);
        sqlx::query(
            "INSERT INTO supplier_ledger \
             (supplier_id, operation_type, document_id, document_number, amount, balance_after, operation_date, notes, created_at) \
             VALUES ($1,$2::ledger_operation_type,$3,$4,$5::numeric,$6::numeric,$7,$8, now())",
        )
        .bind(supplier_id)
        .bind(operation_type)
        .bind(document_id)
        .bind(document_number)
        .bind(amount)
        .bind(new_bal.to_string())
        .bind(operation_date)
        .bind(notes)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Python product_service.update_stock (stock += quantity_change) → stock table.
    async fn update_stock(&self, product_id: Uuid, qty: &str) -> Result<(), InvoicesError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        sqlx::query(
            "INSERT INTO stock (store_id, product_id, quantity, price, updated_at)
             VALUES ($1, $2, $3::numeric, 0, now())
             ON CONFLICT (store_id, product_id) DO UPDATE
                SET quantity = stock.quantity + EXCLUDED.quantity, updated_at = now()",
        )
        .bind(store_id)
        .bind(product_id)
        .bind(qty)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Python _increase_fiscal_stock / _decrease_fiscal_stock.
    async fn fiscal_stock(
        &self,
        product_id: Uuid,
        qty: &str,
        increase: bool,
    ) -> Result<(), InvoicesError> {
        if increase {
            sqlx::query(
                "UPDATE products SET is_fiscal = true, fiscal_stock = COALESCE(fiscal_stock, 0) + $1::numeric, \
                 updated_at = now() WHERE id = $2",
            )
            .bind(qty)
            .bind(product_id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        } else {
            sqlx::query(
                "UPDATE products SET fiscal_stock = GREATEST(0, COALESCE(fiscal_stock, 0) - $1::numeric), \
                 updated_at = now() WHERE id = $2",
            )
            .bind(qty)
            .bind(product_id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        Ok(())
    }
}

fn item_v1_from_row(r: &sqlx::postgres::PgRow) -> InvoiceItemV1Dto {
    InvoiceItemV1Dto {
        id: r.get("id"),
        invoice_id: r.get("invoice_id"),
        product_id: r.get("product_id"),
        product: {
            let title: Option<String> = r.get("title");
            title.map(|t| ProductBriefV1Dto {
                id: r.get("product_id"),
                title: t,
                barcode: r.get("barcode"),
                price: r.get::<Option<String>, _>("p_price"),
                markup: r.get::<Option<String>, _>("p_markup"),
                cost_price: r.get::<Option<String>, _>("p_cost"),
            })
        },
        quantity: den(&r.get::<String, _>("quantity")),
        price: den(&r.get::<String, _>("price")),
        total: den(&r.get::<String, _>("total")),
        cost_price: r.get::<Option<String>, _>("cost_price").map(|s| den(&s)),
        markup_percent: r
            .get::<Option<String>, _>("markup_percent")
            .map(|s| den(&s)),
        previous_price: r
            .get::<Option<String>, _>("previous_price")
            .map(|s| den(&s)),
        created_at: r.get("created_at"),
    }
}

fn invoice_v1_from_row(r: &sqlx::postgres::PgRow, items: Vec<InvoiceItemV1Dto>) -> InvoiceV1Dto {
    let total = r
        .get::<Option<String>, _>("total_amount")
        .map(|s| den(&s))
        .and_then(|s| s.parse::<rust_decimal::Decimal>().ok());
    let paid = r
        .get::<Option<String>, _>("paid_amount")
        .map(|s| den(&s))
        .and_then(|s| s.parse::<rust_decimal::Decimal>().ok());
    let (paid_str, remaining) = match (total, paid) {
        (Some(t), Some(p)) => {
            let rem = (t - p).round_dp(2);
            (
                Some(format!("{p:.2}")),
                if rem < rust_decimal::Decimal::ZERO {
                    Some("0.00".to_string())
                } else {
                    Some(format!("{rem:.2}"))
                },
            )
        }
        (Some(t), None) => (None, Some(format!("{t:.2}"))),
        _ => (None, None),
    };
    InvoiceV1Dto {
        id: r.get("id"),
        number: r.get("number"),
        supplier_id: r.get("supplier_id"),
        supplier_name: r.get("supplier_name"),
        invoice_date: r.get("invoice_date"),
        status: r.get("status"),
        payment_method: r.get("payment_method"),
        is_fiscal: r.get("is_fiscal"),
        notes: r.get("notes"),
        total_amount: total.map(|t| format!("{t:.2}")),
        paid_amount: paid_str,
        remaining,
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        items,
    }
}

// ─── v1 ─────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl InvoicesV1Service for SqlxInvoices {
    async fn list_v1(
        &self,
        supplier_id: Option<Uuid>,
        page: i64,
        size: i64,
    ) -> Result<InvoiceV1ListDto, InvoicesError> {
        let mut where_sql = String::new();
        let mut binds: Vec<String> = vec![];
        if let Some(sid) = supplier_id {
            where_sql = " WHERE i.supplier_id = $1 AND i.status = 'confirmed'".to_string();
            binds.push(sid.to_string());
        }
        let total: i64 = if let Some(sid) = supplier_id {
            sqlx::query(&format!("SELECT count(*) FROM invoices i {where_sql}"))
                .bind(sid)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?
                .get("count")
        } else {
            sqlx::query("SELECT count(*) FROM invoices")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?
                .get("count")
        };
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        let offset = (page - 1) * size;
        let rows = if let Some(sid) = supplier_id {
            sqlx::query(&format!(
                "SELECT {INVOICE_COLS} FROM invoices i \
                 LEFT JOIN suppliers s ON s.id = i.supplier_id {where_sql} \
                 ORDER BY i.created_at DESC OFFSET $2 LIMIT $3"
            ))
            .bind(sid)
            .bind(offset)
            .bind(size)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?
        } else {
            sqlx::query(&format!(
                "SELECT {INVOICE_COLS} FROM invoices i \
                 LEFT JOIN suppliers s ON s.id = i.supplier_id \
                 ORDER BY i.created_at DESC OFFSET $1 LIMIT $2"
            ))
            .bind(offset)
            .bind(size)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?
        };
        let ids: Vec<Uuid> = rows.iter().map(|r| r.get::<Uuid, _>("id")).collect();
        let items = self.items_for(&ids).await?;
        let mut by_inv: std::collections::HashMap<Uuid, Vec<InvoiceItemV1Dto>> =
            std::collections::HashMap::new();
        for it in items {
            by_inv.entry(it.invoice_id).or_default().push(it);
        }
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let id: Uuid = r.get("id");
            let its = by_inv.remove(&id).unwrap_or_default();
            out.push(invoice_v1_from_row(&r, its));
        }
        Ok(InvoiceV1ListDto {
            items: out,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn get_v1(&self, id: Uuid) -> Result<InvoiceV1Dto, InvoicesError> {
        match self.fetch_v1(id).await? {
            Some(v) => Ok(v),
            None => Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            ))),
        }
    }

    async fn create_v1(
        &self,
        input: &InvoiceCreateV1Input,
        user_id: Uuid,
    ) -> Result<InvoiceV1Dto, InvoicesError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        let mut total_amount = input.total_amount.clone();
        if total_amount.is_none() && !input.items.is_empty() {
            // Python: sum(item.total for item in data.items) — Decimal.
            let mut s = rust_decimal::Decimal::ZERO;
            for it in &input.items {
                s += rdec(&it.total);
            }
            total_amount = Some(s.to_string());
        }
        let number = match &input.number {
            Some(n) if !n.is_empty() => n.clone(),
            _ => self.next_number().await?,
        };
        let new_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO invoices \
             (id, number, supplier_id, invoice_date, payment_method, is_fiscal, notes, total_amount, \
              status, created_by_id, store_id, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5::payment_method,$6,$7,$8::numeric,'draft',$9,$10, now(), now())",
        )
        .bind(new_id)
        .bind(&number)
        .bind(input.supplier_id)
        .bind(input.invoice_date)
        .bind(input.payment_method.as_deref())
        .bind(input.is_fiscal)
        .bind(input.notes.as_deref())
        .bind(total_amount.as_deref())
        .bind(user_id)
        .bind(store_id)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        self.insert_items_v1(new_id, &input.items).await?;
        let mut dto = match self.fetch_v1(new_id).await? {
            Some(v) => v,
            None => {
                return Err(InvoicesError::Infrastructure(
                    "створена накладна не знайдена".into(),
                ))
            }
        };
        // Python повертає значення СЕСІЇ (вхідні Decimal без scale):
        // total_amount = передане або sum(item.total); items — вхідні quantity/
        // price/total/cost_price/markup_percent (не перезавантажені з numeric).
        dto.total_amount = total_amount.clone();
        for (dto_item, in_item) in dto.items.iter_mut().zip(input.items.iter()) {
            dto_item.quantity = in_item.quantity.clone();
            dto_item.price = in_item.price.clone();
            dto_item.total = in_item.total.clone();
            dto_item.cost_price = Some(
                in_item
                    .cost_price
                    .clone()
                    .unwrap_or_else(|| in_item.price.clone()),
            );
            dto_item.markup_percent = Some(
                in_item
                    .markup_percent
                    .clone()
                    .unwrap_or_else(|| "0".to_string()),
            );
        }
        Ok(dto)
    }

    async fn update_v1(
        &self,
        id: Uuid,
        input: &InvoiceUpdateV1Input,
    ) -> Result<InvoiceV1Dto, InvoicesError> {
        let row = sqlx::query("SELECT status::text AS st FROM invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let status: String = r.get("st");
        if status != "draft" {
            return Err(InvoicesError::BadRequest(
                "Можна редагувати тільки чернетки".into(),
            ));
        }
        // Скалярні поля (Python update_data exclude_unset) — String-binds з SQL-кастами.
        let mut sets: Vec<String> = vec![];
        let mut vals: Vec<String> = vec![];
        let mut idx = 1usize;
        macro_rules! push_set {
            ($col:expr, $cast:expr, $val:expr) => {
                sets.push(format!("{}=${}::{}", $col, idx, $cast));
                vals.push($val.to_string());
                idx += 1;
            };
        }
        if let Some(v) = &input.number {
            push_set!("number", "text", v);
        }
        if let Some(v) = input.supplier_id {
            push_set!("supplier_id", "uuid", v);
        }
        if let Some(v) = input.invoice_date {
            push_set!("invoice_date", "timestamp", v.format("%Y-%m-%d %H:%M:%S"));
        }
        if let Some(v) = &input.payment_method {
            push_set!("payment_method", "payment_method", v);
        }
        if let Some(v) = input.is_fiscal {
            push_set!("is_fiscal", "boolean", if v { "true" } else { "false" });
        }
        if let Some(v) = &input.notes {
            push_set!("notes", "text", v);
        }
        if let Some(v) = &input.total_amount {
            push_set!("total_amount", "numeric", v);
        }
        if !sets.is_empty() {
            let mut q = "UPDATE invoices SET ".to_string();
            q.push_str(&sets.join(", "));
            q.push_str(&format!(", updated_at = now() WHERE id = ${idx}"));
            let mut qb = sqlx::query(&q);
            for v in &vals {
                qb = qb.bind(v);
            }
            qb = qb.bind(id);
            qb.execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
        }
        // Позиції (Python: видалити старі, додати нові; total = sum(item.total)).
        if let Some(items) = &input.items {
            sqlx::query("DELETE FROM invoice_items WHERE invoice_id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            self.insert_items_v1(id, items).await?;
            let mut s = rust_decimal::Decimal::ZERO;
            for it in items {
                s += rdec(&it.total);
            }
            sqlx::query("UPDATE invoices SET total_amount = $1::numeric WHERE id = $2")
                .bind(s.to_string())
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
        }
        let mut dto = match self.fetch_v1(id).await? {
            Some(v) => v,
            None => {
                return Err(InvoicesError::Infrastructure(
                    "накладну не знайдено після оновлення".into(),
                ))
            }
        };
        // Python: total_amount = sum(item.total) з ВХІДНИХ даних (не з БД);
        // items — вхідні значення без scale (як create).
        if let Some(items) = &input.items {
            let mut t = rust_decimal::Decimal::ZERO;
            for it in items {
                t += rdec(&it.total);
            }
            dto.total_amount = Some(t.to_string());
            for (dto_item, in_item) in dto.items.iter_mut().zip(items.iter()) {
                dto_item.quantity = in_item.quantity.clone();
                dto_item.price = in_item.price.clone();
                dto_item.total = in_item.total.clone();
                dto_item.cost_price = Some(
                    in_item
                        .cost_price
                        .clone()
                        .unwrap_or_else(|| in_item.price.clone()),
                );
                dto_item.markup_percent = Some(
                    in_item
                        .markup_percent
                        .clone()
                        .unwrap_or_else(|| "0".to_string()),
                );
            }
        }
        Ok(dto)
    }

    async fn delete_v1(&self, id: Uuid) -> Result<(), InvoicesError> {
        let row = sqlx::query("SELECT status::text AS st FROM invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let status: String = r.get("st");
        if status != "draft" {
            return Err(InvoicesError::BadRequest(
                "Можна видалити тільки чернетку".into(),
            ));
        }
        sqlx::query("DELETE FROM invoices WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    async fn payment_info_v1(&self, id: Uuid) -> Result<InvoicePaymentInfoV1Dto, InvoicesError> {
        let row = sqlx::query(
            "SELECT number, invoice_date, total_amount::text FROM invoices WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let paid: String = sqlx::query(
            "SELECT COALESCE(SUM(ABS(amount)), 0)::text AS p FROM supplier_ledger \
             WHERE document_id = $1 AND operation_type IN ('payment','return')",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?
        .get("p");
        let paid = rdec(&paid).round_dp(2);
        let total = rdec(
            &r.get::<Option<String>, _>("total_amount")
                .unwrap_or_default(),
        )
        .round_dp(2);
        let remaining = (total - paid).round_dp(2);
        Ok(InvoicePaymentInfoV1Dto {
            invoice_id: id,
            invoice_number: r.get("number"),
            invoice_date: r.get("invoice_date"),
            total_amount: format!("{total:.2}"),
            paid_amount: format!("{paid:.2}"),
            remaining: format!("{remaining:.2}"),
        })
    }

    /// Python POST /{id}/confirm {status: confirmed|cancelled}.
    async fn confirm_v1(&self, id: Uuid, status: &str) -> Result<InvoiceV1Dto, InvoicesError> {
        match status {
            "confirmed" => self.confirm_invoice_v1(id).await?,
            "cancelled" => self.cancel_invoice_v1(id).await?,
            _ => {
                return Err(InvoicesError::BadRequest(
                    "Невірний статус. Використовуйте 'confirmed' або 'cancelled'".into(),
                ))
            }
        };
        match self.fetch_v1(id).await? {
            Some(v) => Ok(v),
            None => Err(InvoicesError::Infrastructure("накладну не знайдено".into())),
        }
    }

    async fn price_changes(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError> {
        let exists: Option<Uuid> = sqlx::query("SELECT id FROM invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?
            .map(|r| r.get("id"));
        if exists.is_none() {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        }
        let rows = sqlx::query(
            "SELECT ii.product_id, ii.price::text AS inv_price, ii.previous_price::text AS prev, \
             p.title, p.barcode, p.sku, p.price::text AS cur \
             FROM invoice_items ii LEFT JOIN products p ON p.id = ii.product_id \
             WHERE ii.invoice_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            let Some(title) = r.get::<Option<String>, _>("title") else {
                continue;
            };
            let inv = rdec(&r.get::<String, _>("inv_price")).round_dp(2);
            let cur = rdec(&r.get::<String, _>("cur")).round_dp(2);
            let prev = rdec(&r.get::<Option<String>, _>("prev").unwrap_or_default()).round_dp(2);
            let diff = (prev - inv).round_dp(2);
            out.push(PriceChangeItemDto {
                product_id: r.get("product_id"),
                title,
                barcode: r.get("barcode"),
                article: r.get::<Option<String>, _>("sku"),
                invoice_price: inv.to_string(),
                current_price: cur.to_string(),
                changed: diff != rust_decimal::Decimal::ZERO,
                difference: diff.to_string(),
            });
        }
        Ok(out)
    }

    async fn print_items(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError> {
        crate::repositories::price_tag::print_invoice_items(self, id, req).await
    }
}

// ─── v2 ─────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl InvoicesV2Service for SqlxInvoices {
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
        // Python: InvoiceStatus(status).value — невалідний → ValueError (500).
        let status_val = match &status {
            Some(s) => Some(match s.as_str() {
                "draft" | "confirmed" | "cancelled" => s.clone(),
                other => {
                    return Err(InvoicesError::Infrastructure(format!(
                        "'{other}' is not a valid InvoiceStatus"
                    )))
                }
            }),
            None => None,
        };
        let mut where_sql = String::new();
        let mut binds: Vec<String> = vec![];
        let mut idx = 1usize;
        if let Some(q) = &search {
            where_sql.push_str(&format!(
                " WHERE (number ILIKE ${idx} OR COALESCE(notes,'') ILIKE ${idx})"
            ));
            binds.push(format!("%{q}%"));
            idx += 1;
        }
        if let Some(sid) = supplier_id {
            if where_sql.is_empty() {
                where_sql.push_str(" WHERE ");
            } else {
                where_sql.push_str(" AND ");
            }
            where_sql.push_str(&format!("supplier_id = ${idx}::uuid"));
            binds.push(sid.to_string());
            idx += 1;
        }
        if let Some(st) = &status_val {
            if where_sql.is_empty() {
                where_sql.push_str(" WHERE ");
            } else {
                where_sql.push_str(" AND ");
            }
            where_sql.push_str(&format!("status = '{}'", st));
        }
        if let Some(d) = date_from {
            if where_sql.is_empty() {
                where_sql.push_str(" WHERE ");
            } else {
                where_sql.push_str(" AND ");
            }
            where_sql.push_str(&format!("invoice_date >= ${idx}::timestamp"));
            binds.push(d.format("%Y-%m-%d %H:%M:%S").to_string());
            idx += 1;
        }
        if let Some(d) = date_to {
            if where_sql.is_empty() {
                where_sql.push_str(" WHERE ");
            } else {
                where_sql.push_str(" AND ");
            }
            where_sql.push_str(&format!("invoice_date <= ${idx}::timestamp"));
            binds.push(d.format("%Y-%m-%d %H:%M:%S").to_string());
            idx += 1;
        }
        let total: i64 = {
            let sql = format!("SELECT count(*) FROM invoices {where_sql}");
            let mut qb = sqlx::query(&sql);
            for b in &binds {
                qb = qb.bind(b);
            }
            qb.fetch_one(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?
                .get("count")
        };
        let offset = (page - 1) * size;
        let sql = format!(
            "SELECT id, number, supplier_id, status::text, total_amount::text, created_at, \
             notes FROM invoices {where_sql} \
             ORDER BY created_at DESC OFFSET ${idx} LIMIT ${idx2}",
            idx = idx,
            idx2 = idx + 1
        );
        let mut qb = sqlx::query(&sql);
        for b in &binds {
            qb = qb.bind(b);
        }
        qb = qb.bind(offset).bind(size);
        let rows = qb
            .fetch_all(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let iid: Uuid = r.get("id");
            let items = sqlx::query(
                "SELECT product_id, quantity::text, price::text FROM invoice_items WHERE invoice_id = $1",
            )
            .bind(iid)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
            let mut v2_items = Vec::with_capacity(items.len());
            for it in items {
                v2_items.push(InvoiceItemV2Dto {
                    product_id: it.get("product_id"),
                    quantity: f64n(&it.get::<String, _>("quantity")),
                    price: f64n(&it.get::<String, _>("price")),
                    tax_rate: 20,
                    name: String::new(),
                });
            }
            out.push(InvoiceV2Dto {
                id: iid,
                number: r.get("number"),
                supplier_id: r.get("supplier_id"),
                items: v2_items,
                total: r
                    .get::<Option<String>, _>("total_amount")
                    .as_deref()
                    .map(f64n),
                status: r.get("status"),
                created_at: r.get("created_at"),
                confirmed_at: None,
                notes: r.get::<Option<String>, _>("notes").unwrap_or_default(),
            });
        }
        Ok(InvoiceV2ListDto {
            items: out,
            total,
            page,
            size,
        })
    }

    async fn get_v2(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        match self.fetch_v2(id).await? {
            Some(v) => Ok(v),
            None => Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            ))),
        }
    }

    /// Python InvoiceUseCases.create_invoice (задумана семантика; Python 500).
    async fn create_v2(&self, input: &InvoiceCreateV2Input) -> Result<InvoiceV2Dto, InvoicesError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        // Перевіряємо існування постачальника.
        let sup: Option<Uuid> = sqlx::query("SELECT id FROM suppliers WHERE id = $1")
            .bind(input.supplier_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?
            .map(|r| r.get("id"));
        if sup.is_none() {
            return Err(InvoicesError::BadRequest(format!(
                "Постачальника з ID '{}' не знайдено",
                input.supplier_id
            )));
        }
        // Унікальність номера.
        let dup: Option<String> = sqlx::query("SELECT number FROM invoices WHERE number = $1")
            .bind(&input.number)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?
            .map(|r| r.get("number"));
        if dup.is_some() {
            return Err(InvoicesError::BadRequest(format!(
                "Накладна з номером '{}' вже існує",
                input.number
            )));
        }
        // total = сума qty*price (задуманий entity.total).
        let total: f64 = input.items.iter().map(|i| i.quantity * i.price).sum();
        let new_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO invoices \
             (id, number, supplier_id, invoice_date, status, notes, total_amount, is_fiscal, \
              created_by_id, store_id, created_at, updated_at) \
             VALUES ($1,$2,$3, now(), 'draft', $4, $5::numeric, false, \
              (SELECT id FROM users ORDER BY created_at LIMIT 1), $6, now(), now())",
        )
        .bind(new_id)
        .bind(&input.number)
        .bind(input.supplier_id)
        .bind(&input.notes)
        .bind(total.to_string())
        .bind(store_id)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in &input.items {
            sqlx::query(
                "INSERT INTO invoice_items \
                 (invoice_id, product_id, quantity, price, total, cost_price, markup_percent, \
                  previous_price, store_id, created_at) \
                 VALUES ($1,$2,$3::numeric,$4::numeric,$5::numeric,$5::numeric,'0', NULL, $6, now())",
            )
            .bind(new_id)
            .bind(it.product_id)
            .bind(it.quantity.to_string())
            .bind(it.price.to_string())
            .bind((it.quantity * it.price).to_string())
            .bind(store_id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        match self.fetch_v2(new_id).await? {
            Some(v) => Ok(v),
            None => Err(InvoicesError::Infrastructure(
                "створена накладна не знайдена".into(),
            )),
        }
    }

    /// Python InvoiceUseCases.confirm_invoice (задумана; Python 500).
    async fn confirm_v2(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        let row = sqlx::query(
            "SELECT i.status::text AS st, i.is_fiscal, i.supplier_id, i.number, \
             i.total_amount::text AS total, i.invoice_date, i.created_at, i.store_id \
             FROM invoices i WHERE i.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let st: String = r.get("st");
        if st != "draft" {
            return Err(InvoicesError::BadRequest(format!(
                "Накладна вже має статус '{st}'"
            )));
        }
        let is_fiscal: bool = r.get("is_fiscal");
        let supplier_id: Uuid = r.get("supplier_id");
        let number: String = r.get("number");
        let total: String = r
            .get::<Option<String>, _>("total")
            .unwrap_or_else(|| "0".into());
        let store_id: Option<Uuid> = r.try_get("store_id").ok().flatten();
        let _store_id = store_id.ok_or_else(|| {
            de(format!("Накладну з ID '{id}' не прив'язано до точки"))
        })?;
        let op_date: NaiveDateTime = r
            .get::<Option<NaiveDateTime>, _>("invoice_date")
            .unwrap_or_else(|| r.get("created_at"));
        // Оновлюємо залишки: stock+; fiscal+; price = item.price ЗАВЖДИ (v2 use case).
        let items = sqlx::query(
            "SELECT product_id, quantity::text AS q, price::text AS p, \
             COALESCE(cost_price, 0)::text AS cp FROM invoice_items WHERE invoice_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in items {
            let pid: Uuid = it.get("product_id");
            let q = it.get::<String, _>("q");
            self.update_stock(pid, &q).await?;
            if is_fiscal {
                self.fiscal_stock(pid, &q, true).await?;
            }
            // Python v2: product.change_price(item.price) — ЗАВЖДИ;
            // cost_price не оновлюється (у v2 DTO немає cost_price).
            sqlx::query(
                "UPDATE products SET price = $1::numeric, updated_at = now() WHERE id = $2",
            )
            .bind(it.get::<String, _>("p"))
            .bind(pid)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        // ledger INVOICE (борг +).
        let notes = format!("Прибуткова накладна №{number}");
        self.ledger_entry(supplier_id, "invoice", &total, op_date, id, &number, &notes)
            .await?;
        sqlx::query("UPDATE invoices SET status = 'confirmed', updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        match self.fetch_v2(id).await? {
            Some(v) => Ok(v),
            None => Err(InvoicesError::Infrastructure("накладну не знайдено".into())),
        }
    }

    async fn update_v2(
        &self,
        id: Uuid,
        input: &InvoiceUpdateV2Input,
    ) -> Result<InvoiceV2Dto, InvoicesError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        let row = sqlx::query("SELECT status::text AS st FROM invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let st: String = r.get("st");
        if st != "draft" {
            return Err(InvoicesError::BadRequest(
                "Можна редагувати тільки чернетки".into(),
            ));
        }
        let mut sets: Vec<String> = vec![];
        let mut vals: Vec<String> = vec![];
        let mut idx = 1usize;
        macro_rules! push_set {
            ($col:expr, $cast:expr, $val:expr) => {
                sets.push(format!("{}=${}::{}", $col, idx, $cast));
                vals.push($val.to_string());
                idx += 1;
            };
        }
        if let Some(v) = &input.number {
            push_set!("number", "text", v);
        }
        if let Some(v) = input.supplier_id {
            push_set!("supplier_id", "uuid", v);
        }
        if let Some(v) = &input.notes {
            push_set!("notes", "text", v);
        }
        if let Some(v) = input.is_fiscal {
            push_set!("is_fiscal", "boolean", if v { "true" } else { "false" });
        }
        if let Some(v) = input.invoice_date {
            push_set!("invoice_date", "timestamp", v.format("%Y-%m-%d %H:%M:%S"));
        }
        if !sets.is_empty() {
            let mut q = "UPDATE invoices SET ".to_string();
            q.push_str(&sets.join(", "));
            q.push_str(&format!(", updated_at = now() WHERE id = ${idx}"));
            let mut qb = sqlx::query(&q);
            for v in &vals {
                qb = qb.bind(v);
            }
            qb = qb.bind(id);
            qb.execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
        }
        // Python: items повністю замінюють; total = sum(float(q)*float(p)).
        if let Some(items) = &input.items {
            sqlx::query("DELETE FROM invoice_items WHERE invoice_id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            let total: f64 = items.iter().map(|i| i.quantity * i.price).sum();
            for it in items {
                sqlx::query(
                    "INSERT INTO invoice_items \
                     (invoice_id, product_id, quantity, price, total, cost_price, markup_percent, \
                      previous_price, store_id, created_at) \
                     VALUES ($1,$2,$3::numeric,$4::numeric,$5::numeric,$5::numeric,'0', NULL, $6, now())",
                )
                .bind(id)
                .bind(it.product_id)
                .bind(it.quantity.to_string())
                .bind(it.price.to_string())
                .bind((it.quantity * it.price).to_string())
                .bind(store_id)
                .execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            }
            sqlx::query("UPDATE invoices SET total_amount = $1::numeric WHERE id = $2")
                .bind(total.to_string())
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
        }
        match self.fetch_v2(id).await? {
            Some(v) => Ok(v),
            None => Err(InvoicesError::Infrastructure("накладну не знайдено".into())),
        }
    }

    async fn delete_v2(&self, id: Uuid) -> Result<(), InvoicesError> {
        let row = sqlx::query("SELECT status::text AS st FROM invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let st: String = r.get("st");
        if st != "draft" {
            return Err(InvoicesError::BadRequest(
                "Можна видалити тільки чернетку".into(),
            ));
        }
        sqlx::query("DELETE FROM invoices WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    async fn payment_info_v2(&self, id: Uuid) -> Result<InvoicePaymentInfoV2Dto, InvoicesError> {
        let row = sqlx::query(
            "SELECT number, invoice_date, total_amount::text FROM invoices WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let paid: f64 = sqlx::query(
            "SELECT COALESCE(SUM(ABS(amount)), 0)::text AS p FROM supplier_ledger \
             WHERE document_id = $1 AND operation_type IN ('payment','return')",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?
        .get::<String, _>("p")
        .parse()
        .unwrap_or(0.0);
        let total = r
            .get::<Option<String>, _>("total_amount")
            .as_deref()
            .map(f64n)
            .unwrap_or(0.0);
        Ok(InvoicePaymentInfoV2Dto {
            invoice_id: id,
            invoice_number: r.get("number"),
            invoice_date: r.get("invoice_date"),
            total_amount: total,
            paid_amount: paid,
            remaining: total - paid,
        })
    }

    async fn price_changes_v2(&self, id: Uuid) -> Result<Vec<PriceChangeItemDto>, InvoicesError> {
        <Self as InvoicesV1Service>::price_changes(self, id).await
    }

    async fn print_items_v2(
        &self,
        id: Uuid,
        req: &InvoicePrintRequest,
    ) -> Result<InvoicePrintDto, InvoicesError> {
        crate::repositories::price_tag::print_invoice_items(self, id, req).await
    }

    /// Python InvoiceUseCases.cancel_invoice (задумана; Python 500).
    async fn cancel_v2(&self, id: Uuid) -> Result<InvoiceV2Dto, InvoicesError> {
        let row = sqlx::query(
            "SELECT i.status::text AS st, i.is_fiscal, i.supplier_id, i.number, \
             i.total_amount::text AS total, i.invoice_date, i.created_at, i.store_id \
             FROM invoices i WHERE i.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let st: String = r.get("st");
        if st != "confirmed" {
            return Err(InvoicesError::BadRequest(format!(
                "Накладну з ID '{id}' не знайдено або вона не в статусі confirmed"
            )));
        }
        let is_fiscal: bool = r.get("is_fiscal");
        let supplier_id: Uuid = r.get("supplier_id");
        let total: f64 = r
            .get::<Option<String>, _>("total")
            .as_deref()
            .map(f64n)
            .unwrap_or(0.0);
        let store_id: Option<Uuid> = r.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            de(format!("Накладну з ID '{id}' не прив'язано до точки"))
        })?;
        // Відкат залишків: stock−; fiscal max(0, ...).
        let items = sqlx::query(
            "SELECT product_id, quantity::text AS q FROM invoice_items WHERE invoice_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in items {
            let pid: Uuid = it.get("product_id");
            let q = it.get::<String, _>("q");
            // Python v2: product.stock = stock - qty (Quantity не допускає від'ємних).
            sqlx::query(
                "UPDATE stock SET quantity = GREATEST(0, quantity - $1::numeric), updated_at = now()
                 WHERE store_id = $2 AND product_id = $3",
            )
            .bind(&q)
            .bind(store_id)
            .bind(pid)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
            if is_fiscal {
                self.fiscal_stock(pid, &q, false).await?;
            }
        }
        // Python v2 cancel: supplier.reduce_balance(invoice.total) — domain entity,
        // колонки balance у БД НЕМАЄ (SQLAlchemy Supplier не зберігає balance);
        // тому баланс не оновлюється (як і в Python — entity не маппиться в БД).
        let _ = (total, supplier_id);
        sqlx::query("UPDATE invoices SET status = 'cancelled', updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        match self.fetch_v2(id).await? {
            Some(v) => Ok(v),
            None => Err(InvoicesError::Infrastructure("накладну не знайдено".into())),
        }
    }
}

// ─── v1 confirm/cancel (document_service) ────────────────────────────────────

impl SqlxInvoices {
    /// Python DocumentService.confirm_invoice.
    async fn confirm_invoice_v1(&self, id: Uuid) -> Result<(), InvoicesError> {
        let row = sqlx::query(
            "SELECT i.status::text AS st, i.is_fiscal, i.supplier_id, i.number, \
             i.total_amount::text AS total, i.payment_method::text AS pm, \
             i.invoice_date, i.created_at, i.store_id \
             FROM invoices i WHERE i.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let st: String = r.get("st");
        if st != "draft" {
            return Err(InvoicesError::BadRequest(format!(
                "Накладна вже має статус '{st}'"
            )));
        }
        let is_fiscal: bool = r.get("is_fiscal");
        let supplier_id: Uuid = r.get("supplier_id");
        let number: String = r.get("number");
        let total: String = r
            .get::<Option<String>, _>("total")
            .unwrap_or_else(|| "0".into());
        let store_id: Option<Uuid> = r.try_get("store_id").ok().flatten();
        let _store_id = store_id.ok_or_else(|| {
            de(format!("Накладну з ID '{id}' не прив'язано до точки"))
        })?;
        let pm: Option<String> = r.get("pm");
        let op_date: NaiveDateTime = r
            .get::<Option<NaiveDateTime>, _>("invoice_date")
            .unwrap_or_else(|| r.get("created_at"));
        let items = sqlx::query(
            "SELECT product_id, quantity::text AS q, price::text AS p, \
             COALESCE(cost_price, 0)::text AS cp, previous_price::text AS pp \
             FROM invoice_items WHERE invoice_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in items {
            let pid: Uuid = it.get("product_id");
            let q = it.get::<String, _>("q");
            self.update_stock(pid, &q).await?;
            if is_fiscal {
                self.fiscal_stock(pid, &q, true).await?;
            }
            // Python v1: cost_price > 0 → cost_price = cp; price = p;
            // previous_price зберегти якщо None.
            let cp = it.get::<String, _>("cp");
            if rdec(&cp) > rust_decimal::Decimal::ZERO {
                let pp: Option<String> = it.get("pp");
                let mut need_pp = pp.clone();
                if pp.is_none() {
                    let prow = sqlx::query("SELECT price::text AS p FROM products WHERE id = $1")
                        .bind(pid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| de(e.to_string()))?;
                    need_pp = prow.map(|rr| rr.get::<String, _>("p"));
                }
                if pp.is_none() {
                    sqlx::query("UPDATE invoice_items SET previous_price = $1::numeric WHERE invoice_id = $2 AND product_id = $3")
                        .bind(need_pp.as_deref())
                        .bind(id)
                        .bind(pid)
                        .execute(&self.pool)
                        .await
                        .map_err(|e| de(e.to_string()))?;
                }
                sqlx::query(
                    "UPDATE products SET cost_price = $1::numeric, price = $2::numeric, updated_at = now() WHERE id = $3",
                )
                .bind(&cp)
                .bind(it.get::<String, _>("p"))
                .bind(pid)
                .execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            }
        }
        // ledger INVOICE: notes + payment method label.
        let mut notes = format!("Прибуткова накладна №{number}");
        if let Some(m) = &pm {
            let label = match m.as_str() {
                "credit" => "в борг",
                "bank_transfer" => "по перерахунку",
                "cash" => "готівкою з каси",
                "other" => "інший спосіб",
                _ => m.as_str(),
            };
            notes.push_str(&format!(" ({label})"));
        }
        self.ledger_entry(supplier_id, "invoice", &total, op_date, id, &number, &notes)
            .await?;
        sqlx::query("UPDATE invoices SET status = 'confirmed', updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Python DocumentService.cancel_invoice.
    async fn cancel_invoice_v1(&self, id: Uuid) -> Result<(), InvoicesError> {
        let row = sqlx::query(
            "SELECT status::text AS st, is_fiscal, supplier_id, number, total_amount::text AS total, \
             store_id FROM invoices WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(r) = row else {
            return Err(InvoicesError::NotFound(format!(
                "Накладну з ID '{id}' не знайдено"
            )));
        };
        let st: String = r.get("st");
        if st != "confirmed" {
            return Err(InvoicesError::BadRequest(
                "Скасувати можна лише підтверджену накладну".into(),
            ));
        }
        let is_fiscal: bool = r.get("is_fiscal");
        let supplier_id: Uuid = r.get("supplier_id");
        let number: String = r.get("number");
        let total: String = r
            .get::<Option<String>, _>("total")
            .unwrap_or_else(|| "0".into());
        let store_id: Option<Uuid> = r.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            de(format!("Накладну з ID '{id}' не прив'язано до точки"))
        })?;
        let items = sqlx::query(
            "SELECT product_id, quantity::text AS q FROM invoice_items WHERE invoice_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in items {
            let pid: Uuid = it.get("product_id");
            let q = it.get::<String, _>("q");
            // Python product_service.update_stock(quantity_change=-qty) → stock table.
            sqlx::query(
                "UPDATE stock SET quantity = GREATEST(0, quantity - $1::numeric), updated_at = now()
                 WHERE store_id = $2 AND product_id = $3",
            )
            .bind(&q)
            .bind(store_id)
            .bind(pid)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
            if is_fiscal {
                self.fiscal_stock(pid, &q, false).await?;
            }
        }
        // CORRECTION-запис, якщо існує INVOICE-запис для накладної.
        let has_invoice: bool = sqlx::query(
            "SELECT 1 FROM supplier_ledger WHERE document_id = $1 AND operation_type = 'invoice' LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?
        .is_some();
        if has_invoice {
            let neg = (rust_decimal::Decimal::ZERO - rdec(&total)).to_string();
            let notes = format!("Скасування накладної №{number}");
            self.ledger_entry(
                supplier_id,
                "correction",
                &neg,
                chrono::Utc::now().naive_utc(),
                id,
                &number,
                &notes,
            )
            .await?;
        }
        sqlx::query("UPDATE invoices SET status = 'cancelled', updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(())
    }
}
