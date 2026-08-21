//! Репозиторій документів (етап 8 — група 2): SqlxDocuments.
//!
//! 1:1 з Python v1/documents.py: list (6 типів), batch-confirm (5 типів),
//! delete (5 типів), copy (5 типів), export (flat+detailed), print (5 типів).
//!
//! Грошові поля numeric повертаються як `::text` і парсяться у f64
//! (Python list/export конвертують Decimal у float).

use chrono::NaiveDateTime;
use serde_json::{json, Value};
use sqlx::{Row};
use crate::store_ctx::StorePool;
use uuid::Uuid;

use torgashka_domain::documents::{
    BatchConfirmErrorDto, BatchConfirmInput, BatchConfirmResultDto, DocListDto, DocListQuery,
    DocPrintDto, DocumentDto, DocumentsError, DocumentsService, ExportData, ExportQuery,
};

fn de(s: String) -> DocumentsError {
    DocumentsError::Infrastructure(s)
}

/// f64 з numeric::text (Python `float(Decimal)`).
fn f64n(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// JSON-число як Python: 0 (int) для нуля, float інакше.
fn jnum(v: f64) -> serde_json::Value {
    if v == 0.0 {
        serde_json::json!(0)
    } else {
        serde_json::json!(v)
    }
}

/// Python str(float): "600.0" для цілих, "0" для нуля (int), інакше to_string.
fn pystr(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else if v.fract() == 0.0 {
        format!("{:.1}", v)
    } else {
        v.to_string()
    }
}

/// ISO-дата created_at (Python `datetime.isoformat()`): без trailing-нулів.
fn iso(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.f").to_string()
}

fn day_str(dt: NaiveDateTime) -> String {
    dt.format("%d.%m.%Y").to_string()
}

fn day_time_str(dt: NaiveDateTime) -> String {
    dt.format("%d.%m.%Y %H:%M").to_string()
}

/// Безпечний пошук підрядка (Python `search.lower() in (number or '').lower()`).
fn search_match(search: &Option<String>, number: &Option<String>) -> bool {
    match search {
        Some(s) if !s.is_empty() => {
            let n = number.as_deref().unwrap_or("").to_lowercase();
            n.contains(&s.to_lowercase())
        }
        _ => true,
    }
}

/// Структура репозиторію.
pub struct SqlxDocuments {
    pool: StorePool,
}

impl SqlxDocuments {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }

    // ── Приватні хелпери: читання повних документів (copy/print) ────────────

    /// Повна InvoiceResponse-форма (Python InvoiceResponse.model_validate + supplier_name).
    async fn read_invoice_json(&self, id: Uuid) -> Result<Value, DocumentsError> {
        let row = sqlx::query(
            r#"SELECT i.id, i.number, i.supplier_id, i.invoice_date, i.status::text,
                      i.payment_method::text, i.is_fiscal, i.notes, i.total_amount::text,
                      i.created_at, i.updated_at, s.name AS supplier_name
               FROM invoices i LEFT JOIN suppliers s ON s.id = i.supplier_id
               WHERE i.id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = row
            .ok_or_else(|| DocumentsError::NotFound(format!("Накладну з ID '{id}' не знайдено")))?;
        let items = self.read_invoice_items(id).await?;
        Ok(json!({
            "id": row.get::<Uuid, _>("id"),
            "number": row.get::<String, _>("number"),
            "supplier_id": row.get::<Uuid, _>("supplier_id"),
            "supplier_name": row.get::<Option<String>, _>("supplier_name"),
            "invoice_date": iso(row.get::<NaiveDateTime, _>("invoice_date")),
            "status": row.get::<String, _>("status"),
            "payment_method": row.get::<Option<String>, _>("payment_method"),
            "is_fiscal": row.get::<bool, _>("is_fiscal"),
            "notes": row.get::<Option<String>, _>("notes"),
            "total_amount": row.get::<Option<String>, _>("total_amount"),
            "created_at": iso(row.get::<NaiveDateTime, _>("created_at")),
            "updated_at": iso(row.get::<NaiveDateTime, _>("updated_at")),
            "items": items,
        }))
    }

    async fn read_invoice_items(&self, id: Uuid) -> Result<Vec<Value>, DocumentsError> {
        let rows = sqlx::query(
            r#"SELECT ii.id, ii.invoice_id, ii.product_id, ii.quantity::text, ii.price::text,
                      ii.total::text, ii.cost_price::text, ii.markup_percent::text,
                      ii.previous_price::text, ii.created_at,
                      p.title, p.barcode, p.price::text AS p_price, p.markup::text AS p_markup, p.cost_price::text AS p_cost
               FROM invoice_items ii
               LEFT JOIN products p ON p.id = ii.product_id
               WHERE ii.invoice_id = $1"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            let product: Option<Value> = r.get::<Option<Uuid>, _>("product_id").map(|pid| {
                json!({
                    "id": pid,
                    "title": r.get::<Option<String>, _>("title"),
                    "barcode": r.get::<Option<String>, _>("barcode"),
                    "price": r.get::<Option<String>, _>("p_price"),
                    "markup": r.get::<Option<String>, _>("p_markup"),
                    "cost_price": r.get::<Option<String>, _>("p_cost"),
                })
            });
            out.push(json!({
                "id": r.get::<Uuid, _>("id"),
                "invoice_id": r.get::<Uuid, _>("invoice_id"),
                "product_id": r.get::<Option<Uuid>, _>("product_id"),
                "product": product,
                "quantity": r.get::<String, _>("quantity"),
                "price": r.get::<String, _>("price"),
                "total": r.get::<String, _>("total"),
                "cost_price": r.get::<Option<String>, _>("cost_price"),
                "markup_percent": r.get::<Option<String>, _>("markup_percent"),
                "previous_price": r.get::<Option<String>, _>("previous_price"),
                "created_at": iso(r.get::<NaiveDateTime, _>("created_at")),
            }));
        }
        Ok(out)
    }

    /// Повна ReturnInvoiceResponse-форма.
    async fn read_return_invoice_json(&self, id: Uuid) -> Result<Value, DocumentsError> {
        let row = sqlx::query(
            r#"SELECT r.id, r.number, r.supplier_id, r.return_date, r.status::text,
                      r.return_action::text, r.is_fiscal, r.notes, r.total_amount::text,
                      r.exchange_invoice_id, r.source_invoice_id, r.created_at, r.updated_at,
                      s.name AS supplier_name
               FROM return_invoices r LEFT JOIN suppliers s ON s.id = r.supplier_id
               WHERE r.id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = row.ok_or_else(|| {
            DocumentsError::NotFound(format!("Повернення з ID '{id}' не знайдено"))
        })?;
        let items = self.read_return_items(id).await?;
        Ok(json!({
            "id": row.get::<Uuid, _>("id"),
            "number": row.get::<String, _>("number"),
            "supplier_id": row.get::<Uuid, _>("supplier_id"),
            "supplier_name": row.get::<Option<String>, _>("supplier_name"),
            "return_date": iso(row.get::<NaiveDateTime, _>("return_date")),
            "status": row.get::<String, _>("status"),
            "return_action": row.get::<String, _>("return_action"),
            "is_fiscal": row.get::<bool, _>("is_fiscal"),
            "notes": row.get::<Option<String>, _>("notes"),
            "total_amount": row.get::<Option<String>, _>("total_amount"),
            "exchange_invoice_id": row.get::<Option<Uuid>, _>("exchange_invoice_id"),
            "exchange_invoice": Value::Null,
            "source_invoice_id": row.get::<Option<Uuid>, _>("source_invoice_id"),
            "created_at": iso(row.get::<NaiveDateTime, _>("created_at")),
            "updated_at": iso(row.get::<NaiveDateTime, _>("updated_at")),
            "items": items,
        }))
    }

    async fn read_return_items(&self, id: Uuid) -> Result<Vec<Value>, DocumentsError> {
        let rows = sqlx::query(
            r#"SELECT ri.id, ri.return_invoice_id, ri.product_id, ri.quantity::text,
                      ri.price::text, ri.total::text, ri.cost_price::text,
                      ri.markup_percent::text, ri.created_at,
                      p.title, p.barcode, p.price::text AS p_price, p.markup::text AS p_markup, p.cost_price::text AS p_cost
               FROM return_invoice_items ri
               LEFT JOIN products p ON p.id = ri.product_id
               WHERE ri.return_invoice_id = $1"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            let product: Option<Value> = r.get::<Option<Uuid>, _>("product_id").map(|pid| {
                json!({
                    "id": pid,
                    "title": r.get::<Option<String>, _>("title"),
                    "barcode": r.get::<Option<String>, _>("barcode"),
                    "price": r.get::<Option<String>, _>("p_price"),
                    "markup": r.get::<Option<String>, _>("p_markup"),
                    "cost_price": r.get::<Option<String>, _>("p_cost"),
                })
            });
            out.push(json!({
                "id": r.get::<Uuid, _>("id"),
                "return_invoice_id": r.get::<Uuid, _>("return_invoice_id"),
                "product_id": r.get::<Option<Uuid>, _>("product_id"),
                "product": product,
                "quantity": r.get::<String, _>("quantity"),
                "price": r.get::<String, _>("price"),
                "cost_price": r.get::<Option<String>, _>("cost_price"),
                "markup_percent": r.get::<Option<String>, _>("markup_percent"),
                "total": r.get::<String, _>("total"),
                "created_at": iso(r.get::<NaiveDateTime, _>("created_at")),
            }));
        }
        Ok(out)
    }

    /// Повна PurchaseOrderResponse-форма.
    async fn read_purchase_order_json(&self, id: Uuid) -> Result<Value, DocumentsError> {
        let row = sqlx::query(
            r#"SELECT p.id, p.number, p.supplier_id, p.order_date, p.expected_date,
                      p.status::text, p.is_fiscal, p.notes, p.total_amount::text,
                      p.invoice_id, p.created_at, p.updated_at, s.name AS supplier_name
               FROM purchase_orders p LEFT JOIN suppliers s ON s.id = p.supplier_id
               WHERE p.id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = row.ok_or_else(|| {
            DocumentsError::NotFound(format!("Замовлення з ID '{id}' не знайдено"))
        })?;
        let items = self.read_order_items(id).await?;
        Ok(json!({
            "id": row.get::<Uuid, _>("id"),
            "number": row.get::<String, _>("number"),
            "supplier_id": row.get::<Uuid, _>("supplier_id"),
            "supplier_name": row.get::<Option<String>, _>("supplier_name"),
            "order_date": iso(row.get::<NaiveDateTime, _>("order_date")),
            "expected_date": row.get::<Option<NaiveDateTime>, _>("expected_date").map(iso),
            "status": row.get::<String, _>("status"),
            "is_fiscal": row.get::<bool, _>("is_fiscal"),
            "notes": row.get::<Option<String>, _>("notes"),
            "total_amount": row.get::<Option<String>, _>("total_amount"),
            "invoice_id": row.get::<Option<Uuid>, _>("invoice_id"),
            "invoice": Value::Null,
            "created_at": iso(row.get::<NaiveDateTime, _>("created_at")),
            "updated_at": iso(row.get::<NaiveDateTime, _>("updated_at")),
            "items": items,
        }))
    }

    async fn read_order_items(&self, id: Uuid) -> Result<Vec<Value>, DocumentsError> {
        let rows = sqlx::query(
            r#"SELECT po.id, po.purchase_order_id, po.product_id, po.quantity::text,
                      po.price::text, po.total::text, po.created_at,
                      p.title, p.barcode, p.price::text AS p_price, p.markup::text AS p_markup, p.cost_price::text AS p_cost
               FROM purchase_order_items po
               LEFT JOIN products p ON p.id = po.product_id
               WHERE po.purchase_order_id = $1"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            let product: Option<Value> = r.get::<Option<Uuid>, _>("product_id").map(|pid| {
                json!({
                    "id": pid,
                    "title": r.get::<Option<String>, _>("title"),
                    "barcode": r.get::<Option<String>, _>("barcode"),
                    "price": r.get::<Option<String>, _>("p_price"),
                    "markup": r.get::<Option<String>, _>("p_markup"),
                    "cost_price": r.get::<Option<String>, _>("p_cost"),
                })
            });
            out.push(json!({
                "id": r.get::<Uuid, _>("id"),
                "purchase_order_id": r.get::<Uuid, _>("purchase_order_id"),
                "product_id": r.get::<Option<Uuid>, _>("product_id"),
                "product": product,
                "quantity": r.get::<String, _>("quantity"),
                "price": r.get::<String, _>("price"),
                "total": r.get::<String, _>("total"),
                "created_at": iso(r.get::<NaiveDateTime, _>("created_at")),
            }));
        }
        Ok(out)
    }

    /// TransferDto-форма (Python TransferResponse).
    async fn read_transfer_json(&self, id: Uuid) -> Result<Value, DocumentsError> {
        let row = sqlx::query(
            r#"SELECT t.id, t.number, t.from_location, t.to_location, t.transfer_date,
                      t.status::text, t.notes, t.created_at, t.updated_at
               FROM transfers t WHERE t.id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = row.ok_or_else(|| {
            DocumentsError::NotFound(format!("Переміщення з ID '{id}' не знайдено"))
        })?;
        let rows = sqlx::query(
            r#"SELECT id, transfer_id, product_id, quantity::text, cost_price::text,
                      price::text, created_at
               FROM transfer_items WHERE transfer_id = $1"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.get::<Uuid, _>("id"),
                    "transfer_id": r.get::<Uuid, _>("transfer_id"),
                    "product_id": r.get::<Uuid, _>("product_id"),
                    "quantity": r.get::<String, _>("quantity"),
                    "cost_price": r.get::<String, _>("cost_price"),
                    "price": r.get::<String, _>("price"),
                    "created_at": iso(r.get::<NaiveDateTime, _>("created_at")),
                })
            })
            .collect();
        Ok(json!({
            "id": row.get::<Uuid, _>("id"),
            "number": row.get::<String, _>("number"),
            "from_location": row.get::<String, _>("from_location"),
            "to_location": row.get::<String, _>("to_location"),
            "transfer_date": iso(row.get::<NaiveDateTime, _>("transfer_date")),
            "status": row.get::<String, _>("status"),
            "notes": row.get::<Option<String>, _>("notes"),
            "created_at": iso(row.get::<NaiveDateTime, _>("created_at")),
            "updated_at": iso(row.get::<NaiveDateTime, _>("updated_at")),
            "items": items,
        }))
    }

    /// WriteOffDto-форма (Python WriteOffResponse).
    async fn read_write_off_json(&self, id: Uuid) -> Result<Value, DocumentsError> {
        let row = sqlx::query(
            r#"SELECT w.id, w.number, w.reason::text, w.write_off_date, w.notes,
                      w.status::text, w.total_amount::text, w.created_at, w.updated_at
               FROM write_offs w WHERE w.id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = row
            .ok_or_else(|| DocumentsError::NotFound(format!("Списання з ID '{id}' не знайдено")))?;
        let rows = sqlx::query(
            r#"SELECT id, write_off_id, product_id, quantity::text, cost_price::text,
                      price::text, created_at
               FROM write_off_items WHERE write_off_id = $1"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.get::<Uuid, _>("id"),
                    "write_off_id": r.get::<Uuid, _>("write_off_id"),
                    "product_id": r.get::<Uuid, _>("product_id"),
                    "quantity": r.get::<String, _>("quantity"),
                    "cost_price": r.get::<String, _>("cost_price"),
                    "price": r.get::<String, _>("price"),
                    "created_at": iso(r.get::<NaiveDateTime, _>("created_at")),
                })
            })
            .collect();
        Ok(json!({
            "id": row.get::<Uuid, _>("id"),
            "number": row.get::<String, _>("number"),
            "reason": row.get::<String, _>("reason"),
            "write_off_date": iso(row.get::<NaiveDateTime, _>("write_off_date")),
            "notes": row.get::<Option<String>, _>("notes"),
            "status": row.get::<String, _>("status"),
            "total_amount": row.get::<Option<String>, _>("total_amount"),
            "created_at": iso(row.get::<NaiveDateTime, _>("created_at")),
            "updated_at": iso(row.get::<NaiveDateTime, _>("updated_at")),
            "items": items,
        }))
    }

    /// (title, barcode) товару для друку (Python select(Product); barcode None→null).
    async fn product_brief(&self, pid: Uuid) -> Result<(String, Option<String>), DocumentsError> {
        let row = sqlx::query("SELECT title, barcode AS b FROM products WHERE id = $1")
            .bind(pid)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        match row {
            Some(r) => Ok((r.get("title"), r.get::<Option<String>, _>("b"))),
            None => Ok(("Невідомий товар".to_string(), None)),
        }
    }

    /// Поточна роздрібна ціна товару (Python product.price).
    async fn product_price(&self, pid: Uuid) -> Result<f64, DocumentsError> {
        let row = sqlx::query("SELECT price::text AS p FROM products WHERE id = $1")
            .bind(pid)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(row.map(|r| f64n(&r.get::<String, _>("p"))).unwrap_or(0.0))
    }

    /// Генерація номера документа (Python generate_*_number).
    async fn next_doc_number(&self, table: &str, prefix: &str) -> Result<String, DocumentsError> {
        let today = chrono::Utc::now().naive_utc().format("%Y%m%d").to_string();
        let pfx = format!("{prefix}-{today}-");
        let q = format!("SELECT max(number) FROM {table} WHERE number LIKE $1");
        let row: (Option<String>,) = sqlx::query_as(&q)
            .bind(format!("{pfx}%"))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let last_seq = row
            .0
            .and_then(|n| {
                n.chars()
                    .rev()
                    .take(3)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
                    .parse::<i64>()
                    .ok()
            })
            .unwrap_or(0);
        Ok(format!("{pfx}{:03}", last_seq + 1))
    }
}
impl SqlxDocuments {
    /// Python: `sum(float(item.cost_price or 0) * float(item.quantity or 0) for item in inv.items)`
    /// — float-арифметика в тому ж порядку (SQL numeric-сума дає інший результат).
    async fn purchase_total_float(&self, invoice_id: Uuid) -> Result<f64, DocumentsError> {
        let rows = sqlx::query(
            r#"SELECT COALESCE(cost_price,0)::text AS cp, COALESCE(quantity,0)::text AS qty
           FROM invoice_items WHERE invoice_id = $1"#,
        )
        .bind(invoice_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut total = 0.0f64;
        for r in rows {
            total += f64n(&r.get::<String, _>("cp")) * f64n(&r.get::<String, _>("qty"));
        }
        Ok(total)
    }

    /// Python inventory-підсумки: float-суми (total_cost, total_selling, deviation).
    async fn inventory_sums(&self, inventory_id: Uuid) -> Result<(f64, f64, f64), DocumentsError> {
        let rows = sqlx::query(
            r#"SELECT COALESCE(actual_quantity,0)::text AS aq, COALESCE(cost_price,0)::text AS cp,
                  COALESCE(price,0)::text AS pr, COALESCE(difference,0)::text AS df
           FROM inventory_items WHERE inventory_id = $1"#,
        )
        .bind(inventory_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut tc = 0.0f64;
        let mut ts = 0.0f64;
        let mut td = 0.0f64;
        for r in rows {
            tc += f64n(&r.get::<String, _>("aq")) * f64n(&r.get::<String, _>("cp"));
            ts += f64n(&r.get::<String, _>("aq")) * f64n(&r.get::<String, _>("pr"));
            td += f64n(&r.get::<String, _>("df")) * f64n(&r.get::<String, _>("cp"));
        }
        Ok((tc, ts, td))
    }
}

#[async_trait::async_trait]
impl DocumentsService for SqlxDocuments {
    // ── СПИСОК ──────────────────────────────────────────────────────────────
    async fn list_documents(&self, q: &DocListQuery) -> Result<DocListDto, DocumentsError> {
        let mut all: Vec<(NaiveDateTime, DocumentDto)> = Vec::new();

        // ── Прибуткові накладні ──
        if q.document_type.is_none() || q.document_type.as_deref() == Some("invoice") {
            let mut sql = String::from(
                r#"SELECT i.id, i.number, i.status::text, i.total_amount::text, i.supplier_id,
                          i.created_at, i.created_by_id, s.name AS supplier_name, u.name AS creator
                   FROM invoices i
                   LEFT JOIN suppliers s ON s.id = i.supplier_id
                   LEFT JOIN users u ON u.id = i.created_by_id WHERE 1=1"#,
            );
            if let Some(d) = q.date_from {
                sql.push_str(&format!(
                    " AND i.created_at >= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(d) = q.date_to {
                sql.push_str(&format!(
                    " AND i.created_at <= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(a) = q.amount_from {
                sql.push_str(&format!(" AND i.total_amount >= {a}"));
            }
            if let Some(a) = q.amount_to {
                sql.push_str(&format!(" AND i.total_amount <= {a}"));
            }
            if let Some(sid) = q.supplier_id {
                sql.push_str(&format!(" AND i.supplier_id = '{sid}'"));
            }
            sql.push_str(" ORDER BY i.created_at DESC");
            let rows = sqlx::query(&sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            for r in rows {
                let status: String = r.get("status");
                if let Some(s) = &q.status {
                    if &status != s {
                        continue;
                    }
                }
                let number: String = r.get("number");
                if !search_match(&q.search, &Some(number.clone())) {
                    continue;
                }
                let created: NaiveDateTime = r.get("created_at");
                let pt = self.purchase_total_float(r.get::<Uuid, _>("id")).await?;
                all.push((
                    created,
                    DocumentDto {
                        id: r.get::<Uuid, _>("id").to_string(),
                        document_type: "invoice".into(),
                        document_number: number,
                        status,
                        total_amount: jnum(f64n(
                            &r.get::<Option<String>, _>("total_amount")
                                .unwrap_or_default(),
                        )),
                        purchase_total: Some(serde_json::json!(pt)),
                        supplier_name: r
                            .get::<Option<String>, _>("supplier_name")
                            .unwrap_or_default(),
                        supplier_id: r
                            .get::<Option<Uuid>, _>("supplier_id")
                            .map(|v| v.to_string()),
                        created_at: Some(iso(created)),
                        created_by: r
                            .get::<Option<Uuid>, _>("created_by_id")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        created_by_name: r.get::<Option<String>, _>("creator").unwrap_or_default(),
                        deviation_total: None,
                    },
                ));
            }
        }

        // ── Переміщення ──
        if q.document_type.is_none() || q.document_type.as_deref() == Some("transfer") {
            let mut sql = String::from(
                r#"SELECT t.id, t.number, t.status::text, t.created_at, t.created_by_id,
                          u.name AS creator
                   FROM transfers t LEFT JOIN users u ON u.id = t.created_by_id WHERE 1=1"#,
            );
            if let Some(d) = q.date_from {
                sql.push_str(&format!(
                    " AND t.created_at >= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(d) = q.date_to {
                sql.push_str(&format!(
                    " AND t.created_at <= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            sql.push_str(" ORDER BY t.created_at DESC");
            let rows = sqlx::query(&sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            for r in rows {
                let status: String = r.get("status");
                if let Some(s) = &q.status {
                    if &status != s {
                        continue;
                    }
                }
                let number: String = r.get("number");
                if !search_match(&q.search, &Some(number.clone())) {
                    continue;
                }
                let created: NaiveDateTime = r.get("created_at");
                all.push((
                    created,
                    DocumentDto {
                        id: r.get::<Uuid, _>("id").to_string(),
                        document_type: "transfer".into(),
                        document_number: number,
                        status,
                        total_amount: serde_json::json!(0),
                        purchase_total: None,
                        supplier_name: String::new(),
                        supplier_id: None,
                        created_at: Some(iso(created)),
                        created_by: r
                            .get::<Option<Uuid>, _>("created_by_id")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        created_by_name: r.get::<Option<String>, _>("creator").unwrap_or_default(),
                        deviation_total: None,
                    },
                ));
            }
        }

        // ── Списання ──
        if q.document_type.is_none() || q.document_type.as_deref() == Some("write_off") {
            let mut sql = String::from(
                r#"SELECT w.id, w.number, w.total_amount::text, w.status::text, w.created_at, w.created_by_id,
                          u.name AS creator,
                          (SELECT COALESCE(sum(p.price*woi.quantity),0)
                           FROM write_off_items woi JOIN products p ON p.id = woi.product_id
                           WHERE woi.write_off_id = w.id)::text AS calc_total
                   FROM write_offs w LEFT JOIN users u ON u.id = w.created_by_id WHERE 1=1"#,
            );
            if let Some(d) = q.date_from {
                sql.push_str(&format!(
                    " AND w.created_at >= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(d) = q.date_to {
                sql.push_str(&format!(
                    " AND w.created_at <= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(a) = q.amount_from {
                sql.push_str(&format!(" AND w.total_amount >= {a}"));
            }
            if let Some(a) = q.amount_to {
                sql.push_str(&format!(" AND w.total_amount <= {a}"));
            }
            sql.push_str(" ORDER BY w.created_at DESC");
            let rows = sqlx::query(&sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            for r in rows {
                let wo_status: String = r.get("status");
                if let Some(s) = &q.status {
                    if &wo_status != s {
                        continue;
                    }
                }
                let number: String = r.get("number");
                if !search_match(&q.search, &Some(number.clone())) {
                    continue;
                }
                let total: Option<String> = r.get("total_amount");
                let calc: String = r.get("calc_total");
                // Python: `float(wo.total_amount) if wo.total_amount else total` —
                // Decimal 0 (falsy) → розрахована сума по items.
                let total_amount = match &total {
                    Some(t) if f64n(t) != 0.0 => f64n(t),
                    _ => f64n(&calc),
                };
                let created: NaiveDateTime = r.get("created_at");
                all.push((
                    created,
                    DocumentDto {
                        id: r.get::<Uuid, _>("id").to_string(),
                        document_type: "write_off".into(),
                        document_number: number,
                        status: wo_status,
                        total_amount: serde_json::json!(total_amount),
                        purchase_total: None,
                        supplier_name: String::new(),
                        supplier_id: None,
                        created_at: Some(iso(created)),
                        created_by: r
                            .get::<Option<Uuid>, _>("created_by_id")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        created_by_name: r.get::<Option<String>, _>("creator").unwrap_or_default(),
                        deviation_total: None,
                    },
                ));
            }
        }

        // ── Повернення постачальнику ──
        if q.document_type.is_none() || q.document_type.as_deref() == Some("return_invoice") {
            let mut sql = String::from(
                r#"SELECT r.id, r.number, r.status::text, r.total_amount::text, r.supplier_id,
                          r.created_at, r.created_by_id, s.name AS supplier_name, u.name AS creator
                   FROM return_invoices r
                   LEFT JOIN suppliers s ON s.id = r.supplier_id
                   LEFT JOIN users u ON u.id = r.created_by_id WHERE 1=1"#,
            );
            if let Some(d) = q.date_from {
                sql.push_str(&format!(
                    " AND r.created_at >= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(d) = q.date_to {
                sql.push_str(&format!(
                    " AND r.created_at <= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(a) = q.amount_from {
                sql.push_str(&format!(" AND r.total_amount >= {a}"));
            }
            if let Some(a) = q.amount_to {
                sql.push_str(&format!(" AND r.total_amount <= {a}"));
            }
            if let Some(sid) = q.supplier_id {
                sql.push_str(&format!(" AND r.supplier_id = '{sid}'"));
            }
            sql.push_str(" ORDER BY r.created_at DESC");
            let rows = sqlx::query(&sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            for r in rows {
                let status: String = r.get("status");
                if let Some(s) = &q.status {
                    if &status != s {
                        continue;
                    }
                }
                let number: String = r.get("number");
                if !search_match(&q.search, &Some(number.clone())) {
                    continue;
                }
                let created: NaiveDateTime = r.get("created_at");
                all.push((
                    created,
                    DocumentDto {
                        id: r.get::<Uuid, _>("id").to_string(),
                        document_type: "return_invoice".into(),
                        document_number: number,
                        status,
                        total_amount: jnum(f64n(
                            &r.get::<Option<String>, _>("total_amount")
                                .unwrap_or_default(),
                        )),
                        purchase_total: None,
                        supplier_name: r
                            .get::<Option<String>, _>("supplier_name")
                            .unwrap_or_default(),
                        supplier_id: r
                            .get::<Option<Uuid>, _>("supplier_id")
                            .map(|v| v.to_string()),
                        created_at: Some(iso(created)),
                        created_by: r
                            .get::<Option<Uuid>, _>("created_by_id")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        created_by_name: r.get::<Option<String>, _>("creator").unwrap_or_default(),
                        deviation_total: None,
                    },
                ));
            }
        }

        // ── Замовлення постачальнику ──
        if q.document_type.is_none() || q.document_type.as_deref() == Some("purchase_order") {
            let mut sql = String::from(
                r#"SELECT p.id, p.number, p.status::text, p.total_amount::text, p.supplier_id,
                          p.created_at, p.created_by_id, s.name AS supplier_name, u.name AS creator
                   FROM purchase_orders p
                   LEFT JOIN suppliers s ON s.id = p.supplier_id
                   LEFT JOIN users u ON u.id = p.created_by_id WHERE 1=1"#,
            );
            if let Some(d) = q.date_from {
                sql.push_str(&format!(
                    " AND p.created_at >= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(d) = q.date_to {
                sql.push_str(&format!(
                    " AND p.created_at <= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(a) = q.amount_from {
                sql.push_str(&format!(" AND p.total_amount >= {a}"));
            }
            if let Some(a) = q.amount_to {
                sql.push_str(&format!(" AND p.total_amount <= {a}"));
            }
            if let Some(sid) = q.supplier_id {
                sql.push_str(&format!(" AND p.supplier_id = '{sid}'"));
            }
            sql.push_str(" ORDER BY p.created_at DESC");
            let rows = sqlx::query(&sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            for r in rows {
                let status: String = r.get("status");
                if let Some(s) = &q.status {
                    if &status != s {
                        continue;
                    }
                }
                let number: String = r.get("number");
                if !search_match(&q.search, &Some(number.clone())) {
                    continue;
                }
                let created: NaiveDateTime = r.get("created_at");
                all.push((
                    created,
                    DocumentDto {
                        id: r.get::<Uuid, _>("id").to_string(),
                        document_type: "purchase_order".into(),
                        document_number: number,
                        status,
                        total_amount: jnum(f64n(
                            &r.get::<Option<String>, _>("total_amount")
                                .unwrap_or_default(),
                        )),
                        purchase_total: None,
                        supplier_name: r
                            .get::<Option<String>, _>("supplier_name")
                            .unwrap_or_default(),
                        supplier_id: r
                            .get::<Option<Uuid>, _>("supplier_id")
                            .map(|v| v.to_string()),
                        created_at: Some(iso(created)),
                        created_by: r
                            .get::<Option<Uuid>, _>("created_by_id")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        created_by_name: r.get::<Option<String>, _>("creator").unwrap_or_default(),
                        deviation_total: None,
                    },
                ));
            }
        }

        // ── Інвентаризації ──
        if q.document_type.is_none() || q.document_type.as_deref() == Some("inventory") {
            let mut sql = String::from(
                r#"SELECT inv.id, inv.number, inv.status::text, inv.location, inv.created_at,
                          inv.created_by_id, u.name AS creator
                   FROM inventories inv LEFT JOIN users u ON u.id = inv.created_by_id WHERE 1=1"#,
            );
            if let Some(d) = q.date_from {
                sql.push_str(&format!(
                    " AND inv.created_at >= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            if let Some(d) = q.date_to {
                sql.push_str(&format!(
                    " AND inv.created_at <= '{}'",
                    d.format("%Y-%m-%d %H:%M:%S%.f")
                ));
            }
            sql.push_str(" ORDER BY inv.created_at DESC");
            let rows = sqlx::query(&sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
            for r in rows {
                let status: String = r.get("status");
                if let Some(s) = &q.status {
                    if &status != s {
                        continue;
                    }
                }
                let number: String = r.get("number");
                if !search_match(&q.search, &Some(number.clone())) {
                    continue;
                }
                let inv_id: Uuid = r.get("id");
                // Підсумки: total_cost, total_selling, deviation_total
                let (tc, ts, td) = self.inventory_sums(inv_id).await?;
                let created: NaiveDateTime = r.get("created_at");
                all.push((
                    created,
                    DocumentDto {
                        id: inv_id.to_string(),
                        document_type: "inventory".into(),
                        document_number: number,
                        status,
                        total_amount: jnum(ts),
                        purchase_total: Some(serde_json::json!(tc)),
                        supplier_name: r.get::<Option<String>, _>("location").unwrap_or_default(),
                        supplier_id: None,
                        created_at: Some(iso(created)),
                        created_by: r
                            .get::<Option<Uuid>, _>("created_by_id")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        created_by_name: r.get::<Option<String>, _>("creator").unwrap_or_default(),
                        deviation_total: Some(td),
                    },
                ));
            }
        }

        // Python: `all_documents.sort(key=lambda d: d["created_at"] or "", reverse=True)`
        all.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
        let total = all.len() as i64;
        let offset = ((q.page - 1) * q.size).max(0) as usize;
        let items: Vec<DocumentDto> = all
            .into_iter()
            .skip(offset)
            .take(q.size.max(0) as usize)
            .map(|(_, d)| d)
            .collect();
        let pages = if total > 0 {
            (total + q.size - 1) / q.size
        } else {
            1
        };
        Ok(DocListDto {
            items,
            total,
            page: q.page,
            page_size: q.size,
            pages,
        })
    }

    // ── BATCH-CONFIRM ───────────────────────────────────────────────────────
    async fn batch_confirm(
        &self,
        input: &BatchConfirmInput,
        user_id: Uuid,
    ) -> Result<BatchConfirmResultDto, DocumentsError> {
        let mut confirmed_count = 0i64;
        let mut errors: Vec<BatchConfirmErrorDto> = Vec::new();

        for id_str in &input.ids {
            let doc_id = match Uuid::parse_str(id_str) {
                Ok(v) => v,
                Err(_) => {
                    errors.push(BatchConfirmErrorDto {
                        id: id_str.clone(),
                        error: format!("Некоректний UUID: '{id_str}'"),
                    });
                    continue;
                }
            };
            let result: Result<(), DocumentsError> = match input.document_type.as_str() {
                "invoice" => self.confirm_invoice(doc_id).await,
                "transfer" => self.confirm_transfer(doc_id).await,
                "write_off" => self.confirm_write_off(doc_id).await,
                "return_invoice" => self.confirm_return_invoice(doc_id).await,
                "purchase_order" => self.confirm_purchase_order(doc_id, user_id).await,
                other => Err(DocumentsError::BadRequest(format!(
                    "Невідомий тип документа: '{other}'"
                ))),
            };
            match result {
                Ok(()) => confirmed_count += 1,
                Err(e) => errors.push(BatchConfirmErrorDto {
                    id: id_str.clone(),
                    error: e.to_string(),
                }),
            }
        }
        Ok(BatchConfirmResultDto {
            confirmed_count,
            errors,
        })
    }

    // ── DELETE ──────────────────────────────────────────────────────────────
    async fn delete_document(&self, id: Uuid, document_type: &str) -> Result<(), DocumentsError> {
        let (table, status_col, not_found_msg, not_draft_msg): (&str, &str, &str, &str) =
            match document_type {
                "invoice" => (
                    "invoices",
                    "status::text",
                    "Накладну з ID '{id}' не знайдено",
                    "Можна видалити тільки чернетку",
                ),
                "transfer" => (
                    "transfers",
                    "status::text",
                    "Переміщення з ID '{id}' не знайдено",
                    "Можна видалити тільки чернетку",
                ),
                "write_off" => (
                    "write_offs",
                    "status",
                    "Списання з ID '{id}' не знайдено",
                    "Можна видалити тільки чернетку",
                ),
                "return_invoice" => (
                    "return_invoices",
                    "status::text",
                    "Повернення з ID '{id}' не знайдено",
                    "Можна видалити тільки чернетку",
                ),
                "purchase_order" => (
                    "purchase_orders",
                    "status::text",
                    "Замовлення з ID '{id}' не знайдено",
                    "Можна видалити тільки чернетку",
                ),
                other => {
                    return Err(DocumentsError::BadRequest(format!(
                        "Невідомий тип документа: '{other}'. Доступні: invoice, transfer, write_off, return_invoice, purchase_order"
                    )))
                }
            };
        let q = format!("SELECT {status_col} AS st FROM {table} WHERE id = $1");
        let row = sqlx::query(&q)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        let row = row.ok_or_else(|| DocumentsError::NotFound(not_found_msg.to_string()))?;
        let st: String = row.get("st");
        // Python write_off: `getattr(doc, 'status', 'draft') != 'draft'` (колонка TEXT)
        if st != "draft" {
            return Err(DocumentsError::BadRequest(not_draft_msg.to_string()));
        }
        let dq = format!("DELETE FROM {table} WHERE id = $1");
        sqlx::query(&dq)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    // ── COPY ────────────────────────────────────────────────────────────────
    async fn copy_document(
        &self,
        id: Uuid,
        document_type: &str,
        user_id: Uuid,
    ) -> Result<Value, DocumentsError> {
        let now = chrono::Utc::now().naive_utc();
        match document_type {
            "invoice" => {
                let src = self.read_invoice_json(id).await?;
                let original_number = src["number"].as_str().unwrap_or("").to_string();
                let new_number = self.next_doc_number("invoices", "ПН").await?;
                let new_id = Uuid::new_v4();
                let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
                sqlx::query(
                    r#"INSERT INTO invoices (id, number, supplier_id, invoice_date, status,
                       payment_method, is_fiscal, notes, total_amount, created_by_id, created_at, updated_at)
                       VALUES ($1,$2,$3::uuid,$4,'draft',$5::payment_method,$6,$7,$8::numeric,$9,$10,$10)"#,
                )
                .bind(new_id)
                .bind(&new_number)
                .bind(src["supplier_id"].as_str().unwrap_or_default())
                .bind(now)
                .bind(src["payment_method"].as_str())
                .bind(src["is_fiscal"].as_bool().unwrap_or(false))
                .bind(format!(
                    "Копія накладної №{original_number}. {}",
                    src["notes"].as_str().unwrap_or("")
                ))
                .bind(src["total_amount"].as_str())
                .bind(user_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| de(e.to_string()))?;
                for it in src["items"].as_array().unwrap_or(&vec![]).iter() {
                    sqlx::query(
                        r#"INSERT INTO invoice_items (id, invoice_id, product_id, quantity, price, total,
                           cost_price, markup_percent, previous_price, created_at)
                           VALUES ($1,$2,$3::uuid,$4::numeric,$5::numeric,$6::numeric,$7::numeric,$8::numeric,$9::numeric,$10)"#,
                    )
                    .bind(Uuid::new_v4())
                    .bind(new_id)
                    .bind(it["product_id"].as_str().unwrap_or_default())
                    .bind(it["quantity"].as_str().unwrap_or("0"))
                    .bind(it["price"].as_str().unwrap_or("0"))
                    .bind(it["total"].as_str().unwrap_or("0"))
                    .bind(Some("0.00"))
                    .bind(Some("0.0"))
                    .bind(None::<&str>)
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                }
                tx.commit().await.map_err(|e| de(e.to_string()))?;
                let mut out = self.read_invoice_json(new_id).await?;
                // Python copy НЕ присвоює supplier_name (default None).
                out["supplier_name"] = Value::Null;
                Ok(out)
            }
            "transfer" => {
                let src = self.read_transfer_json(id).await?;
                let original_number = src["number"].as_str().unwrap_or("").to_string();
                let new_number = self.next_doc_number("transfers", "ТН").await?;
                let new_id = Uuid::new_v4();
                let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
                sqlx::query(
                    r#"INSERT INTO transfers (id, number, from_location, to_location, transfer_date,
                       status, notes, created_by_id, created_at, updated_at)
                       VALUES ($1,$2,$3,$4,$5,'draft',$6,$7,$8,$8)"#,
                )
                .bind(new_id)
                .bind(&new_number)
                .bind(src["from_location"].as_str().unwrap_or(""))
                .bind(src["to_location"].as_str().unwrap_or(""))
                .bind(now)
                .bind(format!(
                    "Копія переміщення №{original_number}. {}",
                    src["notes"].as_str().unwrap_or("")
                ))
                .bind(user_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| de(e.to_string()))?;
                for it in src["items"].as_array().unwrap_or(&vec![]).iter() {
                    sqlx::query(
                        r#"INSERT INTO transfer_items (id, transfer_id, product_id, quantity, created_at)
                           VALUES ($1,$2,$3::uuid,$4::numeric,$5)"#,
                    )
                    .bind(Uuid::new_v4())
                    .bind(new_id)
                    .bind(it["product_id"].as_str().unwrap_or_default())
                    .bind(it["quantity"].as_str().unwrap_or("0"))
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                }
                tx.commit().await.map_err(|e| de(e.to_string()))?;
                self.read_transfer_json(new_id).await
            }
            "write_off" => {
                let src = self.read_write_off_json(id).await?;
                let original_number = src["number"].as_str().unwrap_or("").to_string();
                let new_number = self.next_doc_number("write_offs", "СН").await?;
                let new_id = Uuid::new_v4();
                let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
                sqlx::query(
                    r#"INSERT INTO write_offs (id, number, reason, write_off_date, status,
                       notes, total_amount, created_by_id, created_at, updated_at)
                       VALUES ($1,$2,$3,$4,'draft',$5,$6,$7,$8::numeric,$8)"#,
                )
                .bind(new_id)
                .bind(&new_number)
                .bind(src["reason"].as_str().unwrap_or("other"))
                .bind(now)
                .bind(format!(
                    "Копія списання №{original_number}. {}",
                    src["notes"].as_str().unwrap_or("")
                ))
                .bind(src["total_amount"].as_str())
                .bind(user_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| de(e.to_string()))?;
                for it in src["items"].as_array().unwrap_or(&vec![]).iter() {
                    sqlx::query(
                        r#"INSERT INTO write_off_items (id, write_off_id, product_id, quantity,
                           cost_price, price, created_at) VALUES ($1,$2,$3::uuid,$4::numeric,$5::numeric,$6::numeric,$7)"#,
                    )
                    .bind(Uuid::new_v4())
                    .bind(new_id)
                    .bind(it["product_id"].as_str().unwrap_or_default())
                    .bind(it["quantity"].as_str().unwrap_or("0"))
                    .bind(it["cost_price"].as_str().unwrap_or("0"))
                    .bind(it["price"].as_str().unwrap_or("0"))
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                }
                tx.commit().await.map_err(|e| de(e.to_string()))?;
                self.read_write_off_json(new_id).await
            }
            "return_invoice" => {
                let src = self.read_return_invoice_json(id).await?;
                let original_number = src["number"].as_str().unwrap_or("").to_string();
                let new_number = self.next_doc_number("return_invoices", "ПВН").await?;
                let new_id = Uuid::new_v4();
                let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
                sqlx::query(
                    r#"INSERT INTO return_invoices (id, number, supplier_id, return_date, status,
                       return_action, is_fiscal, notes, total_amount, source_invoice_id, created_by_id,
                       created_at, updated_at)
                       VALUES ($1,$2,$3::uuid,$4,'draft',$5,$6,$7,$8,$9,$10::numeric,$11,$11)"#,
                )
                .bind(new_id)
                .bind(&new_number)
                .bind(src["supplier_id"].as_str().unwrap_or_default())
                .bind(now)
                .bind(src["return_action"].as_str().unwrap_or("deduct_from_debt"))
                .bind(src["is_fiscal"].as_bool().unwrap_or(false))
                .bind(format!(
                    "Копія повернення №{original_number}. {}",
                    src["notes"].as_str().unwrap_or("")
                ))
                .bind(src["total_amount"].as_str())
                .bind(src["source_invoice_id"].as_str())
                .bind(user_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| de(e.to_string()))?;
                for it in src["items"].as_array().unwrap_or(&vec![]).iter() {
                    sqlx::query(
                        r#"INSERT INTO return_invoice_items (id, return_invoice_id, product_id,
                           quantity, price, total, cost_price, markup_percent, created_at)
                           VALUES ($1,$2,$3::uuid,$4::numeric,$5::numeric,$6::numeric,$7::numeric,$8::numeric,$9)"#,
                    )
                    .bind(Uuid::new_v4())
                    .bind(new_id)
                    .bind(it["product_id"].as_str().unwrap_or_default())
                    .bind(it["quantity"].as_str().unwrap_or("0"))
                    .bind(it["price"].as_str().unwrap_or("0"))
                    .bind(it["total"].as_str().unwrap_or("0"))
                    .bind(it["cost_price"].as_str())
                    .bind(it["markup_percent"].as_str())
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                }
                tx.commit().await.map_err(|e| de(e.to_string()))?;
                self.read_return_invoice_json(new_id).await
            }
            "purchase_order" => {
                let src = self.read_purchase_order_json(id).await?;
                let original_number = src["number"].as_str().unwrap_or("").to_string();
                let new_number = self.next_doc_number("purchase_orders", "ЗМ").await?;
                let new_id = Uuid::new_v4();
                let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
                sqlx::query(
                    r#"INSERT INTO purchase_orders (id, number, supplier_id, order_date, expected_date,
                       status, is_fiscal, notes, total_amount, created_by_id, created_at, updated_at)
                       VALUES ($1,$2,$3,$4,$5,'draft',$6,$7,$8,$9,$10,$10)"#,
                )
                .bind(new_id)
                .bind(&new_number)
                .bind(src["supplier_id"].as_str().unwrap_or_default())
                .bind(now)
                .bind(src["expected_date"].as_str())
                .bind(src["is_fiscal"].as_bool().unwrap_or(false))
                .bind(format!(
                    "Копія замовлення №{original_number}. {}",
                    src["notes"].as_str().unwrap_or("")
                ))
                .bind(src["total_amount"].as_str())
                .bind(user_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| de(e.to_string()))?;
                for it in src["items"].as_array().unwrap_or(&vec![]).iter() {
                    sqlx::query(
                        r#"INSERT INTO purchase_order_items (id, purchase_order_id, product_id,
                           quantity, price, total, created_at) VALUES ($1,$2,$3::uuid,$4::numeric,$5::numeric,$6::numeric,$7)"#,
                    )
                    .bind(Uuid::new_v4())
                    .bind(new_id)
                    .bind(it["product_id"].as_str().unwrap_or_default())
                    .bind(it["quantity"].as_str().unwrap_or("0"))
                    .bind(it["price"].as_str().unwrap_or("0"))
                    .bind(it["total"].as_str().unwrap_or("0"))
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| de(e.to_string()))?;
                }
                tx.commit().await.map_err(|e| de(e.to_string()))?;
                self.read_purchase_order_json(new_id).await
            }
            other => Err(DocumentsError::BadRequest(format!(
                "Невідомий тип документа: '{other}'. Доступні: invoice, transfer, write_off, return_invoice, purchase_order"
            ))),
        }
    }

    // ── EXPORT ──────────────────────────────────────────────────────────────
    async fn export_documents(&self, q: &ExportQuery) -> Result<ExportData, DocumentsError> {
        if q.detailed {
            self.export_detailed(q).await
        } else {
            self.export_flat(q).await
        }
    }

    // ── PRINT ───────────────────────────────────────────────────────────────
    async fn print_document(
        &self,
        id: Uuid,
        document_type: &str,
    ) -> Result<DocPrintDto, DocumentsError> {
        // 1:1 Python v1/documents.py print_document (формати header/items/footer).
        let fmt_dmy = |v: &str| -> String {
            if v.len() >= 10 {
                format!("{}.{}.{}", &v[8..10], &v[5..7], &v[0..4])
            } else {
                v.to_string()
            }
        };
        match document_type {
            "invoice" => {
                let doc = self.read_invoice_json(id).await?;
                let mut items = Vec::new();
                let mut total_quantity = 0.0f64;
                for it in doc["items"].as_array().unwrap_or(&vec![]).iter() {
                    let qty = f64n(it["quantity"].as_str().unwrap_or("0"));
                    total_quantity += qty;
                    items.push(json!({
                        "product_name": it["product"]["title"].as_str().unwrap_or("Невідомий товар"),
                        "barcode": it["product"]["barcode"].as_str().map(|s| s.to_string()),
                        "quantity": qty,
                        "price": f64n(it["price"].as_str().unwrap_or("0")),
                        "total": f64n(it["total"].as_str().unwrap_or("0")),
                    }));
                }
                // Python: payment_method.value (raw) або "не вказано".
                let payment = doc["payment_method"].as_str().unwrap_or("");
                let payment_label = if payment.is_empty() {
                    "не вказано".to_string()
                } else {
                    payment.to_string()
                };
                let total_items = items.len();
                Ok(DocPrintDto {
                    header: json!({
                        "document_type": "Прибуткова накладна",
                        "document_number": doc["number"].as_str().unwrap_or(""),
                        "date": fmt_dmy(doc["invoice_date"].as_str().unwrap_or("")),
                        "supplier": doc["supplier_name"].as_str().unwrap_or("—"),
                        "payment_method": payment_label,
                        "status": doc["status"].as_str().unwrap_or(""),
                    }),
                    items,
                    footer: json!({
                        "total_amount": f64n(doc["total_amount"].as_str().unwrap_or("0")),
                        "total_quantity": total_quantity,
                        "total_items": total_items,
                    }),
                })
            }
            "transfer" => {
                let doc = self.read_transfer_json(id).await?;
                let mut items = Vec::new();
                let mut total_quantity = 0.0f64;
                for it in doc["items"].as_array().unwrap_or(&vec![]).iter() {
                    let pid: Uuid = it["product_id"].as_str().unwrap_or("").parse().unwrap_or_default();
                    let (title, barcode) = self
                        .product_brief(pid)
                        .await
                        .unwrap_or(("Невідомий товар".into(), None));
                    let qty = f64n(it["quantity"].as_str().unwrap_or("0"));
                    total_quantity += qty;
                    items.push(json!({
                        "product_name": title,
                        "barcode": barcode,
                        "quantity": qty,
                        "price": 0,
                        "total": 0,
                    }));
                }
                let total_items = items.len();
                Ok(DocPrintDto {
                    header: json!({
                        "document_type": "Переміщення товару",
                        "document_number": doc["number"].as_str().unwrap_or(""),
                        "date": fmt_dmy(doc["transfer_date"].as_str().unwrap_or("")),
                        "from_location": doc["from_location"].as_str().unwrap_or(""),
                        "to_location": doc["to_location"].as_str().unwrap_or(""),
                        "status": doc["status"].as_str().unwrap_or(""),
                    }),
                    items,
                    footer: json!({
                        "total_amount": 0,
                        "total_quantity": total_quantity,
                        "total_items": total_items,
                    }),
                })
            }
            "write_off" => {
                let doc = self.read_write_off_json(id).await?;
                let reason_names = [
                    ("expired", "Закінчився термін придатності"),
                    ("damaged", "Пошкодження / бій"),
                    ("defect", "Брак / дефект"),
                    ("theft", "Крадіжка"),
                    ("inventory", "Інвентаризація (нестача)"),
                    ("other", "Інше"),
                ];
                let reason_raw = doc["reason"].as_str().unwrap_or("");
                let reason = reason_names
                    .iter()
                    .find(|(k, _)| *k == reason_raw)
                    .map(|(_, v)| *v)
                    .unwrap_or(reason_raw)
                    .to_string();
                let mut items = Vec::new();
                let mut total_quantity = 0.0f64;
                let mut total_amount = 0.0f64;
                for it in doc["items"].as_array().unwrap_or(&vec![]).iter() {
                    let pid: Uuid = it["product_id"].as_str().unwrap_or("").parse().unwrap_or_default();
                    let (title, barcode) = self
                        .product_brief(pid)
                        .await
                        .unwrap_or(("Невідомий товар".into(), None));
                    let price = self.product_price(pid).await.unwrap_or(0.0);
                    let qty = f64n(it["quantity"].as_str().unwrap_or("0"));
                    let total = price * qty;
                    total_quantity += qty;
                    total_amount += total;
                    items.push(json!({
                        "product_name": title,
                        "barcode": barcode,
                        "quantity": qty,
                        "price": price,
                        "total": total,
                    }));
                }
                let total_items = items.len();
                Ok(DocPrintDto {
                    header: json!({
                        "document_type": "Списання товару",
                        "document_number": doc["number"].as_str().unwrap_or(""),
                        "date": fmt_dmy(doc["write_off_date"].as_str().unwrap_or("")),
                        "reason": reason,
                        "notes": doc["notes"].as_str().unwrap_or(""),
                    }),
                    items,
                    footer: json!({
                        "total_amount": total_amount,
                        "total_quantity": total_quantity,
                        "total_items": total_items,
                    }),
                })
            }
            "return_invoice" => {
                let doc = self.read_return_invoice_json(id).await?;
                let action_names = [
                    ("deduct_from_debt", "Списання з боргу постачальника"),
                    ("add_to_cash", "Зачислення в касу"),
                    ("exchange", "Обмін на інший товар"),
                ];
                let action_raw = doc["return_action"].as_str().unwrap_or("");
                let action = action_names
                    .iter()
                    .find(|(k, _)| *k == action_raw)
                    .map(|(_, v)| *v)
                    .unwrap_or(action_raw)
                    .to_string();
                let mut items = Vec::new();
                let mut total_quantity = 0.0f64;
                for it in doc["items"].as_array().unwrap_or(&vec![]).iter() {
                    let qty = f64n(it["quantity"].as_str().unwrap_or("0"));
                    total_quantity += qty;
                    items.push(json!({
                        "product_name": it["product"]["title"].as_str().unwrap_or("Невідомий товар"),
                        "barcode": it["product"]["barcode"].as_str().map(|s| s.to_string()),
                        "quantity": qty,
                        "price": f64n(it["price"].as_str().unwrap_or("0")),
                        "total": f64n(it["total"].as_str().unwrap_or("0")),
                    }));
                }
                let total_items = items.len();
                Ok(DocPrintDto {
                    header: json!({
                        "document_type": "Повернення постачальнику",
                        "document_number": doc["number"].as_str().unwrap_or(""),
                        "date": fmt_dmy(doc["return_date"].as_str().unwrap_or("")),
                        "supplier": doc["supplier_name"].as_str().unwrap_or("—"),
                        "action": action,
                        "status": doc["status"].as_str().unwrap_or(""),
                    }),
                    items,
                    footer: json!({
                        "total_amount": f64n(doc["total_amount"].as_str().unwrap_or("0")),
                        "total_quantity": total_quantity,
                        "total_items": total_items,
                    }),
                })
            }
            "purchase_order" => {
                let doc = self.read_purchase_order_json(id).await?;
                let mut items = Vec::new();
                let mut total_quantity = 0.0f64;
                for it in doc["items"].as_array().unwrap_or(&vec![]).iter() {
                    let qty = f64n(it["quantity"].as_str().unwrap_or("0"));
                    total_quantity += qty;
                    items.push(json!({
                        "product_name": it["product"]["title"].as_str().unwrap_or("Невідомий товар"),
                        "barcode": it["product"]["barcode"].as_str().map(|s| s.to_string()),
                        "quantity": qty,
                        "price": f64n(it["price"].as_str().unwrap_or("0")),
                        "total": f64n(it["total"].as_str().unwrap_or("0")),
                    }));
                }
                let expected = if doc["expected_date"].as_str().unwrap_or("").is_empty() {
                    "не вказано".to_string()
                } else {
                    fmt_dmy(doc["expected_date"].as_str().unwrap_or(""))
                };
                let total_items = items.len();
                Ok(DocPrintDto {
                    header: json!({
                        "document_type": "Замовлення постачальнику",
                        "document_number": doc["number"].as_str().unwrap_or(""),
                        "date": fmt_dmy(doc["order_date"].as_str().unwrap_or("")),
                        "supplier": doc["supplier_name"].as_str().unwrap_or("—"),
                        "expected_date": expected,
                        "status": doc["status"].as_str().unwrap_or(""),
                    }),
                    items,
                    footer: json!({
                        "total_amount": f64n(doc["total_amount"].as_str().unwrap_or("0")),
                        "total_quantity": total_quantity,
                        "total_items": total_items,
                    }),
                })
            }
            other => Err(DocumentsError::BadRequest(format!(
                "Невідомий тип документа: '{other}'. Доступні: invoice, transfer, write_off, return_invoice, purchase_order"
            ))),
        }
    }
}

// ── Приватні confirm-реалізації (1:1 DocumentService) ────────────────────────

impl SqlxDocuments {
    /// Python DocumentService.confirm_invoice.
    async fn confirm_invoice(&self, id: Uuid) -> Result<(), DocumentsError> {
        let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
        let row = sqlx::query(
            r#"SELECT i.id, i.status::text, i.is_fiscal, i.supplier_id, i.total_amount::text,
                      i.number, i.invoice_date, i.created_at, i.payment_method::text, i.store_id
               FROM invoices i WHERE i.id = $1 FOR UPDATE"#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = row
            .ok_or_else(|| DocumentsError::NotFound(format!("Накладну з ID '{id}' не знайдено")))?;
        let status: String = row.get("status");
        if status != "draft" {
            return Err(DocumentsError::BadRequest(format!(
                "Накладна вже має статус '{status}'"
            )));
        }
        let is_fiscal: bool = row.get("is_fiscal");
        let store_id: Option<Uuid> = row.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            DocumentsError::BadRequest(format!("Накладну з ID '{id}' не прив'язано до точки"))
        })?;
        let items = sqlx::query(
            r#"SELECT product_id, quantity::text, cost_price::text, price::text,
                      previous_price::text FROM invoice_items WHERE invoice_id = $1"#,
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in &items {
            let pid: Uuid = it.get("product_id");
            let qty = it.get::<String, _>("quantity");
            // update_stock: stock += quantity (надходження → stock table, per store)
            let price: String = it.get("price");
            sqlx::query(
                "INSERT INTO stock (store_id, product_id, quantity, price, updated_at)
                 VALUES ($1, $2, $3::numeric, $4::numeric, now())
                 ON CONFLICT (store_id, product_id) DO UPDATE
                    SET quantity = stock.quantity + EXCLUDED.quantity,
                        price = EXCLUDED.price, updated_at = now()",
            )
            .bind(store_id)
            .bind(pid)
            .bind(&qty)
            .bind(&price)
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
            // ФІКС 2026-08-21: products.stock (сумарний, Python-еталон) += qty.
            sqlx::query(
                "UPDATE products SET stock = COALESCE(stock, 0) + $1::numeric, updated_at = now()
                 WHERE id = $2",
            )
            .bind(&qty)
            .bind(pid)
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
            if is_fiscal {
                // _increase_fiscal_stock: is_fiscal=true, fiscal_stock += qty
                sqlx::query(
                    r#"UPDATE products SET is_fiscal = true,
                       fiscal_stock = COALESCE(fiscal_stock,0) + $1::numeric WHERE id = $2"#,
                )
                .bind(&qty)
                .bind(pid)
                .execute(&mut *tx)
                .await
                .map_err(|e| de(e.to_string()))?;
            }
            let cost_price: Option<String> = it.get("cost_price");
            if let Some(cp) = cost_price {
                if f64n(&cp) > 0.0 {
                    // Оновлюємо собівартість і ціну товару; previous_price зберігається
                    let price: String = it.get("price");
                    let prev: Option<String> = it.get("previous_price");
                    sqlx::query("UPDATE products SET cost_price = $1::numeric WHERE id = $2")
                        .bind(&cp)
                        .bind(pid)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| de(e.to_string()))?;
                    // Python: if item.previous_price is None: item.previous_price = product.price;
                    //         product.price = item.price
                    // Тобто зберігаємо поточну ціну товару як previous_price позиції,
                    // якщо ще не збережена, і ставимо ціну товару = ціна з накладної.
                    if prev.is_none() {
                        let cur_price: Option<String> =
                            sqlx::query_scalar("SELECT price::text FROM products WHERE id = $1")
                                .bind(pid)
                                .fetch_one(&mut *tx)
                                .await
                                .map_err(|e| de(e.to_string()))?;
                        sqlx::query(
                            "UPDATE invoice_items SET previous_price = $1::numeric WHERE invoice_id = $2 AND product_id = $3 AND previous_price IS NULL",
                        )
                        .bind(cur_price.unwrap_or_default())
                        .bind(id)
                        .bind(pid)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| de(e.to_string()))?;
                    }
                    sqlx::query("UPDATE products SET price = $1::numeric WHERE id = $2")
                        .bind(&price)
                        .bind(pid)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| de(e.to_string()))?;
                }
            }
        }
        // Статус → confirmed
        sqlx::query("UPDATE invoices SET status = 'confirmed', updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        // Ledger INVOICE (Python ledger_service.create_ledger_entry v1)
        let supplier_id: Uuid = row.get("supplier_id");
        let total: String = row
            .get::<Option<String>, _>("total_amount")
            .unwrap_or_else(|| "0".to_string());
        let number: String = row.get("number");
        let invoice_date: Option<NaiveDateTime> = row.get("invoice_date");
        let created_at: NaiveDateTime = row.get("created_at");
        let payment_method: Option<String> = row.get("payment_method");
        let mut notes = format!("Прибуткова накладна №{number}");
        if let Some(pm) = payment_method {
            let label = match pm.as_str() {
                "credit" => "в борг",
                "bank_transfer" => "по перерахунку",
                "cash" => "готівкою з каси",
                "other" => "інший спосіб",
                _ => "",
            };
            notes += &format!(" ({label})");
        }
        self.ledger_entry(
            &mut tx,
            supplier_id,
            "invoice",
            &total,
            invoice_date.unwrap_or(created_at),
            id,
            &number,
            &notes,
        )
        .await?;
        tx.commit().await.map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Python DocumentService.confirm_return_invoice (batch-confirm без exchange_items).
    async fn confirm_return_invoice(&self, id: Uuid) -> Result<(), DocumentsError> {
        let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
        let row = sqlx::query(
            r#"SELECT r.id, r.status::text, r.is_fiscal, r.supplier_id, r.total_amount::text,
                      r.number, r.return_date, r.return_action::text, r.source_invoice_id, r.store_id
               FROM return_invoices r WHERE r.id = $1 FOR UPDATE"#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = row.ok_or_else(|| {
            DocumentsError::NotFound(format!("Повернення з ID '{id}' не знайдено"))
        })?;
        let status: String = row.get("status");
        if status != "draft" {
            return Err(DocumentsError::BadRequest(format!(
                "Повернення вже має статус '{status}'"
            )));
        }
        let is_fiscal: bool = row.get("is_fiscal");
        let store_id: Option<Uuid> = row.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            DocumentsError::BadRequest(format!("Повернення з ID '{id}' не прив'язано до точки"))
        })?;
        let items = sqlx::query(
            r#"SELECT product_id, quantity::text FROM return_invoice_items WHERE return_invoice_id = $1"#,
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in &items {
            let pid: Uuid = it.get("product_id");
            let qty = it.get::<String, _>("quantity");
            sqlx::query(
                "UPDATE stock SET quantity = GREATEST(0, quantity - $1::numeric), updated_at = now()
                 WHERE store_id = $2 AND product_id = $3",
            )
            .bind(&qty)
            .bind(store_id)
            .bind(pid)
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
            // ФІКС 2026-08-21: products.stock (сумарний, Python-еталон) -= qty.
            sqlx::query(
                "UPDATE products SET stock = GREATEST(0, COALESCE(stock, 0) - $1::numeric), updated_at = now()
                 WHERE id = $2",
            )
            .bind(&qty)
            .bind(pid)
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
            if is_fiscal {
                sqlx::query(
                    r#"UPDATE products SET fiscal_stock = GREATEST(0, COALESCE(fiscal_stock,0) - $1::numeric)
                       WHERE id = $2"#,
                )
                .bind(&qty)
                .bind(pid)
                .execute(&mut *tx)
                .await
                .map_err(|e| de(e.to_string()))?;
            }
        }
        let action: String = row.get("return_action");
        let action_label = match action.as_str() {
            "deduct_from_debt" => "списано з боргу постачальника",
            "add_to_cash" => "зачислено в касу",
            "exchange" => "обмін на інший товар",
            other => other,
        };
        let number: String = row.get("number");
        let mut notes = format!("Повернення постачальнику №{number} ({action_label})");
        let supplier_id: Uuid = row.get("supplier_id");
        let total: String = row
            .get::<Option<String>, _>("total_amount")
            .unwrap_or_else(|| "0".to_string());
        let return_date: NaiveDateTime = row.get("return_date");
        let source_invoice_id: Option<Uuid> = row.get("source_invoice_id");
        // document_id / document_number: source_invoice_id → накладна
        let (doc_id, doc_number) = match source_invoice_id {
            Some(sid) => {
                let src_number: Option<String> =
                    sqlx::query_scalar("SELECT number FROM invoices WHERE id = $1")
                        .bind(sid)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(|e| de(e.to_string()))?;
                let dn = src_number.unwrap_or_else(|| number.clone());
                notes += &format!(" (прив'язано до накладної №{dn})");
                (sid, dn)
            }
            None => (id, number.clone()),
        };
        if f64n(&total) > 0.0 {
            match action.as_str() {
                "deduct_from_debt" => {
                    // amount = -total (зменшення боргу)
                    let neg = format!("-{}", total.trim_start_matches('-'));
                    self.ledger_entry(
                        &mut tx,
                        supplier_id,
                        "return",
                        &neg,
                        return_date,
                        doc_id,
                        &doc_number,
                        &notes,
                    )
                    .await?;
                }
                "add_to_cash" => {
                    self.ledger_entry(
                        &mut tx,
                        supplier_id,
                        "return",
                        "0.00",
                        return_date,
                        doc_id,
                        &doc_number,
                        &format!("{notes} (сума зачислена в касу)"),
                    )
                    .await?;
                }
                "exchange" => {
                    // batch-confirm викликає confirm_return_invoice БЕЗ exchange_items → 400
                    return Err(DocumentsError::BadRequest(
                        "Для обміну (exchange) необхідно вказати exchange_items — список товарів, на які відбувається обмін".to_string(),
                    ));
                }
                _ => {}
            }
        }
        sqlx::query(
            "UPDATE return_invoices SET status = 'confirmed', updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        tx.commit().await.map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Python documents.py batch-confirm для purchase_order: створює invoice.
    async fn confirm_purchase_order(&self, id: Uuid, user_id: Uuid) -> Result<(), DocumentsError> {
        let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
        let row = sqlx::query(
            r#"SELECT p.id, p.number, p.supplier_id, p.order_date, p.is_fiscal, p.notes,
                      p.total_amount::text, p.status::text
               FROM purchase_orders p WHERE p.id = $1 FOR UPDATE"#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row =
            row.ok_or_else(|| DocumentsError::NotFound(format!("Замовлення '{id}' не знайдено")))?;
        let status: String = row.get("status");
        if status != "draft" {
            return Err(DocumentsError::BadRequest(format!(
                "Замовлення '{number}' вже має статус '{status}'",
                number = row.get::<String, _>("number")
            )));
        }
        let invoice_number = self.next_doc_number_tx(&mut tx, "invoices", "ПН").await?;
        let new_invoice_id = Uuid::new_v4();
        let supplier_id: Uuid = row.get("supplier_id");
        let order_date: NaiveDateTime = row.get("order_date");
        let is_fiscal: bool = row.get("is_fiscal");
        let _notes: Option<String> = row.get("notes");
        let total: String = row
            .get::<Option<String>, _>("total_amount")
            .unwrap_or_else(|| "0".to_string());
        let order_number: String = row.get("number");
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            r#"INSERT INTO invoices (id, number, supplier_id, invoice_date, payment_method,
               is_fiscal, notes, total_amount, status, created_by_id, created_at, updated_at)
               VALUES ($1,$2,$3,$4,'credit',$5,$6,$7::numeric,'draft',$8,$9,$9)"#,
        )
        .bind(new_invoice_id)
        .bind(&invoice_number)
        .bind(supplier_id)
        .bind(order_date)
        .bind(is_fiscal)
        .bind(format!(
            "Автоматично створено із замовлення №{order_number}",
        ))
        .bind(&total)
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        let order_items = sqlx::query(
            r#"SELECT product_id, quantity::text, price::text, total::text
               FROM purchase_order_items WHERE purchase_order_id = $1"#,
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in &order_items {
            sqlx::query(
                r#"INSERT INTO invoice_items (id, invoice_id, product_id, quantity, price, total, created_at)
                   VALUES ($1,$2,$3,$4::numeric,$5::numeric,$6::numeric,$7)"#,
            )
            .bind(Uuid::new_v4())
            .bind(new_invoice_id)
            .bind(it.get::<Uuid, _>("product_id"))
            .bind(it.get::<String, _>("quantity"))
            .bind(it.get::<String, _>("price"))
            .bind(it.get::<String, _>("total"))
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        sqlx::query(
            "UPDATE purchase_orders SET invoice_id = $1, status = 'confirmed', updated_at = now() WHERE id = $2",
        )
        .bind(new_invoice_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        tx.commit().await.map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Rust pos.rs confirm_transfer (вже реалізовано в етапі 3).
    async fn confirm_transfer(&self, id: Uuid) -> Result<(), DocumentsError> {
        let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
        let row = sqlx::query("SELECT store_id FROM transfers WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(row) = row else {
            return Err(DocumentsError::NotFound(format!(
                "Переміщення з ID '{id}' не знайдено"
            )));
        };
        let store_id: Option<Uuid> = row.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            DocumentsError::BadRequest(format!("Переміщення з ID '{id}' не прив'язано до точки"))
        })?;
        let st: String = sqlx::query_scalar("SELECT status::text FROM transfers WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        if st != "draft" {
            return Err(DocumentsError::BadRequest(format!(
                "Переміщення вже має статус '{st}'"
            )));
        }
        let items = sqlx::query(
            "SELECT product_id, quantity::text FROM transfer_items WHERE transfer_id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in &items {
            let qty = it.get::<String, _>("quantity");
            sqlx::query(
                "UPDATE stock SET quantity = GREATEST(0, quantity - $1::numeric), updated_at = now()
                 WHERE store_id = $2 AND product_id = $3",
            )
            .bind(&qty)
            .bind(store_id)
            .bind(it.get::<Uuid, _>("product_id"))
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        sqlx::query("UPDATE transfers SET status = 'confirmed', updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        tx.commit().await.map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Rust pos.rs confirm_write_off (вже реалізовано в етапі 3).
    async fn confirm_write_off(&self, id: Uuid) -> Result<(), DocumentsError> {
        let mut tx = self.pool.begin().await.map_err(|e| de(e.to_string()))?;
        let row = sqlx::query("SELECT store_id FROM write_offs WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        let Some(row) = row else {
            return Err(DocumentsError::NotFound(format!(
                "Списання з ID '{id}' не знайдено"
            )));
        };
        let store_id: Option<Uuid> = row.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            DocumentsError::BadRequest(format!("Списання з ID '{id}' не прив'язано до точки"))
        })?;
        // Python confirm_write_off: БЕЗ перевірки статусу, БЕЗ зміни статусу —
        // тільки зменшує залишки.
        let items = sqlx::query(
            "SELECT product_id, quantity::text FROM write_off_items WHERE write_off_id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        for it in &items {
            let qty = it.get::<String, _>("quantity");
            sqlx::query(
                "UPDATE stock SET quantity = GREATEST(0, quantity - $1::numeric), updated_at = now()
                 WHERE store_id = $2 AND product_id = $3",
            )
            .bind(&qty)
            .bind(store_id)
            .bind(it.get::<Uuid, _>("product_id"))
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
            // ФІКС 2026-08-21: products.stock (сумарний, Python-еталон) -= qty.
            sqlx::query(
                "UPDATE products SET stock = GREATEST(0, COALESCE(stock, 0) - $1::numeric), updated_at = now()
                 WHERE id = $2",
            )
            .bind(&qty)
            .bind(it.get::<Uuid, _>("product_id"))
            .execute(&mut *tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        tx.commit().await.map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// Python ledger_service.create_ledger_entry (v1): balance = SUM(amount) + amount.
    #[allow(clippy::too_many_arguments)]
    async fn ledger_entry(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        supplier_id: Uuid,
        operation_type: &str,
        amount: &str,
        operation_date: NaiveDateTime,
        document_id: Uuid,
        document_number: &str,
        notes: &str,
    ) -> Result<(), DocumentsError> {
        // 404 — постачальника не знайдено
        let sup: Option<Uuid> = sqlx::query_scalar("SELECT id FROM suppliers WHERE id = $1")
            .bind(supplier_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        if sup.is_none() {
            return Err(DocumentsError::NotFound(format!(
                "Постачальника з ID '{supplier_id}' не знайдено"
            )));
        }
        // Python: LedgerOperationType(operation_type) → ValueError → 400
        if !["invoice", "payment", "return", "correction"].contains(&operation_type) {
            return Err(DocumentsError::BadRequest(format!(
                "Невідомий тип операції: '{operation_type}'"
            )));
        }
        let current: f64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(amount),0)::float8 FROM supplier_ledger WHERE supplier_id = $1",
        )
        .bind(supplier_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        let amt = f64n(amount);
        let balance_after = current + amt;
        sqlx::query(
            r#"INSERT INTO supplier_ledger (supplier_id, operation_type, document_id, document_number,
               amount, balance_after, operation_date, notes, created_at)
               VALUES ($1,$2::ledger_operation_type,$3,$4,$5::numeric,$6::numeric,$7,$8,now())"#,
        )
        .bind(supplier_id)
        .bind(operation_type)
        .bind(document_id)
        .bind(document_number)
        .bind(format!("{amt:.2}"))
        .bind(format!("{balance_after:.2}"))
        .bind(operation_date)
        .bind(notes)
        .execute(&mut **tx)
        .await
        .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    /// next_doc_number у транзакції.
    async fn next_doc_number_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        table: &str,
        prefix: &str,
    ) -> Result<String, DocumentsError> {
        let today = chrono::Utc::now().naive_utc().format("%Y%m%d").to_string();
        let pfx = format!("{prefix}-{today}-");
        let q = format!("SELECT max(number) FROM {table} WHERE number LIKE $1");
        let row: (Option<String>,) = sqlx::query_as(&q)
            .bind(format!("{pfx}%"))
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| de(e.to_string()))?;
        let last_seq = row
            .0
            .and_then(|n| {
                n.chars()
                    .rev()
                    .take(3)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
                    .parse::<i64>()
                    .ok()
            })
            .unwrap_or(0);
        Ok(format!("{pfx}{:03}", last_seq + 1))
    }

    // ── EXPORT: flat ─────────────────────────────────────────────────────────
    async fn export_flat(&self, q: &ExportQuery) -> Result<ExportData, DocumentsError> {
        let use_ids = !q.ids.is_empty();
        let mut all_docs: Vec<Vec<String>> = Vec::new();

        if q.document_type.is_none() || q.document_type.as_deref() == Some("invoice") {
            let mut sql = String::from(
                r#"SELECT i.id, i.number, i.status::text, i.total_amount::text, i.created_at,
                          s.name AS supplier_name
                   FROM invoices i LEFT JOIN suppliers s ON s.id = i.supplier_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND i.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND i.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND i.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(a) = q.amount_from {
                    sql.push_str(&format!(" AND i.total_amount >= {a}"));
                }
                if let Some(a) = q.amount_to {
                    sql.push_str(&format!(" AND i.total_amount <= {a}"));
                }
                if let Some(sid) = q.supplier_id {
                    sql.push_str(&format!(" AND i.supplier_id = '{sid}'"));
                }
            }
            sql.push_str(" ORDER BY i.created_at DESC");
            let rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in rows {
                let status: String = r.get("status");
                if !use_ids {
                    if let Some(s) = &q.status {
                        if &status != s {
                            continue;
                        }
                    }
                    let number: String = r.get("number");
                    if !search_match(&q.search, &Some(number.clone())) {
                        continue;
                    }
                }
                let created: NaiveDateTime = r.get("created_at");
                all_docs.push(vec![
                    "Прибуткова накладна".into(),
                    r.get::<String, _>("number"),
                    status,
                    day_time_str(created),
                    r.get::<Option<String>, _>("supplier_name")
                        .unwrap_or_default(),
                    pystr(f64n(
                        &r.get::<Option<String>, _>("total_amount")
                            .unwrap_or_default(),
                    )),
                ]);
            }
        }

        if q.document_type.is_none() || q.document_type.as_deref() == Some("transfer") {
            let mut sql = String::from(
                r#"SELECT t.id, t.number, t.status::text, t.created_at, t.from_location, t.to_location
                   FROM transfers t WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND t.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND t.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND t.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
            }
            sql.push_str(" ORDER BY t.created_at DESC");
            let rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in rows {
                let status: String = r.get("status");
                if !use_ids {
                    if let Some(s) = &q.status {
                        if &status != s {
                            continue;
                        }
                    }
                    let number: String = r.get("number");
                    if !search_match(&q.search, &Some(number.clone())) {
                        continue;
                    }
                }
                let created: NaiveDateTime = r.get("created_at");
                all_docs.push(vec![
                    "Переміщення".into(),
                    r.get::<String, _>("number"),
                    status,
                    day_time_str(created),
                    format!(
                        "{} → {}",
                        r.get::<String, _>("from_location"),
                        r.get::<String, _>("to_location")
                    ),
                    "0".into(),
                ]);
            }
        }

        if q.document_type.is_none() || q.document_type.as_deref() == Some("write_off") {
            let mut sql = String::from(
                r#"SELECT w.id, w.number, w.total_amount::text, w.status::text, w.created_at FROM write_offs w WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND w.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND w.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND w.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(a) = q.amount_from {
                    sql.push_str(&format!(" AND w.total_amount >= {a}"));
                }
                if let Some(a) = q.amount_to {
                    sql.push_str(&format!(" AND w.total_amount <= {a}"));
                }
            }
            sql.push_str(" ORDER BY w.created_at DESC");
            let rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in rows {
                let wo_status: String = r.get("status");
                if !use_ids {
                    if let Some(s) = &q.status {
                        if &wo_status != s {
                            continue;
                        }
                    }
                    let number: String = r.get("number");
                    if !search_match(&q.search, &Some(number.clone())) {
                        continue;
                    }
                }
                let created: NaiveDateTime = r.get("created_at");
                all_docs.push(vec![
                    "Списання".into(),
                    r.get::<String, _>("number"),
                    wo_status,
                    day_time_str(created),
                    String::new(),
                    pystr(f64n(
                        &r.get::<Option<String>, _>("total_amount")
                            .unwrap_or_default(),
                    )),
                ]);
            }
        }

        if q.document_type.is_none() || q.document_type.as_deref() == Some("return_invoice") {
            let mut sql = String::from(
                r#"SELECT r.id, r.number, r.status::text, r.total_amount::text, r.created_at,
                          s.name AS supplier_name
                   FROM return_invoices r LEFT JOIN suppliers s ON s.id = r.supplier_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND r.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND r.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND r.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(a) = q.amount_from {
                    sql.push_str(&format!(" AND r.total_amount >= {a}"));
                }
                if let Some(a) = q.amount_to {
                    sql.push_str(&format!(" AND r.total_amount <= {a}"));
                }
                if let Some(sid) = q.supplier_id {
                    sql.push_str(&format!(" AND r.supplier_id = '{sid}'"));
                }
            }
            sql.push_str(" ORDER BY r.created_at DESC");
            let rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in rows {
                let status: String = r.get("status");
                if !use_ids {
                    if let Some(s) = &q.status {
                        if &status != s {
                            continue;
                        }
                    }
                    let number: String = r.get("number");
                    if !search_match(&q.search, &Some(number.clone())) {
                        continue;
                    }
                }
                let created: NaiveDateTime = r.get("created_at");
                all_docs.push(vec![
                    "Повернення постачальнику".into(),
                    r.get::<String, _>("number"),
                    status,
                    day_time_str(created),
                    r.get::<Option<String>, _>("supplier_name")
                        .unwrap_or_default(),
                    pystr(f64n(
                        &r.get::<Option<String>, _>("total_amount")
                            .unwrap_or_default(),
                    )),
                ]);
            }
        }

        if q.document_type.is_none() || q.document_type.as_deref() == Some("purchase_order") {
            let mut sql = String::from(
                r#"SELECT p.id, p.number, p.status::text, p.total_amount::text, p.created_at,
                          s.name AS supplier_name
                   FROM purchase_orders p LEFT JOIN suppliers s ON s.id = p.supplier_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND p.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND p.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND p.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(a) = q.amount_from {
                    sql.push_str(&format!(" AND p.total_amount >= {a}"));
                }
                if let Some(a) = q.amount_to {
                    sql.push_str(&format!(" AND p.total_amount <= {a}"));
                }
                if let Some(sid) = q.supplier_id {
                    sql.push_str(&format!(" AND p.supplier_id = '{sid}'"));
                }
            }
            sql.push_str(" ORDER BY p.created_at DESC");
            let rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in rows {
                let status: String = r.get("status");
                if !use_ids {
                    if let Some(s) = &q.status {
                        if &status != s {
                            continue;
                        }
                    }
                    let number: String = r.get("number");
                    if !search_match(&q.search, &Some(number.clone())) {
                        continue;
                    }
                }
                let created: NaiveDateTime = r.get("created_at");
                all_docs.push(vec![
                    "Замовлення постачальнику".into(),
                    r.get::<String, _>("number"),
                    status,
                    day_time_str(created),
                    r.get::<Option<String>, _>("supplier_name")
                        .unwrap_or_default(),
                    pystr(f64n(
                        &r.get::<Option<String>, _>("total_amount")
                            .unwrap_or_default(),
                    )),
                ]);
            }
        }

        // Python: `all_docs.sort(key=lambda d: d["Дата"], reverse=True)`
        all_docs.sort_by(|a, b| b[3].cmp(&a[3]));
        Ok(ExportData {
            headers: vec![
                "Тип".into(),
                "Номер".into(),
                "Статус".into(),
                "Дата".into(),
                "Постачальник".into(),
                "Сума".into(),
            ],
            rows: all_docs,
        })
    }

    // ── EXPORT: detailed ─────────────────────────────────────────────────────
    async fn export_detailed(&self, q: &ExportQuery) -> Result<ExportData, DocumentsError> {
        let use_ids = !q.ids.is_empty();
        let mut rows: Vec<Vec<String>> = Vec::new();

        {
            let mut sql = String::from(
                r#"SELECT i.id, i.number, i.status::text, i.created_at, i.payment_method::text,
                          s.name AS supplier_name,
                          ii.quantity::text AS qty, ii.price::text AS price, ii.total::text AS total,
                          p.title, p.barcode
                   FROM invoices i
                   LEFT JOIN suppliers s ON s.id = i.supplier_id
                   JOIN invoice_items ii ON ii.invoice_id = i.id
                   LEFT JOIN products p ON p.id = ii.product_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND i.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND i.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND i.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
            }
            sql.push_str(" ORDER BY i.created_at DESC, ii.created_at ASC");
            let db_rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in db_rows {
                let created: NaiveDateTime = r.get("created_at");
                rows.push(vec![
                    "Прибуткова накладна".into(),
                    r.get::<String, _>("number"),
                    day_str(created),
                    r.get::<Option<String>, _>("supplier_name")
                        .unwrap_or_default(),
                    r.get::<String, _>("status"),
                    r.get::<Option<String>, _>("payment_method")
                        .unwrap_or_else(|| "не вказано".into()),
                    r.get::<Option<String>, _>("title")
                        .unwrap_or_else(|| "Невідомий товар".into()),
                    r.get::<Option<String>, _>("barcode").unwrap_or_default(),
                    pystr(f64n(&r.get::<String, _>("qty"))),
                    pystr(f64n(&r.get::<String, _>("price"))),
                    pystr(f64n(&r.get::<String, _>("total"))),
                ]);
            }
        }

        {
            let mut sql = String::from(
                r#"SELECT t.id, t.number, t.status::text, t.created_at, t.from_location, t.to_location,
                          ti.quantity::text AS qty, p.title, p.barcode
                   FROM transfers t
                   JOIN transfer_items ti ON ti.transfer_id = t.id
                   LEFT JOIN products p ON p.id = ti.product_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND t.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND t.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND t.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
            }
            sql.push_str(" ORDER BY t.created_at DESC, ti.created_at ASC");
            let db_rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in db_rows {
                let created: NaiveDateTime = r.get("created_at");
                rows.push(vec![
                    "Переміщення".into(),
                    r.get::<String, _>("number"),
                    day_str(created),
                    format!(
                        "{} → {}",
                        r.get::<String, _>("from_location"),
                        r.get::<String, _>("to_location")
                    ),
                    r.get::<String, _>("status"),
                    String::new(),
                    r.get::<Option<String>, _>("title")
                        .unwrap_or_else(|| "Невідомий товар".into()),
                    r.get::<Option<String>, _>("barcode").unwrap_or_default(),
                    pystr(f64n(&r.get::<String, _>("qty"))),
                    "0".into(),
                    "0".into(),
                ]);
            }
        }

        {
            let mut sql = String::from(
                r#"SELECT w.id, w.number, w.status::text, w.created_at, w.reason::text,
                          woi.quantity::text AS qty, p.title, p.barcode, p.price::text AS price
                   FROM write_offs w
                   JOIN write_off_items woi ON woi.write_off_id = w.id
                   LEFT JOIN products p ON p.id = woi.product_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND w.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND w.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND w.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
            }
            sql.push_str(" ORDER BY w.created_at DESC, woi.created_at ASC");
            let db_rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in db_rows {
                let created: NaiveDateTime = r.get("created_at");
                let price = f64n(&r.get::<Option<String>, _>("price").unwrap_or_default());
                let qty = f64n(&r.get::<String, _>("qty"));
                let wo_status: String = r.get("status");
                rows.push(vec![
                    "Списання".into(),
                    r.get::<String, _>("number"),
                    day_str(created),
                    r.get::<String, _>("reason"),
                    wo_status,
                    String::new(),
                    r.get::<Option<String>, _>("title")
                        .unwrap_or_else(|| "Невідомий товар".into()),
                    r.get::<Option<String>, _>("barcode").unwrap_or_default(),
                    pystr(qty),
                    pystr(price),
                    pystr(price * qty),
                ]);
            }
        }

        {
            let mut sql = String::from(
                r#"SELECT r.id, r.number, r.status::text, r.created_at, s.name AS supplier_name,
                          ri.quantity::text AS qty, ri.price::text AS price, ri.total::text AS total,
                          p.title, p.barcode
                   FROM return_invoices r
                   LEFT JOIN suppliers s ON s.id = r.supplier_id
                   JOIN return_invoice_items ri ON ri.return_invoice_id = r.id
                   LEFT JOIN products p ON p.id = ri.product_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND r.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND r.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND r.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
            }
            sql.push_str(" ORDER BY r.created_at DESC, ri.created_at ASC");
            let db_rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in db_rows {
                let created: NaiveDateTime = r.get("created_at");
                rows.push(vec![
                    "Повернення постачальнику".into(),
                    r.get::<String, _>("number"),
                    day_str(created),
                    r.get::<Option<String>, _>("supplier_name")
                        .unwrap_or_default(),
                    r.get::<String, _>("status"),
                    String::new(),
                    r.get::<Option<String>, _>("title")
                        .unwrap_or_else(|| "Невідомий товар".into()),
                    r.get::<Option<String>, _>("barcode").unwrap_or_default(),
                    pystr(f64n(&r.get::<String, _>("qty"))),
                    pystr(f64n(&r.get::<String, _>("price"))),
                    pystr(f64n(&r.get::<String, _>("total"))),
                ]);
            }
        }

        {
            let mut sql = String::from(
                r#"SELECT p.id, p.number, p.status::text, p.created_at, s.name AS supplier_name,
                          po.quantity::text AS qty, po.price::text AS price, po.total::text AS total,
                          pr.title, pr.barcode
                   FROM purchase_orders p
                   LEFT JOIN suppliers s ON s.id = p.supplier_id
                   JOIN purchase_order_items po ON po.purchase_order_id = p.id
                   LEFT JOIN products pr ON pr.id = po.product_id WHERE 1=1"#,
            );
            if use_ids {
                sql.push_str(" AND p.id = ANY($1)");
            } else {
                if let Some(d) = q.date_from {
                    sql.push_str(&format!(
                        " AND p.created_at >= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
                if let Some(d) = q.date_to {
                    sql.push_str(&format!(
                        " AND p.created_at <= '{}'",
                        d.format("%Y-%m-%d %H:%M:%S%.f")
                    ));
                }
            }
            sql.push_str(" ORDER BY p.created_at DESC, po.created_at ASC");
            let db_rows = fetch_docs_ids(&self.pool, &sql, &q.ids).await.map_err(de)?;
            for r in db_rows {
                let created: NaiveDateTime = r.get("created_at");
                rows.push(vec![
                    "Замовлення постачальнику".into(),
                    r.get::<String, _>("number"),
                    day_str(created),
                    r.get::<Option<String>, _>("supplier_name")
                        .unwrap_or_default(),
                    r.get::<String, _>("status"),
                    String::new(),
                    r.get::<Option<String>, _>("title")
                        .unwrap_or_else(|| "Невідомий товар".into()),
                    r.get::<Option<String>, _>("barcode").unwrap_or_default(),
                    pystr(f64n(&r.get::<String, _>("qty"))),
                    pystr(f64n(&r.get::<String, _>("price"))),
                    pystr(f64n(&r.get::<String, _>("total"))),
                ]);
            }
        }

        Ok(ExportData {
            headers: vec![
                "Тип документа".into(),
                "Номер документа".into(),
                "Дата".into(),
                "Постачальник".into(),
                "Статус".into(),
                "Спосіб оплати".into(),
                "Назва товару".into(),
                "Штрих-код".into(),
                "Кількість".into(),
                "Ціна".into(),
                "Сума".into(),
            ],
            rows,
        })
    }
}

/// Виконує SQL з `id = ANY($1)` або без параметра (ids порожні).
async fn fetch_docs_ids(
    pool: &StorePool,
    sql: &str,
    ids: &[Uuid],
) -> Result<Vec<sqlx::postgres::PgRow>, String> {
    if ids.is_empty() {
        sqlx::query(sql)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())
    } else {
        sqlx::query(sql)
            .bind(ids)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())
    }
}
