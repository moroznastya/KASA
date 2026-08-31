//! Репозиторій повернень постачальнику (етап 8 — група 4): SqlxReturnInvoices.
//!
//! 1:1 з Python:
//!   - api/v1/return_invoices.py (7 роутів): list (пагінація), get, create
//!     (автономер ПВ-{YYYYMMDD}-{XXX}, total=sum, cost_price з продукту,
//!     markup=calc_markup_percent), update (тільки draft, items замінюються),
//!     delete (тільки draft), confirm/cancel (document_service 1:1).
//!
//! АНОМАЛІЯ PYTHON (зафіксовано 2026-08-07): confirm з return_action=exchange
//! падає 500 — Invoice(created_by_id=None) при NOT NULL колонці (doc_service
//! не отримує current_user). Rust реалізує ЗАДУМАНУ семантику (exchange-
//! накладна створюється з created_by_id = адмін, що підтверджує повернення).
//! Усі інші гілки — 1:1, включаючи cancel, який НЕ видаляє ledger-записи
//! (Python також не відкатує ledger при скасуванні).

use crate::store_ctx::{current_store_ctx, StorePool};
use chrono::NaiveDateTime;
use sqlx::Row;
use uuid::Uuid;

use torgashka_domain::return_invoices::{
    ExchangeInvoiceBriefDto, ExchangeInvoiceItemBriefDto, ProductBriefDto,
    ReturnInvoiceCreateInput, ReturnInvoiceDto, ReturnInvoiceItemCreateInput, ReturnInvoiceItemDto,
    ReturnInvoiceListDto, ReturnInvoiceUpdateInput, ReturnInvoicesError, ReturnInvoicesService,
};

fn de(s: String) -> ReturnInvoicesError {
    ReturnInvoicesError::Infrastructure(s)
}

/// Decimal-рядок з numeric::text (як Python Decimal str).
fn den(s: &str) -> String {
    s.trim().to_string()
}

/// rust_decimal з рядка.
fn rdec(s: &str) -> rust_decimal::Decimal {
    s.trim()
        .parse::<rust_decimal::Decimal>()
        .unwrap_or_default()
}

/// Дата-час як Python Pydantic v2: ISO без Z, з мікросекундами.
fn fmt_dt(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.f").to_string()
}

/// Python calc_markup_percent: ((price - cost) / cost) * 100, round(,2);
/// None якщо cost/price не > 0.
fn calc_markup(price: &str, cost: Option<&str>) -> Option<String> {
    let p = f64n(price);
    let c = cost.and_then(|s| f64n(s).gt(&0.0).then(|| f64n(s)))?;
    if p <= 0.0 || c <= 0.0 {
        return None;
    }
    let raw = ((p - c) / c) * 100.0;
    let r = (raw * 100.0).round() / 100.0;
    Some(format!("{:.2}", r))
}

fn f64n(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// Python RETURN_ACTION_LABELS.
fn action_label(action: &str) -> String {
    match action {
        "deduct_from_debt" => "списано з боргу постачальника".to_string(),
        "add_to_cash" => "зачислено в касу".to_string(),
        "exchange" => "обмін на інший товар".to_string(),
        other => other.to_string(),
    }
}

const RET_COLS: &str = "r.id, r.number, r.supplier_id, r.return_date, r.status::text, \
     r.return_action::text, r.is_fiscal, r.notes, r.total_amount::text, \
     r.exchange_invoice_id, r.source_invoice_id, r.created_by_id, \
     r.created_at, r.updated_at, s.name AS supplier_name";

/// Репозиторій повернень постачальнику.
pub struct SqlxReturnInvoices {
    pool: StorePool,
}

impl SqlxReturnInvoices {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }

    /// Python generate_return_number: ПВ-{YYYYMMDD UTC}-{XXX}.
    async fn next_number(&self) -> Result<String, ReturnInvoicesError> {
        let today = chrono::Utc::now().format("%Y%m%d").to_string();
        let prefix = format!("ПВ-{today}-");
        let row = sqlx::query(
            "SELECT number FROM return_invoices WHERE number LIKE $1 ORDER BY number DESC LIMIT 1",
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

    /// Python get_product_cost_info: (product.cost_price, None) або (None, None).
    async fn product_cost(&self, product_id: Uuid) -> Result<Option<String>, ReturnInvoicesError> {
        let row = sqlx::query("SELECT cost_price::text AS c FROM products WHERE id = $1")
            .bind(product_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(match row {
            Some(r) => r.get::<Option<String>, _>("c"),
            None => None,
        })
    }

    /// Вставка позицій (Python create/update: cost_price з продукту якщо None,
    /// markup = calc_markup_percent).
    async fn insert_items(
        &self,
        return_id: Uuid,
        items: &[ReturnInvoiceItemCreateInput],
    ) -> Result<(), ReturnInvoicesError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        for it in items {
            let cost = match &it.cost_price {
                Some(c) => Some(c.clone()),
                None => self.product_cost(it.product_id).await?,
            };
            let markup = calc_markup(&it.price, cost.as_deref());
            sqlx::query(
                "INSERT INTO return_invoice_items \
                 (return_invoice_id, product_id, quantity, price, total, cost_price, markup_percent, \
                  store_id, created_at) \
                 VALUES ($1,$2,$3::numeric,$4::numeric,$5::numeric,$6::numeric,$7::numeric,$8, now())",
            )
            .bind(return_id)
            .bind(it.product_id)
            .bind(&it.quantity)
            .bind(&it.price)
            .bind(&it.total)
            .bind(cost.as_deref())
            .bind(markup.as_deref())
            .bind(store_id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        Ok(())
    }

    /// Повна відповідь ReturnInvoiceDto (Python ReturnInvoiceResponse з
    /// selectinload items/product, exchange_invoice/items/product, supplier).
    async fn fetch(&self, id: Uuid) -> Result<Option<ReturnInvoiceDto>, ReturnInvoicesError> {
        let row = sqlx::query(&format!(
            "SELECT {RET_COLS} FROM return_invoices r LEFT JOIN suppliers s ON s.id = r.supplier_id WHERE r.id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let Some(row) = row else { return Ok(None) };
        let return_date: NaiveDateTime = row.get("return_date");
        let created_at: NaiveDateTime = row.get("created_at");
        let updated_at: NaiveDateTime = row.get("updated_at");
        let exchange_invoice_id: Option<Uuid> = row.get("exchange_invoice_id");

        let items = self.fetch_items(id).await?;
        let exchange_invoice = match exchange_invoice_id {
            Some(ei_id) => self.fetch_exchange(ei_id).await?,
            None => None,
        };

        Ok(Some(ReturnInvoiceDto {
            id: row.get("id"),
            number: row.get("number"),
            supplier_id: row.get("supplier_id"),
            supplier_name: row.get("supplier_name"),
            return_date: fmt_dt(return_date),
            status: row.get("status"),
            return_action: row.get("return_action"),
            is_fiscal: row.get("is_fiscal"),
            notes: row.get("notes"),
            total_amount: row.get("total_amount"),
            exchange_invoice_id,
            exchange_invoice,
            source_invoice_id: row.get("source_invoice_id"),
            created_at: fmt_dt(created_at),
            updated_at: fmt_dt(updated_at),
            items,
        }))
    }

    /// Позиції повернення (Python ReturnInvoiceItemResponse + ProductBrief).
    async fn fetch_items(
        &self,
        return_id: Uuid,
    ) -> Result<Vec<ReturnInvoiceItemDto>, ReturnInvoicesError> {
        let rows = sqlx::query(
            "SELECT ri.id, ri.return_invoice_id, ri.product_id, ri.quantity::text, \
             ri.price::text, ri.total::text, ri.cost_price::text, ri.markup_percent::text, \
             ri.created_at, p.title, p.barcode \
             FROM return_invoice_items ri JOIN products p ON p.id = ri.product_id \
             WHERE ri.return_invoice_id = $1 ORDER BY ri.created_at, ri.id",
        )
        .bind(return_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let created_at: NaiveDateTime = r.get("created_at");
            out.push(ReturnInvoiceItemDto {
                id: r.get("id"),
                return_invoice_id: r.get("return_invoice_id"),
                product_id: r.get("product_id"),
                product: Some(ProductBriefDto {
                    id: r.get("product_id"),
                    title: r.get("title"),
                    barcode: r.get("barcode"),
                }),
                quantity: den(&r.get::<String, _>("quantity")),
                price: den(&r.get::<String, _>("price")),
                cost_price: r.get::<Option<String>, _>("cost_price").map(|s| den(&s)),
                markup_percent: r
                    .get::<Option<String>, _>("markup_percent")
                    .map(|s| den(&s)),
                total: den(&r.get::<String, _>("total")),
                created_at: fmt_dt(created_at),
            });
        }
        Ok(out)
    }

    /// Прибуткова накладна при обміні (Python ExchangeInvoiceBrief + items).
    async fn fetch_exchange(
        &self,
        invoice_id: Uuid,
    ) -> Result<Option<ExchangeInvoiceBriefDto>, ReturnInvoicesError> {
        let row = sqlx::query("SELECT id, number, total_amount::text FROM invoices WHERE id = $1")
            .bind(invoice_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(row) = row else { return Ok(None) };
        let rows = sqlx::query(
            "SELECT ii.id, ii.product_id, ii.quantity::text, ii.price::text, ii.total::text, \
             p.title, p.barcode \
             FROM invoice_items ii JOIN products p ON p.id = ii.product_id \
             WHERE ii.invoice_id = $1 ORDER BY ii.created_at, ii.id",
        )
        .bind(invoice_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut items = Vec::with_capacity(rows.len());
        for r in rows {
            items.push(ExchangeInvoiceItemBriefDto {
                id: r.get("id"),
                product_id: r.get("product_id"),
                product: Some(ProductBriefDto {
                    id: r.get("product_id"),
                    title: r.get("title"),
                    barcode: r.get("barcode"),
                }),
                quantity: den(&r.get::<String, _>("quantity")),
                price: den(&r.get::<String, _>("price")),
                total: den(&r.get::<String, _>("total")),
            });
        }
        Ok(Some(ExchangeInvoiceBriefDto {
            id: row.get("id"),
            number: row.get("number"),
            total_amount: row
                .get::<Option<String>, _>("total_amount")
                .map(|s| den(&s)),
            items,
        }))
    }

    /// Python product_service.update_stock (stock += qty) → stock table (per store).
    /// Від'ємна зміна (повернення постачальнику): атомарний UPDATE з перевіркою
    /// достатності (quantity >= need); 0 рядків → 400 Python-стилю.
    async fn update_stock(&self, product_id: Uuid, qty: &str) -> Result<(), ReturnInvoicesError> {
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        // ФІКС 2026-08-21: products.stock (сумарний залишок, Python-еталон
        // product.update_stock) оновлюється ТИМ САМИМ знаком, що й stock table.
        if qty.starts_with('-') {
            let need = qty.trim_start_matches('-');
            let res = sqlx::query(
                "UPDATE stock SET quantity = quantity + $1::numeric, updated_at = now()
                 WHERE store_id = $2 AND product_id = $3 AND quantity >= $4::numeric",
            )
            .bind(qty)
            .bind(store_id)
            .bind(product_id)
            .bind(need)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
            if res.rows_affected() == 0 {
                let avail: Option<String> = sqlx::query_scalar(
                    "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
                )
                .bind(store_id)
                .bind(product_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?
                .flatten();
                let avail = avail.unwrap_or_else(|| "0".to_string());
                return Err(ReturnInvoicesError::BadRequest(format!(
                    "Недостатньо товару на складі. Доступно: {}, потрібно: {}",
                    avail, need
                )));
            }
            sqlx::query(
                "UPDATE products SET stock = GREATEST(0, COALESCE(stock, 0) - $1::numeric), updated_at = now()
                 WHERE id = $2",
            )
            .bind(need)
            .bind(product_id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        } else {
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
            sqlx::query(
                "UPDATE products SET stock = COALESCE(stock, 0) + $1::numeric, updated_at = now()
                 WHERE id = $2",
            )
            .bind(qty)
            .bind(product_id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        Ok(())
    }

    /// Python _decrease_fiscal_stock (max 0) / _increase_fiscal_stock.
    async fn fiscal_stock(
        &self,
        product_id: Uuid,
        qty: &str,
        increase: bool,
    ) -> Result<(), ReturnInvoicesError> {
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

    /// Python create_ledger_entry (balance_after = current + amount).
    async fn ledger_entry(
        &self,
        supplier_id: Uuid,
        amount: &str,
        operation_date: NaiveDateTime,
        document_id: Uuid,
        document_number: &str,
        notes: &str,
    ) -> Result<(), ReturnInvoicesError> {
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
             VALUES ($1,'return'::ledger_operation_type,$2,$3,$4::numeric,$5::numeric,$6,$7, now())",
        )
        .bind(supplier_id)
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

    /// Python generate_invoice_number (для exchange-накладної).
    async fn next_invoice_number(&self) -> Result<String, ReturnInvoicesError> {
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

    /// Python session-семантика: грошові поля позицій = вхідні значення
    /// (не перезавантажені з numeric-колонки), cost_price = вхідне або з
    /// продукту, markup = calc_markup_percent.
    async fn apply_session_items(
        &self,
        dto: &mut ReturnInvoiceDto,
        items: &[ReturnInvoiceItemCreateInput],
    ) -> Result<(), ReturnInvoicesError> {
        for (dto_item, in_item) in dto.items.iter_mut().zip(items.iter()) {
            dto_item.quantity = in_item.quantity.clone();
            dto_item.price = in_item.price.clone();
            dto_item.total = in_item.total.clone();
            let cost = match &in_item.cost_price {
                Some(c) => Some(c.clone()),
                None => self.product_cost(in_item.product_id).await?,
            };
            dto_item.cost_price = cost.clone();
            dto_item.markup_percent = calc_markup(&in_item.price, cost.as_deref());
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ReturnInvoicesService for SqlxReturnInvoices {
    async fn list(
        &self,
        page: i64,
        size: i64,
    ) -> Result<ReturnInvoiceListDto, ReturnInvoicesError> {
        // Python: count + pages = max(1, ceil).
        let total: i64 = sqlx::query("SELECT count(*)::int8 AS c FROM return_invoices")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?
            .get("c");
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        let offset = (page - 1) * size;
        let ids: Vec<Uuid> = sqlx::query(
            "SELECT id FROM return_invoices ORDER BY created_at DESC, id DESC LIMIT $1 OFFSET $2",
        )
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?
        .iter()
        .map(|r| r.get("id"))
        .collect();
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(dto) = self.fetch(id).await? {
                items.push(dto);
            }
        }
        Ok(ReturnInvoiceListDto {
            items,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn get(&self, id: Uuid) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        match self.fetch(id).await? {
            Some(dto) => Ok(dto),
            None => Err(ReturnInvoicesError::NotFound(format!(
                "Повернення з ID '{id}' не знайдено"
            ))),
        }
    }

    async fn create(
        &self,
        input: &ReturnInvoiceCreateInput,
        user_id: Uuid,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        // Python: total_amount = data.total_amount; if None and items: sum(total).
        let total_amount = match &input.total_amount {
            Some(t) => Some(t.clone()),
            None if !input.items.is_empty() => {
                let mut s = rust_decimal::Decimal::ZERO;
                for it in &input.items {
                    s += rdec(&it.total);
                }
                Some(s.to_string())
            }
            None => None,
        };
        let number = match &input.number {
            Some(n) if !n.is_empty() => n.clone(),
            _ => self.next_number().await?,
        };
        // Python: exchange вимагає exchange_items.
        if input.return_action == "exchange"
            && input
                .exchange_items
                .as_ref()
                .map(|v| v.is_empty())
                .unwrap_or(true)
        {
            return Err(ReturnInvoicesError::BadRequest(
                "Для обміну (exchange) необхідно вказати exchange_items — \
                 список товарів, на які відбувається обмін"
                    .to_string(),
            ));
        }
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| de("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO return_invoices \
             (id, number, supplier_id, return_date, status, return_action, is_fiscal, notes, \
              total_amount, source_invoice_id, created_by_id, store_id, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,'draft'::return_invoice_status,$5::return_action_type,$6,$7,$8::numeric,$9,$10,$11, now(), now())",
        )
        .bind(id)
        .bind(&number)
        .bind(input.supplier_id)
        .bind(input.return_date)
        .bind(&input.return_action)
        .bind(input.is_fiscal)
        .bind(input.notes.as_deref())
        .bind(total_amount.as_deref())
        .bind(input.source_invoice_id)
        .bind(user_id)
        .bind(store_id)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        self.insert_items(id, &input.items).await?;
        match self.fetch(id).await? {
            Some(mut dto) => {
                // Python повертає ЗНАЧЕННЯ СЕСІЇ (вхідні Decimal без scale
                // колонки): quantity/price/total з input, cost_price з input
                // або продукту, markup = calc_markup_percent.
                dto.total_amount = total_amount.clone();
                self.apply_session_items(&mut dto, &input.items).await?;
                Ok(dto)
            }
            None => Err(ReturnInvoicesError::Infrastructure(
                "повернення не знайдено після створення".into(),
            )),
        }
    }

    async fn update(
        &self,
        id: Uuid,
        input: &ReturnInvoiceUpdateInput,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        let row = sqlx::query("SELECT status::text AS s FROM return_invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(row) = row else {
            return Err(ReturnInvoicesError::NotFound(format!(
                "Повернення з ID '{id}' не знайдено"
            )));
        };
        let status: String = row.get("s");
        if status != "draft" {
            return Err(ReturnInvoicesError::BadRequest(
                "Можна редагувати тільки чернетки".to_string(),
            ));
        }
        // Python update_data exclude_unset, exclude={"items","exchange_items"}.
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
        if let Some(v) = input.return_date {
            push_set!("return_date", "timestamp", v.format("%Y-%m-%d %H:%M:%S"));
        }
        if let Some(v) = &input.return_action {
            push_set!("return_action", "return_action_type", v);
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
        if let Some(v) = input.source_invoice_id {
            push_set!("source_invoice_id", "uuid", v);
        }
        if !sets.is_empty() {
            let mut q = "UPDATE return_invoices SET ".to_string();
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
        // Python: items замінюються повністю.
        if let Some(items) = &input.items {
            sqlx::query("DELETE FROM return_invoice_items WHERE return_invoice_id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            self.insert_items(id, items).await?;
        }
        match self.fetch(id).await? {
            Some(mut dto) => {
                // Python: session-значення для items (якщо передані).
                if let Some(items) = &input.items {
                    self.apply_session_items(&mut dto, items).await?;
                }
                Ok(dto)
            }
            None => Err(ReturnInvoicesError::Infrastructure(
                "повернення не знайдено після оновлення".into(),
            )),
        }
    }

    async fn delete(&self, id: Uuid) -> Result<(), ReturnInvoicesError> {
        let row = sqlx::query("SELECT status::text AS s FROM return_invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(row) = row else {
            return Err(ReturnInvoicesError::NotFound(format!(
                "Повернення з ID '{id}' не знайдено"
            )));
        };
        let status: String = row.get("s");
        if status != "draft" {
            return Err(ReturnInvoicesError::BadRequest(
                "Можна видалити тільки чернетку".to_string(),
            ));
        }
        sqlx::query("DELETE FROM return_invoices WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    async fn confirm(
        &self,
        id: Uuid,
        input: &torgashka_domain::return_invoices::ReturnInvoiceConfirmInput,
        user_id: Uuid,
    ) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        // Python confirm роут: status == "cancelled" → cancel_return_invoice.
        if input.status == "cancelled" {
            return self.cancel(id).await;
        }
        if input.status != "confirmed" {
            return Err(ReturnInvoicesError::BadRequest(
                "Невірний статус. Використовуйте 'confirmed' або 'cancelled'".to_string(),
            ));
        }
        let Some(dto) = self.fetch(id).await? else {
            return Err(ReturnInvoicesError::NotFound(format!(
                "Повернення з ID '{id}' не знайдено"
            )));
        };
        if dto.status != "draft" {
            return Err(ReturnInvoicesError::BadRequest(format!(
                "Повернення вже має статус '{}'",
                dto.status
            )));
        }
        // Зменшуємо залишки повернутих товарів + фіскальний залишок.
        for it in &dto.items {
            self.update_stock(it.product_id, &format!("-{}", it.quantity))
                .await?;
            if dto.is_fiscal {
                self.fiscal_stock(it.product_id, &it.quantity, false)
                    .await?;
            }
        }
        // Python: notes = "Повернення постачальнику №N (action_label)".
        let action_label = action_label(&dto.return_action);
        let mut notes = format!(
            "Повернення постачальнику №{} ({})",
            dto.number, action_label
        );
        // doc_id: source_invoice_id або id повернення.
        let (doc_id, doc_number) = match dto.source_invoice_id {
            Some(src) => {
                let src_number: Option<String> =
                    sqlx::query("SELECT number FROM invoices WHERE id = $1")
                        .bind(src)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| de(e.to_string()))?
                        .map(|r| r.get("number"));
                let num = src_number.unwrap_or_else(|| dto.number.clone());
                notes.push_str(&format!(" (прив'язано до накладної №{num})"));
                (src, num)
            }
            None => (dto.id, dto.number.clone()),
        };
        // Дія згідно return_action (тільки якщо total_amount > 0).
        let total_gt_0 = dto
            .total_amount
            .as_deref()
            .map(|t| rdec(t) > rust_decimal::Decimal::ZERO)
            .unwrap_or(false);
        if total_gt_0 {
            match dto.return_action.as_str() {
                "deduct_from_debt" => {
                    let neg = format!("-{}", dto.total_amount.as_deref().unwrap_or("0"));
                    self.ledger_entry(
                        dto.supplier_id,
                        &neg,
                        parse_dt(&dto.return_date),
                        doc_id,
                        &doc_number,
                        &notes,
                    )
                    .await?;
                }
                "add_to_cash" => {
                    self.ledger_entry(
                        dto.supplier_id,
                        "0.00",
                        parse_dt(&dto.return_date),
                        doc_id,
                        &doc_number,
                        &format!("{notes} (сума зачислена в касу)"),
                    )
                    .await?;
                }
                "exchange" => {
                    let Some(exch) = &input.exchange_items else {
                        return Err(ReturnInvoicesError::BadRequest(
                            "Для обміну (exchange) необхідно вказати exchange_items — \
                             список товарів, на які відбувається обмін"
                                .to_string(),
                        ));
                    };
                    if exch.is_empty() {
                        return Err(ReturnInvoicesError::BadRequest(
                            "Для обміну (exchange) необхідно вказати exchange_items — \
                             список товарів, на які відбувається обмін"
                                .to_string(),
                        ));
                    }
                    let invoice_number = self.next_invoice_number().await?;
                    let mut exchange_total = rust_decimal::Decimal::ZERO;
                    for it in exch {
                        exchange_total += rdec(&it.total);
                    }
                    let new_id = Uuid::new_v4();
                    // Python: Invoice(...) БЕЗ created_by_id → 500 (NOT NULL);
                    // Rust — задумана семантика: created_by_id = адмін.
                    sqlx::query(
                        "INSERT INTO invoices \
                         (id, number, supplier_id, invoice_date, payment_method, is_fiscal, notes, \
                          total_amount, status, created_by_id, created_at, updated_at) \
                         VALUES ($1,$2,$3,$4,'credit'::payment_method,$5,$6,$7::numeric,'confirmed'::invoice_status,$8, now(), now())",
                    )
                    .bind(new_id)
                    .bind(&invoice_number)
                    .bind(dto.supplier_id)
                    .bind(parse_dt(&dto.return_date))
                    .bind(dto.is_fiscal)
                    .bind(format!(
                        "Автоматично створено при обміні з повернення №{}",
                        dto.number
                    ))
                    .bind(exchange_total.to_string())
                    .bind(user_id)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                    for it in exch {
                        sqlx::query(
                            "INSERT INTO invoice_items (invoice_id, product_id, quantity, price, total, created_at) \
                             VALUES ($1,$2,$3::numeric,$4::numeric,$5::numeric, now())",
                        )
                        .bind(new_id)
                        .bind(it.product_id)
                        .bind(&it.quantity)
                        .bind(&it.price)
                        .bind(&it.total)
                        .execute(&self.pool)
                        .await
                        .map_err(|e| de(e.to_string()))?;
                    }
                    for it in exch {
                        self.update_stock(it.product_id, &it.quantity).await?;
                    }
                    sqlx::query(
                        "UPDATE return_invoices SET exchange_invoice_id = $1 WHERE id = $2",
                    )
                    .bind(new_id)
                    .bind(dto.id)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                    self.ledger_entry(
                        dto.supplier_id,
                        "0.00",
                        parse_dt(&dto.return_date),
                        doc_id,
                        &doc_number,
                        &format!("{notes} (створено прибуткову накладну №{invoice_number})"),
                    )
                    .await?;
                }
                _ => {}
            }
        }
        sqlx::query(
            "UPDATE return_invoices SET status = 'confirmed'::return_invoice_status, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        match self.fetch(id).await? {
            Some(dto) => Ok(dto),
            None => Err(ReturnInvoicesError::Infrastructure(
                "повернення не знайдено після підтвердження".into(),
            )),
        }
    }

    async fn cancel(&self, id: Uuid) -> Result<ReturnInvoiceDto, ReturnInvoicesError> {
        let Some(dto) = self.fetch(id).await? else {
            return Err(ReturnInvoicesError::NotFound(format!(
                "Повернення з ID '{id}' не знайдено"
            )));
        };
        if dto.status != "confirmed" {
            return Err(ReturnInvoicesError::BadRequest(
                "Скасувати можна лише підтверджене повернення".to_string(),
            ));
        }
        // Python: якщо був обмін — скасовуємо прибуткову накладну.
        if let Some(exch) = &dto.exchange_invoice {
            if exch.items.iter().any(|_| true) {
                // status накладної: Python перевіряє exchange_inv.status == CONFIRMED.
                let st: Option<String> =
                    sqlx::query("SELECT status::text AS s FROM invoices WHERE id = $1")
                        .bind(dto.exchange_invoice_id)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| de(e.to_string()))?
                        .map(|r| r.get("s"));
                if st.as_deref() == Some("confirmed") {
                    for it in &exch.items {
                        self.update_stock(it.product_id, &format!("-{}", it.quantity))
                            .await?;
                    }
                    sqlx::query(
                        "UPDATE invoices SET status = 'cancelled'::invoice_status, updated_at = now() WHERE id = $1",
                    )
                    .bind(dto.exchange_invoice_id)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                }
            }
        }
        // Відкатуємо залишки повернутих товарів.
        for it in &dto.items {
            self.update_stock(it.product_id, &it.quantity).await?;
            if dto.is_fiscal {
                self.fiscal_stock(it.product_id, &it.quantity, true).await?;
            }
        }
        // Python НЕ видаляє ledger-записи при скасуванні — 1:1.
        sqlx::query(
            "UPDATE return_invoices SET status = 'cancelled'::return_invoice_status, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        match self.fetch(id).await? {
            Some(dto) => Ok(dto),
            None => Err(ReturnInvoicesError::Infrastructure(
                "повернення не знайдено після скасування".into(),
            )),
        }
    }
}

/// Парсинг ISO-дати Python Pydantic (без Z) у NaiveDateTime.
fn parse_dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .unwrap_or_else(|_| chrono::Utc::now().naive_utc())
}
