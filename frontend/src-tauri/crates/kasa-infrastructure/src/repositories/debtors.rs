//! SQL-репозиторій боржників (етап 8 — група 1).
//!
//! Реалізує [`DebtorService`] на sqlx/PostgreSQL — 1:1 з Python v1/debtors.py:
//!   - search: ilike по імені, сортування name, limit (1..50)
//!   - list: count + page (sort total_debt DESC), pages = max(1, ceil)
//!   - create: 201, дефолтні timestamps UTC
//!   - pay: транзакція; сума > боргу/<=0/немає боргу → 400; при 0 → DELETE
//!     (FK ondelete=CASCADE видаляє і payment — як SQLAlchemy cascade
//!     "all, delete-orphan" у Python)
//!   - receipts: v1 ReceiptResponse (items з product_name; product_barcode,
//!     profit, vat_amount, total_profit, cashier_name — завжди null/"" як Python)
//!   - payments: історія оплат (created_at DESC)
//!
//! Decimal-поля: `::text` → рядок зі scale колонки (Python Pydantic Decimal).

use chrono::{NaiveDateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use kasa_domain::{
    parse_scaled2, DebtorCreateInput, DebtorDto, DebtorError, DebtorListDto, DebtorPayInput,
    DebtorPaymentDto, DebtorReceiptDto, DebtorReceiptItemDto, DebtorSearchQuery, DebtorService,
    DebtorUpdateInput,
};

/// sqlx::Error → DebtorError::Infrastructure.
trait De<T> {
    fn de(self) -> Result<T, DebtorError>;
}
impl<T> De<T> for Result<T, sqlx::Error> {
    fn de(self) -> Result<T, DebtorError> {
        self.map_err(|e| DebtorError::Infrastructure(e.to_string()))
    }
}

/// SQL-реалізація боржників.
#[derive(Clone)]
pub struct SqlxDebtors {
    pool: PgPool,
}

impl SqlxDebtors {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn utc_now() -> NaiveDateTime {
    Utc::now().naive_utc()
}

fn row_to_debtor(row: &sqlx::postgres::PgRow) -> Result<DebtorDto, DebtorError> {
    Ok(DebtorDto {
        id: row.try_get("id").de()?,
        name: row.try_get("name").de()?,
        phone: row.try_get("phone").de()?,
        notes: row.try_get("notes").de()?,
        total_debt: row.try_get("total_debt").de()?,
        created_at: row.try_get("created_at").de()?,
        updated_at: row.try_get("updated_at").de()?,
    })
}

#[async_trait::async_trait]
impl DebtorService for SqlxDebtors {
    async fn search(&self, q: &DebtorSearchQuery) -> Result<Vec<DebtorDto>, DebtorError> {
        let pattern = format!("%{}%", q.query);
        let rows = sqlx::query(
            r#"SELECT id, name, phone, notes, total_debt::text AS total_debt,
                      created_at, updated_at
               FROM debtors
               WHERE name ILIKE $1
               ORDER BY name
               LIMIT $2"#,
        )
        .bind(&pattern)
        .bind(q.limit)
        .fetch_all(&self.pool)
        .await
        .de()?;
        rows.iter().map(row_to_debtor).collect()
    }

    async fn list(&self, page: i64, size: i64) -> Result<DebtorListDto, DebtorError> {
        let total: i64 = sqlx::query_scalar("SELECT count(id) FROM debtors")
            .fetch_one(&self.pool)
            .await
            .de()?;
        let offset = (page - 1) * size;
        let rows = sqlx::query(
            r#"SELECT id, name, phone, notes, total_debt::text AS total_debt,
                      created_at, updated_at
               FROM debtors
               ORDER BY total_debt DESC
               OFFSET $1 LIMIT $2"#,
        )
        .bind(offset)
        .bind(size)
        .fetch_all(&self.pool)
        .await
        .de()?;
        let items = rows.iter().map(row_to_debtor).collect::<Result<_, _>>()?;
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        Ok(DebtorListDto {
            items,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn create(&self, input: &DebtorCreateInput) -> Result<DebtorDto, DebtorError> {
        let now = utc_now();
        let row = sqlx::query(
            r#"INSERT INTO debtors (id, name, phone, notes, total_debt, created_at, updated_at)
               VALUES ($1, $2, $3, $4, 0, $5, $5)
               RETURNING id, name, phone, notes, total_debt::text AS total_debt,
                         created_at, updated_at"#,
        )
        .bind(Uuid::new_v4())
        .bind(&input.name)
        .bind(&input.phone)
        .bind(&input.notes)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .de()?;
        row_to_debtor(&row)
    }

    async fn get(&self, id: Uuid) -> Result<DebtorDto, DebtorError> {
        let row = sqlx::query(
            r#"SELECT id, name, phone, notes, total_debt::text AS total_debt,
                      created_at, updated_at
               FROM debtors WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .de()?;
        match row {
            Some(r) => row_to_debtor(&r),
            None => Err(DebtorError::NotFound(id)),
        }
    }

    async fn update(&self, id: Uuid, input: &DebtorUpdateInput) -> Result<DebtorDto, DebtorError> {
        // Python: окремі UPDATE-присвоєння (name/phone/notes якщо не None).
        let mut tx = self.pool.begin().await.de()?;
        let exists: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM debtors WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .de()?;
        if exists.is_none() {
            return Err(DebtorError::NotFound(id));
        }
        if let Some(name) = &input.name {
            sqlx::query("UPDATE debtors SET name = $1, updated_at = $2 WHERE id = $3")
                .bind(name)
                .bind(utc_now())
                .bind(id)
                .execute(&mut *tx)
                .await
                .de()?;
        }
        if let Some(phone) = &input.phone {
            sqlx::query("UPDATE debtors SET phone = $1, updated_at = $2 WHERE id = $3")
                .bind(phone)
                .bind(utc_now())
                .bind(id)
                .execute(&mut *tx)
                .await
                .de()?;
        }
        if let Some(notes) = &input.notes {
            sqlx::query("UPDATE debtors SET notes = $1, updated_at = $2 WHERE id = $3")
                .bind(notes)
                .bind(utc_now())
                .bind(id)
                .execute(&mut *tx)
                .await
                .de()?;
        }
        let row = sqlx::query(
            r#"SELECT id, name, phone, notes, total_debt::text AS total_debt,
                      created_at, updated_at
               FROM debtors WHERE id = $1"#,
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .de()?;
        let dto = row_to_debtor(&row)?;
        tx.commit().await.de()?;
        Ok(dto)
    }

    async fn pay(&self, id: Uuid, input: &DebtorPayInput) -> Result<DebtorDto, DebtorError> {
        // Валідація суми: Decimal scale <= 2 (Pydantic decimal_places=2).
        let amount_cents = match parse_scaled2(&input.amount) {
            Some(v) => v,
            None => {
                return Err(DebtorError::Validation(serde_json::json!({
                    "detail": [{
                        "type": "decimal_parsing",
                        "loc": ["body", "amount"],
                        "msg": "Input should be a valid decimal",
                        "input": input.amount,
                    }]
                })))
            }
        };
        if amount_cents <= 0 {
            return Err(DebtorError::BadRequest(
                "Сума оплати має бути більше 0".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await.de()?;
        // SELECT FOR UPDATE — блокуємо рядок боржника (конкурентність).
        let row = sqlx::query(
            r#"SELECT id, name, phone, notes, total_debt::text AS total_debt,
                      created_at, updated_at
               FROM debtors WHERE id = $1 FOR UPDATE"#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .de()?;
        let Some(row) = row else {
            return Err(DebtorError::NotFound(id));
        };
        let debtor = row_to_debtor(&row)?;
        let debt_cents = parse_scaled2(&debtor.total_debt).unwrap_or(0);
        if debt_cents <= 0 {
            return Err(DebtorError::BadRequest(
                "У боржника немає боргу".to_string(),
            ));
        }
        if amount_cents > debt_cents {
            return Err(DebtorError::BadRequest(format!(
                "Сума оплати ({}) перевищує поточний борг ({})",
                input.amount, debtor.total_debt
            )));
        }

        let new_debt = debt_cents - amount_cents;
        let now = utc_now();

        if new_debt <= 0 {
            // Повне погашення → Python видаляє боржника (SQLAlchemy cascade
            // "all, delete-orphan" видаляє і payment; БД: FK ondelete=CASCADE).
            sqlx::query(
                r#"UPDATE debtors SET total_debt = total_debt - $1::numeric,
                       updated_at = $2 WHERE id = $3"#,
            )
            .bind(&input.amount)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await
            .de()?;
            sqlx::query("DELETE FROM debtors WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .de()?;
            tx.commit().await.de()?;
            // Відповідь — стан ПЕРЕД видаленням (total_debt = "0.00").
            let mut dto = debtor;
            dto.total_debt = format_decimal2(new_debt);
            Ok(dto)
        } else {
            sqlx::query(
                r#"UPDATE debtors SET total_debt = total_debt - $1::numeric,
                       updated_at = $2 WHERE id = $3"#,
            )
            .bind(&input.amount)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await
            .de()?;
            sqlx::query(
                r#"INSERT INTO debtor_payments (id, debtor_id, amount, payment_method, created_at)
                   VALUES ($1, $2, $3::numeric, $4, $5)"#,
            )
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(&input.amount)
            .bind(&input.payment_method)
            .bind(now)
            .execute(&mut *tx)
            .await
            .de()?;
            let row = sqlx::query(
                r#"SELECT id, name, phone, notes, total_debt::text AS total_debt,
                          created_at, updated_at
                   FROM debtors WHERE id = $1"#,
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .de()?;
            let dto = row_to_debtor(&row)?;
            tx.commit().await.de()?;
            Ok(dto)
        }
    }

    async fn receipts(&self, id: Uuid) -> Result<Vec<DebtorReceiptDto>, DebtorError> {
        // Python: спершу 404 якщо боржника немає.
        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM debtors WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .de()?;
        if exists.is_none() {
            return Err(DebtorError::NotFound(id));
        }
        let rows = sqlx::query(
            r#"SELECT r.id, r.receipt_number, r.receipt_type::text AS receipt_type,
                      r.cashier_id, r.total_amount::text AS total_amount,
                      r.paid_amount::text AS paid_amount,
                      r.change_amount::text AS change_amount,
                      r.debtor_id, r.is_return, r.notes, r.created_at,
                      r.payment_method::text AS payment_method,
                      ri.id AS item_id, ri.product_id,
                      COALESCE(p.title, '') AS product_name,
                      ri.quantity::text AS quantity, ri.price::text AS price,
                      ri.total::text AS total, ri.purchase_price::text AS purchase_price,
                      ri.created_at AS item_created_at
               FROM receipts r
               LEFT JOIN receipt_items ri ON ri.receipt_id = r.id
               LEFT JOIN products p ON p.id = ri.product_id
               WHERE r.debtor_id = $1
               ORDER BY r.created_at DESC"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .de()?;

        let mut receipts: Vec<DebtorReceiptDto> = Vec::new();
        for row in &rows {
            let receipt_id: Uuid = row.try_get("id").de()?;
            let item_id: Option<Uuid> = row.try_get("item_id").de()?;
            // Новий чек (або перший рядок чеку).
            let need_push = receipts
                .last()
                .map(|r: &DebtorReceiptDto| r.id != receipt_id)
                .unwrap_or(true);
            if need_push {
                receipts.push(DebtorReceiptDto {
                    id: receipt_id,
                    receipt_number: row.try_get("receipt_number").de()?,
                    receipt_type: row.try_get("receipt_type").de()?,
                    cashier_id: row.try_get("cashier_id").de()?,
                    total_amount: row.try_get("total_amount").de()?,
                    paid_amount: row.try_get("paid_amount").de()?,
                    change_amount: row.try_get("change_amount").de()?,
                    debtor_id: row.try_get("debtor_id").de()?,
                    is_return: row.try_get("is_return").de()?,
                    notes: row.try_get("notes").de()?,
                    created_at: row.try_get("created_at").de()?,
                    items: Vec::new(),
                    total_profit: None,
                    vat_amount: None,
                    cashier_name: String::new(),
                    payment_method: row.try_get("payment_method").de()?,
                });
            }
            if let Some(item_id) = item_id {
                let last = receipts.last_mut().expect("щойно додали");
                last.items.push(DebtorReceiptItemDto {
                    id: item_id,
                    receipt_id,
                    product_id: row.try_get("product_id").de()?,
                    product_name: row.try_get("product_name").de()?,
                    product_barcode: None,
                    quantity: row.try_get("quantity").de()?,
                    price: row.try_get("price").de()?,
                    total: row.try_get("total").de()?,
                    purchase_price: row.try_get("purchase_price").de()?,
                    profit: None,
                    vat_amount: None,
                    created_at: row.try_get("item_created_at").de()?,
                });
            }
        }
        Ok(receipts)
    }

    async fn payments(&self, id: Uuid) -> Result<Vec<DebtorPaymentDto>, DebtorError> {
        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM debtors WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .de()?;
        if exists.is_none() {
            return Err(DebtorError::NotFound(id));
        }
        let rows = sqlx::query(
            r#"SELECT id, debtor_id, amount::text AS amount, payment_method, created_at
               FROM debtor_payments
               WHERE debtor_id = $1
               ORDER BY created_at DESC"#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .de()?;
        rows.iter()
            .map(|row| {
                Ok(DebtorPaymentDto {
                    id: row.try_get("id").de()?,
                    debtor_id: row.try_get("debtor_id").de()?,
                    amount: row.try_get("amount").de()?,
                    payment_method: row.try_get("payment_method").de()?,
                    created_at: row.try_get("created_at").de()?,
                })
            })
            .collect()
    }
}

/// Форматування scaled2 (копійки) у Decimal-рядок "0.00".
pub fn format_decimal2(cents: i64) -> String {
    let neg = cents < 0;
    let abs = cents.abs();
    format!(
        "{}{}.{:02}",
        if neg { "-" } else { "" },
        abs / 100,
        abs % 100
    )
}
