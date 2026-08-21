//! POS-репозиторії (етап 3 — чеки v2, робочі сесії, списання, переміщення, зміни).
//!
//! Реалізують [`PosService`] на sqlx/PostgreSQL — 1:1 з Python-еталоном:
//!   - чеки v2: ReceiptUseCases + v2/receipts.py (sale/return/list/detail/items/
//!     stats/search/by-product/returnable-quantity)
//!   - робочі сесії: v1/work_sessions.py (/my, /report, /user/{id})
//!   - списання: v1/write_offs.py (CRUD + confirm, авто-confirm при create)
//!   - переміщення: v1/transfers.py (CRUD + confirm/cancel, тільки чернетки)
//!   - зміни ПРРО: v2/prro.py (list з БД; open/close без ПРРО → 400 як Python)
//!
//! Транзакції: BEGIN у кожному write-методі; конкурентний продаж —
//! `SELECT ... FOR UPDATE` на рядку продукту (нуль втрат stock).
//! Timestamps: `(now() AT TIME ZONE 'UTC')::timestamp` — Python пише UTC.
//!
//! Scale Decimal: create-відповіді зберігають ВХІДНУ scale (identity map
//! Python), GET/confirm — scale колонки (`::text`).

use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::Row;
use crate::store_ctx::{current_store_ctx, StorePool};
use uuid::Uuid;

use torgashka_domain::{
    iso_utc_z, parse_scaled2, parse_scaled3, DocItemInput, MySessionsDto, PosError, PosService,
    ProductBriefInfoDto, ProductRecentSalesDto, PrroShiftDto, ReceiptCreateInput, ReceiptDto,
    ReceiptItemDetailDto, ReceiptItemDto, ReceiptItemInput, ReceiptListDto, ReceiptListQuery,
    ReceiptSearchDto, ReceiptSearchItemDto, ReceiptSearchQuery, ReceiptStatsDto,
    ReceiptV1CreateInput, ReceiptV1Dto, ReceiptV1ItemDto, ReceiptV1ItemInput, ReceiptV1ListDto,
    ReceiptV1ListQuery, ReceiptV1SearchDto, ReceiptV1SearchItemDto, RecentSaleDto,
    CashBalances, CashOperationCreateInput, CashOperationDto, CashOperationsListDto,
    CashOperationType, CashType,
    ReturnableQtyDto, ShiftListDto, TransferCreateInput, TransferDto, TransferItemDto,
    TransferListDto, TransferUpdateInput, UserHoursSummaryDto, UserSessionsDto, WorkReportDto,
    WorkSessionDto, WriteOffCreateInput, WriteOffDto, WriteOffItemDto, WriteOffListDto,
    WriteOffReasonItem, WriteOffReasonsListDto, WriteOffUpdateInput,
};

/// Локальний екстеншен: sqlx::Error → PosError.
trait SqlxResultExt<T> {
    fn pe(self) -> Result<T, PosError>;
}

impl<T> SqlxResultExt<T> for Result<T, sqlx::Error> {
    fn pe(self) -> Result<T, PosError> {
        self.map_err(|e| PosError::Infrastructure(e.to_string()))
    }
}

/// SQL-реалізація POS-операцій.
#[derive(Clone)]
pub struct SqlxPos {
    pool: StorePool,
}

impl SqlxPos {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }
}

// ─── Утиліти ────────────────────────────────────────────────────────────────

/// total чеку у scaled2 (для порівняння оплати mixed).
fn receipt_total_scaled2(items: &[ReceiptItemInput]) -> i64 {
    items
        .iter()
        .filter_map(|it| {
            let q = parse_scaled3(&it.quantity)?;
            let p = parse_scaled2(&it.price)?;
            Some(q as i128 * p as i128)
        })
        .sum::<i128>() as i64
        / 1000
}

/// Номер чеку: RCPT-{YYYYMMDD}-{6 hex uppercase}.
fn receipt_number(now: NaiveDateTime) -> String {
    let date = now.format("%Y%m%d").to_string();
    let hex: String = Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(6)
        .collect::<String>()
        .to_uppercase();
    format!("RCPT-{date}-{hex}")
}

/// Номер документа: ПРЕФІКС-{YYYYMMDD}-{XXX} (max за день + 1).
async fn next_doc_number(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    prefix: &str,
) -> Result<String, PosError> {
    let today = Utc::now().naive_utc().format("%Y%m%d").to_string();
    let pfx = format!("{prefix}-{today}-");
    let q = format!("SELECT max(number) FROM {table} WHERE number LIKE $1");
    let row: (Option<String>,) = sqlx::query_as(&q)
        .bind(format!("{pfx}%"))
        .fetch_one(&mut **tx)
        .await
        .pe()?;
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

/// Розв'язує ціни позиції документа (fallback на ціни продукту; 0 — якщо немає).
async fn resolve_item_prices(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item: &DocItemInput,
) -> Result<(String, String), PosError> {
    let cost = item
        .cost_price
        .as_deref()
        .and_then(|s| parse_scaled2(s).filter(|v| *v != 0));
    let price = item
        .price
        .as_deref()
        .and_then(|s| parse_scaled2(s).filter(|v| *v != 0));
    if cost.is_some() && price.is_some() {
        return Ok((
            item.cost_price.clone().unwrap_or_else(|| "0".to_string()),
            item.price.clone().unwrap_or_else(|| "0".to_string()),
        ));
    }
    // Per-store ціна: stock активної точки (COALESCE(NULLIF(st.price,0), p.price) —
    // 0 у stock = «без перевизначення» → глобальна products.price).
    let row = sqlx::query(
        "SELECT p.cost_price::text, COALESCE(NULLIF(st.price, 0), p.price)::text AS price \
         FROM products p \
         LEFT JOIN stock st ON st.product_id = p.id \
             AND st.store_id = NULLIF(current_setting('app.store_id', true), '')::uuid \
         WHERE p.id = $1",
    )
    .bind(item.product_id)
        .fetch_optional(&mut **tx)
        .await
        .pe()?;
    let (db_cost, db_price): (Option<String>, Option<String>) = match row {
        Some(r) => (r.try_get("cost_price").ok(), r.try_get("price").ok()),
        None => (None, None),
    };
    let cost_out = if cost.is_some() {
        item.cost_price.clone().unwrap_or_else(|| "0".to_string())
    } else {
        db_cost
            .filter(|s| parse_scaled2(s).map(|v| v != 0).unwrap_or(false))
            .unwrap_or_else(|| "0".to_string())
    };
    let price_out = if price.is_some() {
        item.price.clone().unwrap_or_else(|| "0".to_string())
    } else {
        db_price
            .filter(|s| parse_scaled2(s).map(|v| v != 0).unwrap_or(false))
            .unwrap_or_else(|| "0".to_string())
    };
    Ok((cost_out, price_out))
}

// ─── Чеки v2: створення ────────────────────────────────────────────────────

/// Валідація сум оплати (Python ReceiptUseCases._validate_payment).
fn validate_payment(input: &ReceiptCreateInput, total_scaled2: i64) -> Result<(), PosError> {
    let method = input.payment_method.to_lowercase();
    let total = total_scaled2;
    let is_debt = input.customer_id.is_some();
    let cash = input.cash_amount.as_deref().and_then(parse_scaled2);
    let card = input.card_amount.as_deref().and_then(parse_scaled2);

    if method == "mixed" {
        let (c, k) = match (cash, card) {
            (Some(c), Some(k)) => (c, k),
            _ => {
                return Err(PosError::BadRequest(
                    "Для змішаної оплати (mixed) обов'язково вкажіть cash_amount і card_amount"
                        .to_string(),
                ))
            }
        };
        let paid = c + k;
        if paid != total {
            return Err(PosError::BadRequest(format!(
                "Сума оплати (готівка {} + картка {} = {}) має дорівнювати сумі чеку ({})",
                dec2(c),
                dec2(k),
                dec2(paid),
                dec2(total)
            )));
        }
        return Ok(());
    }
    if method == "cash" {
        if let Some(k) = card {
            if k > 0 {
                return Err(PosError::BadRequest(
                    "Для оплати готівкою (cash) card_amount має бути 0 або не вказаний".to_string(),
                ));
            }
        }
        let paid = cash.unwrap_or(total);
        if !is_debt && paid < total {
            return Err(PosError::BadRequest(format!(
                "Сума оплати ({}) менша за суму чеку ({})",
                dec2(paid),
                dec2(total)
            )));
        }
        return Ok(());
    }
    if method == "card" {
        if let Some(c) = cash {
            if c > 0 {
                return Err(PosError::BadRequest(
                    "Для оплати карткою (card) cash_amount має бути 0 або не вказаний".to_string(),
                ));
            }
        }
        let paid = card.unwrap_or(total);
        if !is_debt && paid < total {
            return Err(PosError::BadRequest(format!(
                "Сума оплати ({}) менша за суму чеку ({})",
                dec2(paid),
                dec2(total)
            )));
        }
        return Ok(());
    }
    // bank_transfer / credit — без жорсткої перевірки
    Ok(())
}

/// Валідація даних терміналу (Python _validate_terminal).
fn validate_terminal(input: &ReceiptCreateInput, require_rrn: bool) -> Result<(), PosError> {
    let method = input.payment_method.to_lowercase();
    if !(method == "card" || method == "mixed") {
        return Ok(());
    }
    let status = input
        .terminal_status
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if status == "declined" {
        return Err(PosError::Validation(
            "Оплата карткою не підтверджена терміналом".to_string(),
        ));
    }
    if require_rrn
        && input
            .terminal_rrn
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        return Err(PosError::Validation(
            "Для повернення карткового чека необхідний RRN оригінальної транзакції".to_string(),
        ));
    }
    Ok(())
}

/// "14270" → "142.70"; "500" → "5.00"; "-500" → "-5.00"
fn dec2(v: i64) -> String {
    let (sign, v) = if v < 0 { ("-", -v) } else { ("", v) };
    format!("{sign}{}.{:02}", v / 100, v % 100)
}

/// SQL-вставка чеку (sale/return). Повертає (id, created_at::text).
#[allow(clippy::too_many_arguments)]
async fn insert_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    number: &str,
    receipt_type: &str,
    cashier_id: Uuid,
    total: f64,
    cash_amount: Option<f64>,
    card_amount: Option<f64>,
    change_amount: Option<f64>,
    notes: Option<&str>,
    payment_method: Option<&str>,
    input: &ReceiptCreateInput,
) -> Result<(NaiveDateTime,), PosError> {
    let store_id = current_store_ctx()
        .map(|c| c.store_id)
        .ok_or_else(|| PosError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string()))?;
    let row = sqlx::query(
        r#"
        INSERT INTO receipts (
            id, receipt_number, receipt_type, cashier_id, total_amount, paid_amount,
            change_amount, cash_amount, card_amount, is_return, notes, payment_method,
            terminal_rrn, terminal_approval_code, terminal_invoice_number,
            terminal_transaction_id, terminal_response_code, terminal_status,
            terminal_receipt, terminal_card_pan, terminal_payment_system,
            terminal_merchant, terminal_created_at, is_fiscal, fiscal_status,
            split_group_id, store_id, created_at
        ) VALUES (
            $1, $2, $3::receipt_type, $4, $5, NULL, $6, $7, $8, $9, $10,
            $11::receipt_payment_method,
            $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23,
            $24::fiscal_status, $25, $26,
            (now() AT TIME ZONE 'UTC')::timestamp
        )
        RETURNING created_at::text
        "#,
    )
    .bind(id)
    .bind(number)
    .bind(receipt_type)
    .bind(cashier_id)
    .bind(total)
    .bind(change_amount)
    .bind(cash_amount)
    .bind(card_amount)
    .bind(receipt_type == "return")
    .bind(notes)
    .bind(payment_method)
    .bind(input.terminal_rrn.as_deref())
    .bind(input.terminal_approval_code.as_deref())
    .bind(input.terminal_invoice_number.as_deref())
    .bind(input.terminal_transaction_id.as_deref())
    .bind(input.terminal_response_code.as_deref())
    .bind(input.terminal_status.as_deref())
    .bind(input.terminal_receipt.as_deref())
    .bind(input.terminal_card_pan.as_deref())
    .bind(input.terminal_payment_system.as_deref())
    .bind(input.terminal_merchant.as_deref())
    .bind(input.terminal_created_at)
    .bind(input.is_fiscal)
    .bind(if input.is_fiscal { "pending" } else { "none" })
    .bind(input.split_group_id)
    .bind(store_id)
    .fetch_one(&mut **tx)
    .await
    .pe()?;
    let created: String = row.get("created_at");
    let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    Ok((created,))
}

/// Читає чек (після commit) і будує ReceiptDto.
async fn fetch_receipt_dto(
    pool: &StorePool,
    id: Uuid,
    cash_amount: Option<f64>,
    card_amount: Option<f64>,
    change_amount: Option<f64>,
    total: f64,
) -> Result<ReceiptDto, PosError> {
    let row = sqlx::query(
        r#"
        SELECT r.receipt_number, r.receipt_type::text, r.payment_method::text, r.is_fiscal,
               r.fiscal_status::text, r.fiscal_number, r.fiscal_serial, r.fiscal_sent_at::text,
               r.fiscal_error, r.split_group_id, r.cashier_id, r.debtor_id,
               r.terminal_rrn, r.terminal_approval_code, r.terminal_invoice_number,
               r.terminal_transaction_id, r.terminal_response_code, r.terminal_status,
               r.terminal_receipt, r.terminal_card_pan, r.terminal_payment_system,
               r.terminal_merchant, r.terminal_created_at::text, r.created_at::text
        FROM receipts r WHERE r.id = $1
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .pe()?;
    let number: String = row.get("receipt_number");
    let payment_method: Option<String> = row.get("payment_method");
    let created: String = row.get("created_at");
    let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let items = fetch_receipt_items_short(pool, id).await?;
    Ok(ReceiptDto {
        id,
        number,
        items,
        total: Some(total),
        payment_method: payment_method.unwrap_or_else(|| "cash".to_string()),
        created_at: Some(iso_utc_z(created)),
        cash_amount,
        card_amount,
        change_amount,
        customer_id: row.try_get::<Option<Uuid>, _>("debtor_id").ok().flatten(),
        notes: String::new(),
        is_fiscal: row.get("is_fiscal"),
        fiscal_status: row.get("fiscal_status"),
        fiscal_number: row.get("fiscal_number"),
        fiscal_serial: row.get("fiscal_serial"),
        fiscal_sent_at: normalize_utc_z(row.get("fiscal_sent_at")),
        fiscal_error: row.get("fiscal_error"),
        split_group_id: row.get("split_group_id"),
        terminal_rrn: row.get("terminal_rrn"),
        terminal_approval_code: row.get("terminal_approval_code"),
        terminal_invoice_number: row.get("terminal_invoice_number"),
        terminal_transaction_id: row.get("terminal_transaction_id"),
        terminal_response_code: row.get("terminal_response_code"),
        terminal_status: row.get("terminal_status"),
        terminal_receipt: row.get("terminal_receipt"),
        terminal_card_pan: row.get("terminal_card_pan"),
        terminal_payment_system: row.get("terminal_payment_system"),
        terminal_merchant: row.get("terminal_merchant"),
        terminal_created_at: row.get("terminal_created_at"),
        fiscal_check_url: None,
    })
}

/// Позиції чеку (v2: name="", tax_rate=20 — ORM-шлях Python).
async fn fetch_receipt_items_short(
    pool: &StorePool,
    receipt_id: Uuid,
) -> Result<Vec<ReceiptItemDto>, PosError> {
    let rows = sqlx::query(
        "SELECT ri.product_id, p.title, ri.quantity::text, ri.price::text
         FROM receipt_items ri
         LEFT JOIN products p ON p.id = ri.product_id
         WHERE ri.receipt_id = $1 ORDER BY ri.created_at",
    )
    .bind(receipt_id)
    .fetch_all(pool)
    .await
    .pe()?;
    let mut items = Vec::new();
    for r in rows {
        let q: String = r.get("quantity");
        let p: String = r.get("price");
        items.push(ReceiptItemDto {
            product_id: r.get("product_id"),
            name: r.get::<Option<String>, _>("title").unwrap_or_default(),
            quantity: parse_scaled3(&q).unwrap_or(0) as f64 / 1000.0,
            price: parse_scaled2(&p).unwrap_or(0) as f64 / 100.0,
            tax_rate: 20,
        });
    }
    Ok(items)
}

/// Загальний шлях створення чеку sale/return.
async fn create_receipt_impl(
    pool: &StorePool,
    input: &ReceiptCreateInput,
    receipt_type: &str,
) -> Result<ReceiptDto, PosError> {
    if input.items.is_empty() {
        return Err(PosError::Validation("items: Field required".to_string()));
    }
    let total_scaled2 = receipt_total_scaled2(&input.items);
    let total = total_scaled2 as f64 / 100.0;

    // Валідації — ДО транзакції (як Python).
    let require_rrn = receipt_type == "return";
    if receipt_type == "sale" {
        validate_payment(input, total_scaled2)?;
    }
    validate_terminal(input, require_rrn)?;

    // set_payment (cash, cash >= total) → change_amount.
    let mut cash_amount = input
        .cash_amount
        .as_deref()
        .and_then(parse_scaled2)
        .map(|v| v as f64 / 100.0);
    let mut card_amount = input
        .card_amount
        .as_deref()
        .and_then(parse_scaled2)
        .map(|v| v as f64 / 100.0);
    let mut change_amount: Option<f64> = None;
    if input.payment_method.to_lowercase() == "cash" {
        if let Some(c) = cash_amount {
            let c_scaled = (c * 100.0).round() as i64;
            if c_scaled >= total_scaled2 {
                change_amount = Some((c_scaled - total_scaled2) as f64 / 100.0);
            }
        }
    }
    if cash_amount.is_none() {
        cash_amount = None;
    }
    if card_amount.is_none() {
        card_amount = None;
    }

    let cashier_id = input.cashier_id.ok_or_else(|| {
        PosError::BadRequest("Відсутній ідентифікатор касира в токені".to_string())
    })?;
    let number = receipt_number(Utc::now().naive_utc());
    let id = Uuid::new_v4();
    let notes = if input.notes.is_empty() {
        None
    } else {
        Some(input.notes.as_str())
    };
    let pm = if input.payment_method.is_empty() {
        None
    } else {
        Some(input.payment_method.as_str())
    };

    let mut tx = pool.begin().await.pe()?;
    let store_id = current_store_ctx()
        .map(|c| c.store_id)
        .ok_or_else(|| PosError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string()))?;

    // Перевірка товарів + оновлення залишків stock (Етап 3: per store, атомарно).
    for item in &input.items {
        let title: Option<String> = sqlx::query_scalar("SELECT title FROM products WHERE id = $1")
            .bind(item.product_id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?;
        let title = title.ok_or_else(|| {
            PosError::BadRequest(format!(
                "Товар з ID '{}' не знайдено",
                item.product_id
            ))
        })?;
        // Валідація формату quantity (як Python Decimal).
        let _qty = parse_scaled3(&item.quantity).ok_or_else(|| {
            PosError::Validation(format!(
                "quantity: невалідне десяткове число: {}",
                item.quantity
            ))
        })?;
        if receipt_type == "sale" {
            // Атомарний продаж: зменшуємо ЛИШЕ якщо залишку достатньо
            // (UPDATE ... WHERE quantity >= qty; 0 рядків → «недостатньо»).
            let res = sqlx::query(
                "UPDATE stock SET quantity = quantity - $1::numeric, updated_at = now()
                 WHERE store_id = $2 AND product_id = $3 AND quantity >= $1::numeric",
            )
            .bind(&item.quantity)
            .bind(store_id)
            .bind(item.product_id)
            .execute(&mut *tx)
            .await
            .pe()?;
            if res.rows_affected() == 0 {
                let avail: Option<String> = sqlx::query_scalar(
                    "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
                )
                .bind(store_id)
                .bind(item.product_id)
                .fetch_optional(&mut *tx)
                .await
                .pe()?
                .flatten();
                let avail = avail.unwrap_or_else(|| "0".to_string());
                return Err(PosError::BadRequest(format!(
                    "Недостатньо залишку товару '{}': доступно {}, потрібно {}",
                    title, avail, item.quantity
                )));
            }
            // ФІКС 2026-08-21: products.stock (сумарний, Python-еталон) -= qty.
            sqlx::query(
                "UPDATE products SET stock = GREATEST(0, COALESCE(stock, 0) - $1::numeric), updated_at = now()
                 WHERE id = $2",
            )
            .bind(&item.quantity)
            .bind(item.product_id)
            .execute(&mut *tx)
            .await
            .pe()?;
        } else {
            // Повернення: додаємо залишок (upsert, якщо рядка ще немає).
            sqlx::query(
                "INSERT INTO stock (store_id, product_id, quantity, price, updated_at)
                 VALUES ($1, $2, $3::numeric, 0, now())
                 ON CONFLICT (store_id, product_id) DO UPDATE
                    SET quantity = stock.quantity + EXCLUDED.quantity, updated_at = now()",
            )
            .bind(store_id)
            .bind(item.product_id)
            .bind(&item.quantity)
            .execute(&mut *tx)
            .await
            .pe()?;
            // ФІКС 2026-08-21: products.stock (сумарний, Python-еталон) += qty.
            sqlx::query(
                "UPDATE products SET stock = COALESCE(stock, 0) + $1::numeric, updated_at = now()
                 WHERE id = $2",
            )
            .bind(&item.quantity)
            .bind(item.product_id)
            .execute(&mut *tx)
            .await
            .pe()?;
        }
    }

    let (created,) = insert_receipt(
        &mut tx,
        id,
        &number,
        receipt_type,
        cashier_id,
        total,
        cash_amount,
        card_amount,
        change_amount,
        notes,
        pm,
        input,
    )
    .await?;

    // Позиції (purchase_price = product.cost_price).
    for item in &input.items {
        let cost: Option<f64> = sqlx::query("SELECT cost_price::text FROM products WHERE id = $1")
            .bind(item.product_id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?
            .and_then(|r| r.try_get::<Option<String>, _>("cost_price").ok().flatten())
            .and_then(|s| s.parse::<f64>().ok());
        let item_total = {
            let q = parse_scaled3(&item.quantity).unwrap_or(0) as i128;
            let p = parse_scaled2(&item.price).unwrap_or(0) as i128;
            (q * p) as f64 / 100_000.0
        };
        sqlx::query(
            r#"
            INSERT INTO receipt_items (id, receipt_id, product_id, quantity, price, total,
                purchase_price, fiscal_quantity, store_id, created_at)
            VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7, 0, $8,
                (now() AT TIME ZONE 'UTC')::timestamp)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(item.product_id)
        .bind(dec3(parse_scaled3(&item.quantity).unwrap_or(0)))
        .bind(dec2(parse_scaled2(&item.price).unwrap_or(0)))
        .bind(dec2((item_total * 100.0).round() as i64))
        .bind(cost)
        .bind(store_id)
        .execute(&mut *tx)
        .await
        .pe()?;
    }

    tx.commit().await.pe()?;

    let _ = created;
    fetch_receipt_dto(pool, id, cash_amount, card_amount, change_amount, total).await
}

fn dec3(v: i64) -> String {
    let (sign, v) = if v < 0 { ("-", -v) } else { ("", v) };
    format!("{sign}{}.{:03}", v / 1000, v % 1000)
}

// ─── Чеки v2: читання ──────────────────────────────────────────────────────

/// Позиції чеку з товарами (GET /{id}/items — v2 ReceiptItemsResponse).
async fn fetch_receipt_items_detail(
    pool: &StorePool,
    receipt_id: Uuid,
) -> Result<Vec<ReceiptItemDetailDto>, PosError> {
    let rows = sqlx::query(
        r#"
        SELECT ri.id, ri.product_id, p.title, p.barcode, ri.quantity::text, ri.price::text,
               ri.total::text, ri.purchase_price::text, ri.created_at::text
        FROM receipt_items ri
        LEFT JOIN products p ON p.id = ri.product_id
        WHERE ri.receipt_id = $1
        ORDER BY ri.created_at
        "#,
    )
    .bind(receipt_id)
    .fetch_all(pool)
    .await
    .pe()?;
    let mut out = Vec::new();
    for r in rows {
        let q: String = r.get("quantity");
        let p: String = r.get("price");
        let t: String = r.get("total");
        let pp: Option<String> = r.get("purchase_price");
        let created: String = r.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        out.push(ReceiptItemDetailDto {
            id: r.get("id"),
            product_id: r.get("product_id"),
            product_name: r.get::<Option<String>, _>("title").unwrap_or_default(),
            product_barcode: r.get("barcode"),
            quantity: parse_scaled3(&q).unwrap_or(0) as f64 / 1000.0,
            price: parse_scaled2(&p).unwrap_or(0) as f64 / 100.0,
            total: parse_scaled2(&t).unwrap_or(0) as f64 / 100.0,
            purchase_price: pp.and_then(|s| s.parse::<f64>().ok()),
            created_at: Some(iso_utc_z(created)),
        });
    }
    Ok(out)
}

/// Повний чек з БД (GET /{id} — scale колонок).
async fn read_receipt_dto(pool: &StorePool, id: Uuid) -> Result<Option<ReceiptDto>, PosError> {
    let row = sqlx::query(
        r#"
        SELECT r.receipt_number, r.receipt_type::text, r.total_amount::text, r.payment_method::text,
               r.cash_amount::text, r.card_amount::text, r.change_amount::text, r.debtor_id,
               r.notes, r.is_fiscal, r.fiscal_status::text, r.fiscal_number, r.fiscal_serial,
               r.fiscal_sent_at::text, r.fiscal_error, r.split_group_id,
               r.terminal_rrn, r.terminal_approval_code, r.terminal_invoice_number,
               r.terminal_transaction_id, r.terminal_response_code, r.terminal_status,
               r.terminal_receipt, r.terminal_card_pan, r.terminal_payment_system,
               r.terminal_merchant, r.terminal_created_at::text, r.created_at::text
        FROM receipts r WHERE r.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .pe()?;
    let Some(row) = row else { return Ok(None) };
    let number: String = row.get("receipt_number");
    let payment_method: Option<String> = row.get("payment_method");
    let total: Option<String> = row.get("total_amount");
    let cash: Option<String> = row.get("cash_amount");
    let card: Option<String> = row.get("card_amount");
    let change: Option<String> = row.get("change_amount");
    let created: String = row.get("created_at");
    let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let items = fetch_receipt_items_short(pool, id).await?;
    Ok(Some(ReceiptDto {
        id,
        number,
        items,
        total: total.and_then(|s| s.parse::<f64>().ok()),
        payment_method: payment_method.unwrap_or_else(|| "cash".to_string()),
        created_at: Some(iso_utc_z(created)),
        cash_amount: cash.and_then(|s| s.parse::<f64>().ok()),
        card_amount: card.and_then(|s| s.parse::<f64>().ok()),
        change_amount: change.and_then(|s| s.parse::<f64>().ok()),
        customer_id: row.get("debtor_id"),
        notes: row.get::<Option<String>, _>("notes").unwrap_or_default(),
        is_fiscal: row.get("is_fiscal"),
        fiscal_status: row.get("fiscal_status"),
        fiscal_number: row.get("fiscal_number"),
        fiscal_serial: row.get("fiscal_serial"),
        fiscal_sent_at: normalize_utc_z(row.get("fiscal_sent_at")),
        fiscal_error: row.get("fiscal_error"),
        split_group_id: row.get("split_group_id"),
        terminal_rrn: row.get("terminal_rrn"),
        terminal_approval_code: row.get("terminal_approval_code"),
        terminal_invoice_number: row.get("terminal_invoice_number"),
        terminal_transaction_id: row.get("terminal_transaction_id"),
        terminal_response_code: row.get("terminal_response_code"),
        terminal_status: row.get("terminal_status"),
        terminal_receipt: row.get("terminal_receipt"),
        terminal_card_pan: row.get("terminal_card_pan"),
        terminal_payment_system: row.get("terminal_payment_system"),
        terminal_merchant: row.get("terminal_merchant"),
        terminal_created_at: row.get("terminal_created_at"),
        fiscal_check_url: None,
    }))
}

/// Список чеків (GET "" — v2 list_receipts).
async fn list_receipts_impl(
    pool: &StorePool,
    q: &ReceiptListQuery,
) -> Result<ReceiptListDto, PosError> {
    let mut sql = String::from(
        "SELECT r.id, r.receipt_number, r.receipt_type::text, r.total_amount::text, r.payment_method::text, \
         r.cash_amount::text, r.card_amount::text, r.change_amount::text, r.debtor_id, r.notes, \
         r.is_fiscal, r.fiscal_status::text, r.fiscal_number, r.fiscal_serial, r.fiscal_sent_at::text, \
         r.fiscal_error, r.split_group_id, r.terminal_rrn, r.terminal_approval_code, \
         r.terminal_invoice_number, r.terminal_transaction_id, r.terminal_response_code, \
         r.terminal_status, r.terminal_receipt, r.terminal_card_pan, r.terminal_payment_system, \
         r.terminal_merchant, r.terminal_created_at::text, r.created_at::text \
         FROM receipts r WHERE 1=1",
    );
    let mut count_sql = String::from("SELECT count(*) FROM receipts r WHERE 1=1");
    let mut binds: Vec<String> = Vec::new();
    if let Some(search) = &q.search {
        if !search.is_empty() {
            binds.push(format!("%{search}%"));
            let idx = binds.len();
            sql.push_str(&format!(
                " AND (r.receipt_number ILIKE ${idx} OR r.notes ILIKE ${idx})"
            ));
            count_sql.push_str(&format!(
                " AND (r.receipt_number ILIKE ${idx} OR r.notes ILIKE ${idx})"
            ));
        }
    }
    if let Some(dt) = q.date_from {
        binds.push(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string());
        let idx = binds.len();
        sql.push_str(&format!(" AND r.created_at >= ${idx}::timestamp"));
        count_sql.push_str(&format!(" AND r.created_at >= ${idx}::timestamp"));
    }
    if let Some(dt) = q.date_to {
        binds.push(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string());
        let idx = binds.len();
        sql.push_str(&format!(" AND r.created_at <= ${idx}::timestamp"));
        count_sql.push_str(&format!(" AND r.created_at <= ${idx}::timestamp"));
    }
    if let Some(pm) = &q.payment_method {
        binds.push(pm.clone());
        let idx = binds.len();
        sql.push_str(&format!(" AND r.payment_method::text = ${idx}"));
        count_sql.push_str(&format!(" AND r.payment_method::text = ${idx}"));
    }
    let total: i64 = {
        let mut cq = sqlx::query(&count_sql);
        for b in &binds {
            cq = cq.bind(b);
        }
        cq.fetch_one(pool).await.pe()?.get("count")
    };
    let offset = (q.page - 1) * q.size;
    sql.push_str(" ORDER BY r.created_at DESC LIMIT ");
    sql.push_str(&q.size.to_string());
    sql.push_str(" OFFSET ");
    sql.push_str(&offset.to_string());
    let mut qr = sqlx::query(&sql);
    for b in &binds {
        qr = qr.bind(b);
    }
    let rows = qr.fetch_all(pool).await.pe()?;
    let mut items = Vec::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let created: String = row.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let total_s: Option<String> = row.get("total_amount");
        let cash: Option<String> = row.get("cash_amount");
        let card: Option<String> = row.get("card_amount");
        let change: Option<String> = row.get("change_amount");
        let items_short = fetch_receipt_items_short(pool, id).await?;
        items.push(ReceiptDto {
            id,
            number: row.get("receipt_number"),
            items: items_short,
            total: total_s.and_then(|s| s.parse::<f64>().ok()),
            payment_method: row
                .get::<Option<String>, _>("payment_method")
                .unwrap_or_else(|| "cash".to_string()),
            created_at: Some(iso_utc_z(created)),
            cash_amount: cash.and_then(|s| s.parse::<f64>().ok()),
            card_amount: card.and_then(|s| s.parse::<f64>().ok()),
            change_amount: change.and_then(|s| s.parse::<f64>().ok()),
            customer_id: row.get("debtor_id"),
            notes: row.get::<Option<String>, _>("notes").unwrap_or_default(),
            is_fiscal: row.get("is_fiscal"),
            fiscal_status: row.get("fiscal_status"),
            fiscal_number: row.get("fiscal_number"),
            fiscal_serial: row.get("fiscal_serial"),
            fiscal_sent_at: normalize_utc_z(row.get("fiscal_sent_at")),
            fiscal_error: row.get("fiscal_error"),
            split_group_id: row.get("split_group_id"),
            terminal_rrn: row.get("terminal_rrn"),
            terminal_approval_code: row.get("terminal_approval_code"),
            terminal_invoice_number: row.get("terminal_invoice_number"),
            terminal_transaction_id: row.get("terminal_transaction_id"),
            terminal_response_code: row.get("terminal_response_code"),
            terminal_status: row.get("terminal_status"),
            terminal_receipt: row.get("terminal_receipt"),
            terminal_card_pan: row.get("terminal_card_pan"),
            terminal_payment_system: row.get("terminal_payment_system"),
            terminal_merchant: row.get("terminal_merchant"),
            terminal_created_at: row.get("terminal_created_at"),
            fiscal_check_url: None,
        });
    }
    Ok(ReceiptListDto {
        items,
        total,
        page: q.page,
        size: q.size,
    })
}

/// ПДВ позиції (scale12, floor-похибка < 1e-12 — наближення до Decimal 28 знаків).
/// vat = total * rate / (1 + rate), total = price*qty (scale5).
fn calc_vat_scaled12(price_scaled2: i64, qty_scaled3: i64, tax_rate: i64) -> i128 {
    if tax_rate == 0 {
        return 0;
    }
    let total = (price_scaled2 as i128) * (qty_scaled3 as i128);
    // vat (scale5) = total*tax_rate/(100+tax_rate); scale12 = scale5 × 10^7.
    // HALF_UP на проміжному діленні (похибка < 5e-13/позицію — наближення
    // до Decimal-точності Python, уникає накопичення floor-похибки).
    let num = total * (tax_rate as i128) * 10_000_000;
    let den = (100 + tax_rate) as i128;
    (num + den / 2) / den
}

/// GET /api/v2/receipts/stats/today.
async fn today_stats_impl(pool: &StorePool) -> Result<ReceiptStatsDto, PosError> {
    let now = Utc::now().naive_utc();
    let day = now.date();
    let start = day.and_hms_opt(0, 0, 0).unwrap();
    let end = day.and_hms_nano_opt(23, 59, 59, 999_999_000).unwrap();
    // Окремо (як Python): суми і count — без JOIN (інакше count(*) множить чеки
    // з кількома позиціями); items_sold — окремий JOIN-запит.
    let row = sqlx::query(
        r#"
        SELECT
          coalesce(sum(total_amount) FILTER (WHERE receipt_type = 'sale'), 0)::text AS sales,
          coalesce(sum(total_amount) FILTER (WHERE receipt_type = 'return'), 0)::text AS returns
        FROM receipts r
        WHERE r.created_at >= $1 AND r.created_at <= $2
        "#,
    )
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await
    .pe()?;
    let cnt: i64 =
        sqlx::query("SELECT count(*) FROM receipts WHERE created_at >= $1 AND created_at <= $2")
            .bind(start)
            .bind(end)
            .fetch_one(pool)
            .await
            .pe()?
            .get("count");
    let items: Option<String> = sqlx::query(
        r#"
        SELECT coalesce(sum(ri.quantity), 0)::text AS items
        FROM receipt_items ri JOIN receipts r ON r.id = ri.receipt_id
        WHERE r.receipt_type = 'sale' AND r.created_at >= $1 AND r.created_at <= $2
        "#,
    )
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await
    .pe()?
    .get("items");
    let sales: Option<String> = row.get("sales");
    let returns: Option<String> = row.get("returns");

    // Прибуток + ПДВ (Python-цикл по sale-позиціях).
    let rows = sqlx::query(
        r#"
        SELECT ri.total::text, ri.price::text, ri.quantity::text, ri.purchase_price::text,
               p.cost_price::text, p.tax_rate::text
        FROM receipt_items ri
        JOIN receipts r ON r.id = ri.receipt_id
        LEFT JOIN products p ON p.id = ri.product_id
        WHERE r.receipt_type = 'sale' AND r.created_at >= $1 AND r.created_at <= $2
        "#,
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .pe()?;
    let mut profit: f64 = 0.0;
    let mut vat_scaled12: i128 = 0;
    for r in rows {
        let total: String = r.get("total");
        let price: String = r.get("price");
        let quantity: String = r.get("quantity");
        let pp: Option<String> = r.get("purchase_price");
        let cost: Option<String> = r.get("cost_price");
        let tax: Option<String> = r.get("tax_rate");
        let purchase = pp.or(cost).and_then(|s| s.parse::<f64>().ok());
        if let Some(pc) = purchase {
            let t = total.parse::<f64>().unwrap_or(0.0);
            let q = parse_scaled3(&quantity).unwrap_or(0) as f64 / 1000.0;
            profit += t - pc * q;
        }
        let p2 = parse_scaled2(&price).unwrap_or(0);
        let q3 = parse_scaled3(&quantity).unwrap_or(0);
        let rate = tax.and_then(|s| parse_scaled2(&s)).unwrap_or(0) / 100;
        vat_scaled12 += calc_vat_scaled12(p2, q3, rate);
    }
    let vat: f64 = (vat_scaled12 / 10_000_000) as f64 / 100_000.0
        + (vat_scaled12 % 10_000_000) as f64 / 1_000_000_000_000.0;
    Ok(ReceiptStatsDto {
        total_sales: sales.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
        total_returns: returns.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
        total_profit: profit,
        total_vat: vat,
        receipts_count: cnt,
        items_sold: items
            .and_then(|s| s.parse::<f64>().ok())
            .map(|v| v.round() as i64)
            .unwrap_or(0),
        date: now.format("%Y-%m-%d").to_string(),
    })
}

/// Пошук чеків для повернень (GET /search).
async fn search_receipts_impl(
    pool: &StorePool,
    q: &ReceiptSearchQuery,
) -> Result<ReceiptSearchDto, PosError> {
    let rtype = q.receipt_type.as_deref().unwrap_or("sale");
    if rtype != "sale" && rtype != "return" {
        return Err(PosError::BadRequest(format!(
            "Невірний тип чеку '{}'. Використовуйте 'sale' або 'return'",
            rtype
        )));
    }
    let mut base = format!(
        "SELECT r.id, r.receipt_number, r.receipt_type::text, r.total_amount::text, \
         r.created_at::text, u.name FROM receipts r \
         LEFT JOIN users u ON u.id = r.cashier_id \
         LEFT JOIN receipt_items ri ON ri.receipt_id = r.id \
         LEFT JOIN products p ON p.id = ri.product_id \
         WHERE r.receipt_type = '{}'",
        rtype
    );
    let mut count = format!(
        "SELECT count(DISTINCT r.id) FROM receipts r \
         LEFT JOIN receipt_items ri ON ri.receipt_id = r.id \
         LEFT JOIN products p ON p.id = ri.product_id \
         WHERE r.receipt_type = '{}'",
        rtype
    );
    let mut binds: Vec<String> = Vec::new();
    if let Some(dt) = q.date_from {
        binds.push(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string());
        let i = binds.len();
        base.push_str(&format!(" AND r.created_at >= ${i}::timestamp"));
        count.push_str(&format!(" AND r.created_at >= ${i}::timestamp"));
    }
    if let Some(dt) = q.date_to {
        binds.push(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string());
        let i = binds.len();
        base.push_str(&format!(" AND r.created_at <= ${i}::timestamp"));
        count.push_str(&format!(" AND r.created_at <= ${i}::timestamp"));
    }
    let qq = q.q.trim();
    if !qq.is_empty() {
        binds.push(format!("%{qq}%"));
        let i = binds.len();
        base.push_str(&format!(
            " AND (r.receipt_number ILIKE ${i} OR p.title ILIKE ${i})"
        ));
        count.push_str(&format!(
            " AND (r.receipt_number ILIKE ${i} OR p.title ILIKE ${i})"
        ));
    }
    let total: i64 = {
        let mut cq = sqlx::query(&count);
        for b in &binds {
            cq = cq.bind(b);
        }
        cq.fetch_one(pool).await.pe()?.get("count")
    };
    let offset = (q.page - 1) * q.size;
    base.push_str(
        " GROUP BY r.id, r.receipt_number, r.receipt_type, r.total_amount, r.created_at, u.name",
    );
    base.push_str(&format!(
        " ORDER BY r.created_at DESC LIMIT {} OFFSET {}",
        q.size, offset
    ));
    let mut qr = sqlx::query(&base);
    for b in &binds {
        qr = qr.bind(b);
    }
    let rows = qr.fetch_all(pool).await.pe()?;
    let mut items = Vec::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let created: String = row.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let item_count: i64 =
            sqlx::query("SELECT count(*) FROM receipt_items WHERE receipt_id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .pe()?
                .get("count");
        items.push(ReceiptSearchItemDto {
            id,
            receipt_number: row.get("receipt_number"),
            receipt_type: row.get("receipt_type"),
            total_amount: row
                .get::<Option<String>, _>("total_amount")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0),
            created_at: Some(iso_utc_z(created)),
            cashier_name: row.get::<Option<String>, _>("name").unwrap_or_default(),
            items_count: item_count,
        });
    }
    let pages = if total > 0 {
        (total + q.size - 1) / q.size
    } else {
        1
    };
    Ok(ReceiptSearchDto {
        items,
        total,
        page: q.page,
        page_size: q.size,
        pages,
    })
}

/// Останні продажі товару (GET /by-product/{query}/recent-sales).
async fn recent_sales_impl(
    pool: &StorePool,
    query: &str,
    limit: i64,
) -> Result<Vec<ProductRecentSalesDto>, PosError> {
    let products = sqlx::query(
        "SELECT id, title, barcode, price::text, unit FROM products \
         WHERE barcode = $1 OR title ILIKE $2 ORDER BY title LIMIT 20",
    )
    .bind(query)
    .bind(format!("%{query}%"))
    .fetch_all(pool)
    .await
    .pe()?;
    let mut items = Vec::new();
    for p in products {
        let pid: Uuid = p.get("id");
        let sold_returned = sold_returned_totals(pool, pid).await?;
        let returnable = (sold_returned.0 - sold_returned.1).max(0.0);
        let sales = sqlx::query(
            r#"
            SELECT ri.receipt_id, r.receipt_number, ri.created_at::text,
                   ri.quantity::text, ri.price::text
            FROM receipt_items ri
            JOIN receipts r ON r.id = ri.receipt_id
            WHERE ri.product_id = $1 AND r.receipt_type = 'sale'
            ORDER BY r.created_at DESC LIMIT $2
            "#,
        )
        .bind(pid)
        .bind(limit)
        .fetch_all(pool)
        .await
        .pe()?;
        let mut recent = Vec::new();
        for s in sales {
            let created: String = s.get("created_at");
            let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
                .unwrap_or_else(|_| Utc::now().naive_utc());
            let q: String = s.get("quantity");
            let pr: String = s.get("price");
            recent.push(RecentSaleDto {
                receipt_id: s.get("receipt_id"),
                receipt_number: s.get("receipt_number"),
                created_at: Some(iso_utc_z(created)),
                quantity: parse_scaled3(&q).unwrap_or(0) as f64 / 1000.0,
                price: parse_scaled2(&pr).unwrap_or(0) as f64 / 100.0,
            });
        }
        items.push(ProductRecentSalesDto {
            product: ProductBriefInfoDto {
                id: pid,
                title: p.get("title"),
                barcode: p.get("barcode"),
                price: p
                    .get::<Option<String>, _>("price")
                    .and_then(|s| s.parse::<f64>().ok()),
                unit: p.get("unit"),
            },
            total_sold: sold_returned.0,
            total_returned: sold_returned.1,
            returnable,
            recent_sales: recent,
        });
    }
    Ok(items)
}

/// (продано, повернуто) у float.
async fn sold_returned_totals(pool: &StorePool, product_id: Uuid) -> Result<(f64, f64), PosError> {
    let row = sqlx::query(
        r#"
        SELECT
          coalesce(sum(quantity) FILTER (WHERE r.receipt_type = 'sale'), 0)::text AS sold,
          coalesce(sum(quantity) FILTER (WHERE r.receipt_type = 'return'), 0)::text AS ret
        FROM receipt_items ri JOIN receipts r ON r.id = ri.receipt_id
        WHERE ri.product_id = $1
        "#,
    )
    .bind(product_id)
    .fetch_one(pool)
    .await
    .pe()?;
    let sold: Option<String> = row.get("sold");
    let ret: Option<String> = row.get("ret");
    Ok((
        sold.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
        ret.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
    ))
}

// ─── Робочі сесії ──────────────────────────────────────────────────────────

const MAX_SESSION_HOURS: f64 = 24.0;

/// Ефективна тривалість сесії (Python _effective_duration).
fn effective_duration(
    logout: Option<NaiveDateTime>,
    duration: Option<f64>,
    login: NaiveDateTime,
    now: NaiveDateTime,
) -> f64 {
    if logout.is_some() {
        return duration.unwrap_or(0.0);
    }
    let live = (now - login).num_seconds() as f64 / 3600.0;
    (live.min(MAX_SESSION_HOURS) * 100.0).round() / 100.0
}

async fn read_session_dto(
    pool: &StorePool,
    id: Uuid,
    user_id: Uuid,
    now: NaiveDateTime,
) -> Result<WorkSessionDto, PosError> {
    let row = sqlx::query(
        "SELECT login_time::text, logout_time::text, duration_hours::text FROM work_sessions WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .pe()?;
    let login: String = row.get("login_time");
    let login = NaiveDateTime::parse_from_str(&login, "%Y-%m-%d %H:%M:%S%.f").unwrap_or(now);
    let logout: Option<String> = row.get("logout_time");
    let logout =
        logout.and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f").ok());
    let duration: Option<String> = row.get("duration_hours");
    let duration = duration.and_then(|s| s.parse::<f64>().ok());
    let dur = effective_duration(logout, duration, login, now);
    Ok(WorkSessionDto {
        id,
        user_id,
        login_time: iso_utc_z(login),
        logout_time: logout.map(iso_utc_z),
        duration_hours: Some(dur),
        is_active: logout.is_none(),
    })
}

async fn my_sessions_impl(
    pool: &StorePool,
    user_id: Uuid,
    month: i64,
    year: i64,
) -> Result<MySessionsDto, PosError> {
    let (start, end) = month_bounds(month, year);
    let now = Utc::now().naive_utc();
    let rows = sqlx::query(
        "SELECT id FROM work_sessions WHERE user_id = $1 AND login_time >= $2 AND login_time < $3 \
         ORDER BY login_time DESC",
    )
    .bind(user_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .pe()?;
    let mut sessions = Vec::new();
    for r in rows {
        sessions.push(read_session_dto(pool, r.get("id"), user_id, now).await?);
    }
    let total_hours: f64 = sessions
        .iter()
        .map(|s| s.duration_hours.unwrap_or(0.0))
        .sum();
    let hourly: Option<f64> = sqlx::query("SELECT hourly_rate::text FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .pe()?
        .and_then(|r| r.try_get::<Option<String>, _>("hourly_rate").ok().flatten())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| *v != 0.0);
    Ok(MySessionsDto {
        sessions,
        total_hours: (total_hours * 100.0).round() / 100.0,
        hourly_rate: hourly,
    })
}

fn month_bounds(month: i64, year: i64) -> (chrono::NaiveDateTime, chrono::NaiveDateTime) {
    let start = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let end = if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year as i32 + 1, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    } else {
        chrono::NaiveDate::from_ymd_opt(year as i32, (month + 1) as u32, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    };
    (start, end)
}

async fn work_report_impl(pool: &StorePool, month: i64, year: i64) -> Result<WorkReportDto, PosError> {
    let (start, end) = month_bounds(month, year);
    let now = Utc::now().naive_utc();
    let users = sqlx::query("SELECT id, name, hourly_rate::text FROM users ORDER BY name")
        .fetch_all(pool)
        .await
        .pe()?;
    let mut items = Vec::new();
    for u in users {
        let uid: Uuid = u.get("id");
        let sessions = sqlx::query(
            "SELECT id FROM work_sessions WHERE user_id = $1 AND login_time >= $2 AND login_time < $3",
        )
        .bind(uid)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await
        .pe()?;
        let mut total_hours = 0.0;
        for s in sessions {
            let dto = read_session_dto(pool, s.get("id"), uid, now).await?;
            total_hours += dto.duration_hours.unwrap_or(0.0);
        }
        let hourly = u
            .get::<Option<String>, _>("hourly_rate")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let salary = if hourly != 0.0 {
            Some(((total_hours * hourly) * 100.0).round() / 100.0)
        } else {
            None
        };
        items.push(UserHoursSummaryDto {
            user_id: uid,
            user_name: u.get("name"),
            total_hours: (total_hours * 100.0).round() / 100.0,
            hourly_rate: if hourly != 0.0 { Some(hourly) } else { None },
            salary: if salary.unwrap_or(0.0) != 0.0 {
                salary
            } else {
                None
            },
        });
    }
    Ok(WorkReportDto { month, year, items })
}

async fn user_sessions_impl(
    pool: &StorePool,
    user_id: Uuid,
    month: i64,
    year: i64,
) -> Result<UserSessionsDto, PosError> {
    let user = sqlx::query("SELECT name FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .pe()?;
    let Some(user) = user else {
        return Err(PosError::NotFound(format!(
            "Користувача з ID '{user_id}' не знайдено"
        )));
    };
    let user_name: String = user.get("name");
    let (start, end) = month_bounds(month, year);
    let now = Utc::now().naive_utc();
    let rows = sqlx::query(
        "SELECT id FROM work_sessions WHERE user_id = $1 AND login_time >= $2 AND login_time < $3 \
         ORDER BY login_time DESC",
    )
    .bind(user_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .pe()?;
    let mut sessions = Vec::new();
    let mut total_hours = 0.0;
    for r in rows {
        let dto = read_session_dto(pool, r.get("id"), user_id, now).await?;
        total_hours += dto.duration_hours.unwrap_or(0.0);
        sessions.push(dto);
    }
    Ok(UserSessionsDto {
        user_id,
        user_name,
        total_hours: (total_hours * 100.0).round() / 100.0,
        sessions,
    })
}

// ─── Списання та переміщення: спільні читання ─────────────────────────────

/// Читає документ (write_off/transfer) з позиціями — scale БД.
#[allow(clippy::too_many_arguments)]
async fn read_write_off_dto(
    pool: &StorePool,
    id: Uuid,
    number: String,
    reason: String,
    date: String,
    notes: Option<String>,
    status: String,
    total_amount: String,
    created: NaiveDateTime,
    updated: NaiveDateTime,
) -> Result<WriteOffDto, PosError> {
    let items = sqlx::query(
        "SELECT wi.id, wi.write_off_id, wi.product_id, p.title, wi.quantity::text, \
         wi.cost_price::text, wi.price::text, wi.created_at::text \
         FROM write_off_items wi JOIN products p ON p.id = wi.product_id \
         WHERE wi.write_off_id = $1 ORDER BY wi.created_at",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .pe()?;
    let mut dtos = Vec::new();
    for it in items {
        let created: String = it.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let qty = parse_scaled3(&it.get::<String, _>("quantity")).unwrap_or(0) as i128;
        let prc = parse_scaled2(&it.get::<String, _>("price")).unwrap_or(0) as i128;
        dtos.push(WriteOffItemDto {
            id: it.get("id"),
            write_off_id: it.get("write_off_id"),
            product_id: it.get("product_id"),
            product_name: it.get::<Option<String>, _>("title").unwrap_or_default(),
            quantity: it.get("quantity"),
            cost_price: it.get("cost_price"),
            price: it.get("price"),
            total: dec2((qty * prc / 1000) as i64),
            created_at: iso_utc_z(created),
        });
    }
    Ok(WriteOffDto {
        id,
        number,
        reason,
        write_off_date: date,
        notes,
        status,
        total_amount: Some(total_amount),
        created_at: iso_utc_z(created),
        updated_at: iso_utc_z(updated),
        items: dtos,
    })
}

/// Повний read write-off з БД (GET/confirm).
async fn read_write_off(pool: &StorePool, id: Uuid) -> Result<WriteOffDto, PosError> {
    let row = sqlx::query(
        "SELECT number, reason::text, write_off_date::text, notes, status, total_amount::text, \
         created_at::text, updated_at::text FROM write_offs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .pe()?;
    let Some(row) = row else {
        return Err(PosError::NotFound(format!(
            "Списання з ID '{id}' не знайдено"
        )));
    };
    let created: String = row.get("created_at");
    let updated: String = row.get("updated_at");
    let wdate: String = row.get("write_off_date");
    let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let updated = NaiveDateTime::parse_from_str(&updated, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let wdate = NaiveDateTime::parse_from_str(&wdate, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    read_write_off_dto(
        pool,
        id,
        row.get("number"),
        row.get("reason"),
        iso_utc_z(wdate),
        row.get("notes"),
        row.get("status"),
        row.get("total_amount"),
        created,
        updated,
    )
    .await
}

/// Відповідь create write-off: вхідні scale позицій, total_amount "0.0".
#[allow(clippy::too_many_arguments)]
async fn build_write_off_create(
    pool: &StorePool,
    id: Uuid,
    number: String,
    reason: String,
    date: NaiveDateTime,
    notes: Option<String>,
    created: NaiveDateTime,
    updated: NaiveDateTime,
    items: &[(DocItemInput, String, String)],
) -> Result<WriteOffDto, PosError> {
    let mut dtos = Vec::new();
    let mut total_cents: i128 = 0;
    for (item, cost, price) in items {
        let row = sqlx::query(
            "SELECT wi.id, p.title, wi.created_at::text FROM write_off_items wi \
             JOIN products p ON p.id = wi.product_id \
             WHERE wi.write_off_id = $1 AND wi.product_id = $2 ORDER BY wi.created_at DESC LIMIT 1",
        )
        .bind(id)
        .bind(item.product_id)
        .fetch_one(pool)
        .await
        .pe()?;
        let created: String = row.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let qty = parse_scaled3(&item.quantity).unwrap_or(0) as i128;
        let prc = parse_scaled2(price).unwrap_or(0) as i128;
        let it_total = qty * prc / 1000;
        total_cents += it_total;
        dtos.push(WriteOffItemDto {
            id: row.get("id"),
            write_off_id: id,
            product_id: item.product_id,
            product_name: row.get::<Option<String>, _>("title").unwrap_or_default(),
            quantity: item.quantity.clone(),
            cost_price: cost.clone(),
            price: price.clone(),
            total: dec2(it_total as i64),
            created_at: iso_utc_z(created),
        });
    }
    Ok(WriteOffDto {
        id,
        number,
        reason,
        write_off_date: iso_utc_z(date),
        notes,
        status: "draft".to_string(),
        total_amount: Some(dec2(total_cents as i64)),
        created_at: iso_utc_z(created),
        updated_at: iso_utc_z(updated),
        items: dtos,
    })
}

/// Відповідь create transfer: вхідні scale позицій, статус "draft".
#[allow(clippy::too_many_arguments)]
async fn build_transfer_create(
    pool: &StorePool,
    id: Uuid,
    number: String,
    from_location: String,
    to_location: String,
    date: NaiveDateTime,
    notes: Option<String>,
    created: NaiveDateTime,
    updated: NaiveDateTime,
    items: &[(DocItemInput, String, String)],
) -> Result<TransferDto, PosError> {
    let mut dtos = Vec::new();
    for (item, cost, price) in items {
        let row = sqlx::query(
            "SELECT id, created_at::text FROM transfer_items WHERE transfer_id = $1 AND product_id = $2 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .bind(item.product_id)
        .fetch_one(pool)
        .await
        .pe()?;
        let created: String = row.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        dtos.push(TransferItemDto {
            id: row.get("id"),
            transfer_id: id,
            product_id: item.product_id,
            quantity: item.quantity.clone(),
            cost_price: cost.clone(),
            price: price.clone(),
            created_at: iso_utc_z(created),
        });
    }
    Ok(TransferDto {
        id,
        number,
        from_location,
        to_location,
        transfer_date: iso_utc_z(date),
        status: "draft".to_string(),
        notes,
        created_at: iso_utc_z(created),
        updated_at: iso_utc_z(updated),
        items: dtos,
    })
}

/// Повний read transfer з БД.
async fn read_transfer(pool: &StorePool, id: Uuid) -> Result<TransferDto, PosError> {
    let row = sqlx::query(
        "SELECT number, from_location, to_location, transfer_date::text, status::text, notes, \
         created_at::text, updated_at::text FROM transfers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .pe()?;
    let Some(row) = row else {
        return Err(PosError::NotFound(format!(
            "Переміщення з ID '{id}' не знайдено"
        )));
    };
    let created: String = row.get("created_at");
    let updated: String = row.get("updated_at");
    let tdate: String = row.get("transfer_date");
    let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let updated = NaiveDateTime::parse_from_str(&updated, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let tdate = NaiveDateTime::parse_from_str(&tdate, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let items = sqlx::query(
        "SELECT id, transfer_id, product_id, quantity::text, cost_price::text, price::text, \
         created_at::text FROM transfer_items WHERE transfer_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .pe()?;
    let mut dtos = Vec::new();
    for it in items {
        let created: String = it.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        dtos.push(TransferItemDto {
            id: it.get("id"),
            transfer_id: it.get("transfer_id"),
            product_id: it.get("product_id"),
            quantity: it.get("quantity"),
            cost_price: it.get("cost_price"),
            price: it.get("price"),
            created_at: iso_utc_z(created),
        });
    }
    Ok(TransferDto {
        id,
        number: row.get("number"),
        from_location: row.get("from_location"),
        to_location: row.get("to_location"),
        transfer_date: iso_utc_z(tdate),
        status: row.get("status"),
        notes: row.get("notes"),
        created_at: iso_utc_z(created),
        updated_at: iso_utc_z(updated),
        items: dtos,
    })
}

// ─── Чеки v1: POST /api/v1/receipts (create_receipt + боргова семантика) ────

/// ID товару "Борг" (barcode: DEBT-PAYMENT) — константа Python v1.
const DEBT_PRODUCT_ID: &str = "c230fe32-78ef-4501-a21d-71467a668fc4";

/// Python `Decimal.quantize(..., ROUND_HALF_UP)` — value_objects/rounding.py.
fn round_amount_v1(s: &str, code: i64) -> Result<String, PosError> {
    use rust_decimal::prelude::*;
    use rust_decimal::RoundingStrategy;
    use std::str::FromStr;

    let d = Decimal::from_str(s)
        .map_err(|_| PosError::BadRequest(format!("Невалідне десяткове число: {s}")))?;
    let away = RoundingStrategy::MidpointAwayFromZero;
    let r = match code {
        1 => d.round_dp_with_strategy(2, away),
        10 => d.round_dp_with_strategy(1, away),
        50 => {
            let doubled = (d * Decimal::TWO).round_dp_with_strategy(0, away);
            (doubled / Decimal::TWO).round_dp_with_strategy(2, away)
        }
        100 => d.round_dp_with_strategy(0, away),
        500 => {
            let div = (d / Decimal::from(5)).round_dp_with_strategy(0, away);
            (div * Decimal::from(5)).round_dp_with_strategy(0, away)
        }
        _ => d.round_dp_with_strategy(2, away),
    };
    Ok(r.to_string())
}

/// Python `str(float)` — найкоротший round-trip (Rust Debug f64).
fn py_float_str(v: f64) -> String {
    format!("{v:?}")
}

/// SELECT value FROM system_settings WHERE key=$1 AND is_active=true.
async fn get_setting_v1(pool: &StorePool, key: &str) -> Result<Option<String>, PosError> {
    let row = sqlx::query("SELECT value FROM system_settings WHERE key = $1 AND is_active = true")
        .bind(key)
        .fetch_optional(pool)
        .await
        .pe()?;
    Ok(row.map(|r| r.get("value")))
}

/// Python `int(receipt_number.split("-")[-1])` з catch → 0.
fn last_seq_from_number(n: &str) -> i64 {
    n.rsplit('-')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

/// Python `max(Decimal("0"), sold - returned)` — повертає рядок як str(Decimal).
fn returnable_str(sold: &str, returned: &str) -> String {
    use rust_decimal::prelude::*;
    use std::str::FromStr;
    let s = Decimal::from_str(sold).unwrap_or_default();
    let r = Decimal::from_str(returned).unwrap_or_default();
    if s > r {
        (s - r).to_string()
    } else {
        Decimal::ZERO.to_string()
    }
}

/// Python str(datetime) Pydantic з UTC-маркером: "2026-08-09T14:19:31.489344Z" (або без .%f).
fn iso_naive_str(created: &str) -> String {
    // created::text з БД: "2026-08-09 14:19:31.489344" або "2026-08-09 14:19:31".
    let base = if let Some((date, time)) = created.split_once(' ') {
        format!("{date}T{time}")
    } else {
        created.to_string()
    };
    format!("{base}Z")
}

/// Нормалізація Option<String> часу з БД ("YYYY-MM-DD HH:MM:SS[.ffffff]") → iso_utc_z.
/// Непарсибельне значення повертається як є (не панікуємо).
fn normalize_utc_z(v: Option<String>) -> Option<String> {
    v.map(|raw| {
        NaiveDateTime::parse_from_str(&raw, "%Y-%m-%d %H:%M:%S%.f")
            .map(iso_utc_z)
            .unwrap_or(raw)
    })
}

/// Python `_calc_vat` — ПДВ = (price*qty*rate)/(1+rate), quantize 0.01 HALF_EVEN.
fn calc_vat_v1(price: &str, quantity: &str, tax_rate: &str) -> String {
    use rust_decimal::prelude::*;
    use rust_decimal::RoundingStrategy;
    use std::str::FromStr;
    let tr = Decimal::from_str(tax_rate).unwrap_or_default();
    if tr.is_zero() {
        return "0.00".to_string();
    }
    let price = Decimal::from_str(price).unwrap_or_default();
    let qty = Decimal::from_str(quantity).unwrap_or_default();
    let rate = tr / Decimal::from(100);
    let total = price * qty;
    let vat = (total * rate) / (Decimal::ONE + rate);
    vat.round_dp_with_strategy(2, RoundingStrategy::MidpointNearestEven)
        .to_string()
}

/// POST /api/v1/receipts — 1:1 `create_receipt` (app/api/v1/receipts.py:663).
#[allow(clippy::too_many_lines)]
async fn create_receipt_v1_impl(
    pool: &StorePool,
    input: &ReceiptV1CreateInput,
) -> Result<ReceiptV1Dto, PosError> {
    use rust_decimal::prelude::*;
    use std::str::FromStr;

    let debt_product = Uuid::parse_str(DEBT_PRODUCT_ID).expect("DEBT_PRODUCT_ID static");
    let is_debt_payment = input.debt_payment.is_some();

    // ─── Валідація та підготовка оплати боргу ─────────────────────────────
    let mut debtor_id = input.debtor_id;
    let mut items = input.items.clone();
    let mut debt_payment_debt: Option<String> = None; // total_debt::text боржника
    if let Some(dp) = &input.debt_payment {
        let row = sqlx::query("SELECT total_debt::text FROM debtors WHERE id = $1")
            .bind(dp.debtor_id)
            .fetch_optional(pool)
            .await
            .pe()?;
        let Some(row) = row else {
            return Err(PosError::NotFound(format!(
                "Боржника з ID '{}' не знайдено",
                dp.debtor_id
            )));
        };
        let current_debt: String = row.get("total_debt");
        let amount = Decimal::from_str(&dp.amount).map_err(|_| {
            PosError::BadRequest(format!("Невалідне десяткове число: {}", dp.amount))
        })?;
        let current = Decimal::from_str(&current_debt).unwrap_or_default();
        if amount > current {
            return Err(PosError::BadRequest(format!(
                "Сума оплати боргу ({}) перевищує поточний борг ({})",
                dp.amount, current_debt
            )));
        }
        // Якщо товару "Борг" немає серед items — додати автоматично.
        let has_debt_item = items.iter().any(|i| i.product_id == debt_product);
        if !has_debt_item {
            items.push(ReceiptV1ItemInput {
                product_id: debt_product,
                quantity: "1".to_string(),
                price: dp.amount.clone(),
                total: Some(dp.amount.clone()),
            });
        }
        debtor_id = Some(dp.debtor_id);
        debt_payment_debt = Some(current_debt);
    }

    // ─── Генерація номера чеку (RCPT-{YYYYMMDD}-{last+1:04d}) ─────────────
    let number = match &input.receipt_number {
        Some(n) if !n.is_empty() => n.clone(),
        _ => {
            let last =
                sqlx::query("SELECT receipt_number FROM receipts ORDER BY created_at DESC LIMIT 1")
                    .fetch_optional(pool)
                    .await
                    .pe()?;
            let last_num = match last {
                Some(r) => {
                    let n: String = r.get("receipt_number");
                    last_seq_from_number(&n)
                }
                None => 0,
            };
            let date = chrono::Local::now().format("%Y%m%d");
            format!("RCPT-{date}-{:04}", last_num + 1)
        }
    };

    let cashier_id = input.cashier_id.ok_or_else(|| {
        PosError::BadRequest("Відсутній ідентифікатор касира в токені".to_string())
    })?;

    // paid_amount: якщо не передано — повна оплата.
    let mut paid = input
        .paid_amount
        .clone()
        .unwrap_or_else(|| input.total_amount.clone());
    let paid_d = Decimal::from_str(&paid)
        .map_err(|_| PosError::BadRequest(format!("Невалідне десяткове число: {paid}")))?;
    if paid_d.is_sign_negative() {
        return Err(PosError::BadRequest(
            "Сума оплати (paid_amount) не може бути від'ємною".to_string(),
        ));
    }
    let mut total = input.total_amount.clone();

    // ─── Валідація кількості для повернень ────────────────────────────────
    if input.receipt_type == "return" {
        for item in &items {
            if item.product_id == debt_product {
                continue;
            }
            let sold: String = sqlx::query_scalar(
                "SELECT COALESCE(SUM(ri.quantity), 0)::text FROM receipt_items ri \
                 JOIN receipts r ON r.id = ri.receipt_id \
                 WHERE ri.product_id = $1 AND r.receipt_type = 'sale'",
            )
            .bind(item.product_id)
            .fetch_one(pool)
            .await
            .pe()?;
            let returned: String = sqlx::query_scalar(
                "SELECT COALESCE(SUM(ri.quantity), 0)::text FROM receipt_items ri \
                 JOIN receipts r ON r.id = ri.receipt_id \
                 WHERE ri.product_id = $1 AND r.receipt_type = 'return'",
            )
            .bind(item.product_id)
            .fetch_one(pool)
            .await
            .pe()?;
            let returnable = returnable_str(&sold, &returned);
            let qty = Decimal::from_str(&item.quantity).map_err(|_| {
                PosError::BadRequest(format!("Невалідне десяткове число: {}", item.quantity))
            })?;
            let rb = Decimal::from_str(&returnable).unwrap_or_default();
            if qty > rb {
                let name: String = sqlx::query_scalar("SELECT title FROM products WHERE id = $1")
                    .bind(item.product_id)
                    .fetch_optional(pool)
                    .await
                    .pe()?
                    .unwrap_or_else(|| item.product_id.to_string());
                return Err(PosError::BadRequest(format!(
                    "Товар '{}': можна повернути не більше {} од. (продано: {}, вже повернуто: {})",
                    name, returnable, returnable, 0
                )));
            }
        }
    }

    // ─── Здача (change) якщо paid > total ─────────────────────────────────
    let total_d = Decimal::from_str(&total)
        .map_err(|_| PosError::BadRequest(format!("Невалідне десяткове число: {total}")))?;
    let change: Option<String> = if paid_d > total_d {
        Some((paid_d - total_d).to_string())
    } else {
        None
    };

    // ─── Заокруглення суми чеку (price_rounding) ──────────────────────────
    let rounding_code: i64 = get_setting_v1(pool, "price_rounding")
        .await?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(1);
    if rounding_code != 1 {
        let rounded = round_amount_v1(&total, rounding_code)?;
        if paid_d == total_d {
            paid = rounded.clone();
        }
        total = rounded;
    }

    // ─── Отримуємо debtor (якщо вказано) ──────────────────────────────────
    let mut debtor_debt: Option<String> = None;
    if let Some(did) = debtor_id {
        if !is_debt_payment {
            let row = sqlx::query("SELECT total_debt::text FROM debtors WHERE id = $1")
                .bind(did)
                .fetch_optional(pool)
                .await
                .pe()?;
            let Some(row) = row else {
                return Err(PosError::NotFound(format!(
                    "Боржника з ID '{did}' не знайдено"
                )));
            };
            debtor_debt = Some(row.get("total_debt"));
        } else {
            debtor_debt = debt_payment_debt.clone();
        }
    }

    // ─── Створюємо чек ────────────────────────────────────────────────────
    let id = Uuid::new_v4();
    let change_bind = change.as_ref().filter(|c| {
        Decimal::from_str(c)
            .map(|v| v > Decimal::ZERO)
            .unwrap_or(false)
    });
    let mut tx = pool.begin().await.pe()?;
    let store_id = current_store_ctx()
        .map(|c| c.store_id)
        .ok_or_else(|| PosError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string()))?;
    sqlx::query(
        r#"
        INSERT INTO receipts (
            id, receipt_number, receipt_type, cashier_id, total_amount, paid_amount,
            change_amount, debtor_id, is_return, notes, payment_method,
            original_receipt_id, store_id, created_at
        ) VALUES (
            $1, $2, $3::receipt_type, $4, $5::numeric, $6::numeric, $7::numeric,
            $8, $9, $10, $11::receipt_payment_method, $12, $13,
            (now() AT TIME ZONE 'UTC')::timestamp
        )
        "#,
    )
    .bind(id)
    .bind(&number)
    .bind(&input.receipt_type)
    .bind(cashier_id)
    .bind(&total)
    .bind(paid.as_str())
    .bind(change_bind)
    .bind(debtor_id)
    .bind(input.is_return)
    .bind(input.notes.as_deref())
    .bind(input.payment_method.as_deref())
    .bind(input.original_receipt_id)
    .bind(store_id)
    .execute(&mut *tx)
    .await
    .pe()?;

    // ─── Позиції та оновлення залишків ────────────────────────────────────
    for item in &items {
        let item_total = match &item.total {
            Some(t) => t.clone(),
            None => {
                let q = Decimal::from_str(&item.quantity).unwrap_or_default();
                let p = Decimal::from_str(&item.price).unwrap_or_default();
                (q * p).to_string()
            }
        };

        let prod = sqlx::query("SELECT title, cost_price::text FROM products WHERE id = $1")
            .bind(item.product_id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?;

        // purchase_price = float(cost_price) (None якщо товар не знайдено).
        let (title, cost_price): (String, Option<String>) = match &prod {
            Some(r) => (
                r.try_get("title").unwrap_or_default(),
                r.try_get("cost_price").ok().flatten(),
            ),
            None => (String::new(), None),
        };

        // Python: item вставляється, потім update_stock (для не-DEBT).
        // Неіснуючий товар (не DEBT): SQLAlchemy autoflush при наступному
        // SELECT → FK violation receipt_items_product_id → 500 IntegrityError.
        let is_debt_item = item.product_id == debt_product;
        if prod.is_none() && !is_debt_item {
            return Err(PosError::Integrity(
                "insert or update on table \"receipt_items\" violates foreign key constraint \"receipt_items_product_id_fkey\"".to_string(),
            ));
        }

        sqlx::query(
            r#"
            INSERT INTO receipt_items (
                id, receipt_id, product_id, quantity, price, total, purchase_price,
                fiscal_quantity, store_id, created_at
            ) VALUES (
                $1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7::numeric, 0, $8,
                (now() AT TIME ZONE 'UTC')::timestamp
            )
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(item.product_id)
        .bind(&item.quantity)
        .bind(&item.price)
        .bind(&item_total)
        .bind(cost_price.as_deref())
        .bind(store_id)
        .execute(&mut *tx)
        .await
        .pe()?;

        // Оновлення залишку stock (Етап 3: per store, крім товару "Борг").
        if !is_debt_item {
            if input.receipt_type == "sale" {
                let qty = Decimal::from_str(&item.quantity).unwrap_or_default();
                // Python update_stock: if stock + (-qty) < 0 → 400.
                let res = sqlx::query(
                    "UPDATE stock SET quantity = quantity - $1::numeric, updated_at = now()
                     WHERE store_id = $2 AND product_id = $3 AND quantity >= $1::numeric",
                )
                .bind(&item.quantity)
                .bind(store_id)
                .bind(item.product_id)
                .execute(&mut *tx)
                .await
                .pe()?;
                if res.rows_affected() == 0 {
                    let stock: Option<String> = sqlx::query_scalar(
                        "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
                    )
                    .bind(store_id)
                    .bind(item.product_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .pe()?
                    .flatten();
                    let insufficient = match &stock {
                        Some(st_txt) => {
                            let st = Decimal::from_str(st_txt).unwrap_or_default();
                            st < qty
                        }
                        None => true,
                    };
                    if insufficient {
                        let allow = get_setting_v1(pool, "allow_negative_stock")
                            .await?
                            .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
                            .unwrap_or(false);
                        if !allow {
                            let stock_txt = stock.unwrap_or_else(|| "0".to_string());
                            return Err(PosError::BadRequest(format!(
                                "Недостатньо товару '{}' на складі. Доступно: {}, потрібно: {}",
                                title, stock_txt, item.quantity
                            )));
                        }
                        // allow_negative_stock=true → дозволяємо від'ємний залишок.
                        sqlx::query(
                            "UPDATE stock SET quantity = quantity - $1::numeric, updated_at = now()
                             WHERE store_id = $2 AND product_id = $3",
                        )
                        .bind(&item.quantity)
                        .bind(store_id)
                        .bind(item.product_id)
                        .execute(&mut *tx)
                        .await
                        .pe()?;
                    }
                }
            } else {
                sqlx::query(
                    "INSERT INTO stock (store_id, product_id, quantity, price, updated_at)
                     VALUES ($1, $2, $3::numeric, 0, now())
                     ON CONFLICT (store_id, product_id) DO UPDATE
                        SET quantity = stock.quantity + EXCLUDED.quantity, updated_at = now()",
                )
                .bind(store_id)
                .bind(item.product_id)
                .bind(&item.quantity)
                .execute(&mut *tx)
                .await
                .pe()?;
            }
        }
    }

    // ─── Логіка боргу ─────────────────────────────────────────────────────
    if is_debt_payment {
        let dp = input.debt_payment.as_ref().expect("is_debt_payment");
        // DebtorPayment (payment_method='cash' — оплата через касу).
        sqlx::query(
            r#"
            INSERT INTO debtor_payments (id, debtor_id, amount, payment_method, store_id, created_at)
            VALUES ($1, $2, $3::numeric, 'cash',
                    COALESCE(NULLIF(current_setting('app.store_id', true), '')::uuid, NULL),
                    (now() AT TIME ZONE 'UTC')::timestamp)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(dp.debtor_id)
        .bind(&dp.amount)
        .execute(&mut *tx)
        .await
        .pe()?;

        let current =
            Decimal::from_str(debt_payment_debt.as_deref().unwrap_or("0")).unwrap_or_default();
        let amount = Decimal::from_str(&dp.amount).unwrap_or_default();
        let new_debt = current - amount;
        if new_debt <= Decimal::ZERO {
            // Python float(total_debt) <= 0 → видалити боржника (каскад видалить payment).
            sqlx::query("DELETE FROM debtors WHERE id = $1")
                .bind(dp.debtor_id)
                .execute(&mut *tx)
                .await
                .pe()?;
        } else {
            sqlx::query("UPDATE debtors SET total_debt = $1::numeric WHERE id = $2")
                .bind(new_debt.to_string())
                .bind(dp.debtor_id)
                .execute(&mut *tx)
                .await
                .pe()?;
        }
    } else if let Some(debt_txt) = &debtor_debt {
        let paid_final = Decimal::from_str(&paid).unwrap_or_default();
        let total_final = Decimal::from_str(&total).unwrap_or_default();
        if paid_final < total_final {
            let debt_amount = total_final - paid_final;
            let current = Decimal::from_str(debt_txt).unwrap_or_default();
            let new_debt = current + debt_amount;
            let did = debtor_id.expect("debtor_debt — лише при debtor_id");
            if new_debt <= Decimal::ZERO {
                sqlx::query("DELETE FROM debtors WHERE id = $1")
                    .bind(did)
                    .execute(&mut *tx)
                    .await
                    .pe()?;
            } else {
                sqlx::query("UPDATE debtors SET total_debt = $1::numeric WHERE id = $2")
                    .bind(new_debt.to_string())
                    .bind(did)
                    .execute(&mut *tx)
                    .await
                    .pe()?;
            }
        }
    }

    tx.commit().await.pe()?;

    // ─── Відповідь (1:1 _fill_product_names_and_profit) ───────────────────
    let row = sqlx::query(
        r#"
        SELECT r.id, r.receipt_number, r.receipt_type::text, r.cashier_id,
               r.total_amount::text, r.paid_amount::text, r.change_amount::text,
               r.debtor_id, r.is_return, r.notes, r.payment_method::text,
               r.created_at::text, u.name AS cashier_name
        FROM receipts r
        LEFT JOIN users u ON u.id = r.cashier_id
        WHERE r.id = $1
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .pe()?;

    let item_rows = sqlx::query(
        r#"
        SELECT ri.id, ri.receipt_id, ri.product_id, p.title, p.barcode,
               ri.quantity::text, ri.price::text, ri.total::text,
               ri.purchase_price::text, ri.created_at::text,
               p.cost_price::text, p.tax_rate::text
        FROM receipt_items ri
        LEFT JOIN products p ON p.id = ri.product_id
        WHERE ri.receipt_id = $1
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .pe()?;

    let created: String = row.get("created_at");
    let mut total_profit = Decimal::ZERO;
    let mut total_vat = Decimal::ZERO;
    let mut items_dto = Vec::with_capacity(item_rows.len());
    // Відповідь Python — identity map (expire_on_commit=False): quantity/price/total —
    // ВХІДНІ значення, purchase_price = float(cost_price) (str(float)).
    for (idx, r) in item_rows.iter().enumerate() {
        let it = items
            .get(idx)
            .ok_or_else(|| PosError::Infrastructure("items/rows mismatch".to_string()))?;
        let cost_price: Option<String> = r.try_get("cost_price").ok().flatten();
        let title: Option<String> = r.try_get("title").ok().flatten();
        let barcode: Option<String> = r.try_get("barcode").ok().flatten();
        let item_created: String = r.get("created_at");

        // Прибуток (якщо purchase_price є): total - purchase_price*quantity.
        // Python: Decimal(str(item.total)) - Decimal(str(float(cost_price)))*Decimal(quantity).
        let item_total_in = match &it.total {
            Some(t) => t.clone(),
            None => {
                let q = Decimal::from_str(&it.quantity).unwrap_or_default();
                let p = Decimal::from_str(&it.price).unwrap_or_default();
                (q * p).to_string()
            }
        };
        if let Some(cost_txt) = &cost_price {
            let it_t = Decimal::from_str(&item_total_in).unwrap_or_default();
            let c = Decimal::from_str(&py_float_str(cost_txt.parse::<f64>().unwrap_or_default()))
                .unwrap_or_default();
            let q = Decimal::from_str(&it.quantity).unwrap_or_default();
            total_profit += it_t - c * q;
        }
        // ПДВ (якщо product є і tax_rate не None).
        let tax_rate: Option<String> = r.try_get("tax_rate").ok().flatten();
        let vat = match &tax_rate {
            Some(tr) => calc_vat_v1(&it.price, &it.quantity, tr),
            None => "0.00".to_string(),
        };
        let vat_d = Decimal::from_str(&vat).unwrap_or_default();
        total_vat += vat_d;

        // Python (емпірично): при >1 позиціях ПЕРША перечитується з БД
        // (SQLAlchemy selectinload + expire), решта — вхідні (identity map).
        // При 1 позиції — вхідні. DEBT — завжди вхідні.
        let from_db = items.len() > 1 && idx == 0;
        let qty_dto = if from_db {
            r.get::<String, _>("quantity")
        } else {
            it.quantity.clone()
        };
        let price_dto = if from_db {
            r.get::<String, _>("price")
        } else {
            it.price.clone()
        };
        let total_dto = if from_db {
            r.get::<String, _>("total")
        } else {
            item_total_in
        };
        let pp_dto = cost_price.as_ref().map(|c| {
            if from_db {
                c.clone()
            } else {
                py_float_str(c.parse::<f64>().unwrap_or_default())
            }
        });
        items_dto.push(ReceiptV1ItemDto {
            id: r.get("id"),
            receipt_id: r.get("receipt_id"),
            product_id: r.get("product_id"),
            product_name: title.unwrap_or_default(),
            product_barcode: barcode,
            quantity: qty_dto,
            price: price_dto,
            total: total_dto,
            purchase_price: pp_dto,
            profit: None,
            vat_amount: None,
            created_at: iso_naive_str(&item_created),
        });
    }

    // change_amount: Python передає None → ORM default 0.00 (asdecimal=False
    // → float 0.0) → "0.0"; при change>0 — Decimal(paid-total) як є.
    let change_dto = match &change {
        Some(c) => c.clone(),
        None => "0.0".to_string(),
    };
    let cashier_name: Option<String> = row.try_get("cashier_name").ok().flatten();
    Ok(ReceiptV1Dto {
        id: row.get("id"),
        receipt_number: row.get("receipt_number"),
        receipt_type: row.get("receipt_type"),
        cashier_id: row.get("cashier_id"),
        total_amount: total,
        paid_amount: Some(paid),
        change_amount: Some(change_dto),
        debtor_id,
        is_return: row.get("is_return"),
        notes: row.get("notes"),
        created_at: iso_naive_str(&created),
        items: items_dto,
        total_profit: serde_json::Value::String(py_float_str(
            total_profit.to_f64().unwrap_or_default(),
        )),
        vat_amount: serde_json::Value::String(py_float_str(total_vat.to_f64().unwrap_or_default())),
        cashier_name: cashier_name.unwrap_or_else(|| "Невідомо".to_string()),
        payment_method: input.payment_method.clone(),
    })
}

// ─── Чеки v1: LIST/GET/items (1:1 Python app/api/v1/receipts.py) ────────────

/// Побудова v1 item DTO з рядка items SELECT. Повертає (DTO, profit, vat).
/// - pp_fallback: Python _fill ставить float(cost_price) якщо purchase_price None
///   (GET/LIST чека); items-роут цього НЕ робить.
/// - items_vat: LIST ставить item.vat_amount = float(_vat_amount); GET/items — None.
#[allow(clippy::type_complexity)]
fn build_v1_item_from_row(
    r: &sqlx::postgres::PgRow,
    pp_fallback: bool,
    items_vat: bool,
) -> (
    ReceiptV1ItemDto,
    rust_decimal::Decimal,
    rust_decimal::Decimal,
) {
    use rust_decimal::prelude::*;
    use std::str::FromStr;
    let quantity: String = r.get("quantity");
    let price: String = r.get("price");
    let total: String = r.get("total");
    let cost_price: Option<String> = r.try_get("cost_price").ok().flatten();
    let db_pp: Option<String> = r.try_get("purchase_price").ok().flatten();
    let pp_txt = match (&db_pp, pp_fallback, &cost_price) {
        (Some(pp), _, _) => Some(pp.clone()),
        (None, true, Some(c)) => Some(py_float_str(c.parse::<f64>().unwrap_or_default())),
        _ => None,
    };
    let mut profit = Decimal::ZERO;
    if let Some(pp) = &pp_txt {
        let t = Decimal::from_str(&total).unwrap_or_default();
        let c = Decimal::from_str(pp).unwrap_or_default();
        let q = Decimal::from_str(&quantity).unwrap_or_default();
        profit = t - c * q;
    }
    let tax_rate: Option<String> = r.try_get("tax_rate").ok().flatten();
    let vat = match &tax_rate {
        Some(tr) => calc_vat_v1(&price, &quantity, tr),
        None => "0.00".to_string(),
    };
    let vat_d = Decimal::from_str(&vat).unwrap_or_default();
    let vat_dto = if items_vat {
        Some(serde_json::Value::from(vat_d.to_f64().unwrap_or_default()))
    } else {
        None
    };
    let title: Option<String> = r.try_get("title").ok().flatten();
    let barcode: Option<String> = r.try_get("barcode").ok().flatten();
    let created: String = r.get("created_at");
    let dto = ReceiptV1ItemDto {
        id: r.get("id"),
        receipt_id: r.get("receipt_id"),
        product_id: r.get("product_id"),
        product_name: title.unwrap_or_default(),
        product_barcode: barcode,
        quantity,
        price,
        total,
        purchase_price: pp_txt,
        profit: None,
        vat_amount: vat_dto,
        created_at: iso_naive_str(&created),
    };
    (dto, profit, vat_d)
}

/// Режим відповіді v1: GET — Decimal-рядки (Python ReceiptResponse),
/// LIST — float-числа (Python r_dict["total_profit"] = float).
#[derive(Clone, Copy, PartialEq)]
enum V1RespMode {
    Get,
    List,
}

/// Побудова повного v1 чека з рядка receipts + item_rows.
#[allow(clippy::type_complexity)]
fn build_v1_receipt_dto(
    row: sqlx::postgres::PgRow,
    item_rows: Vec<sqlx::postgres::PgRow>,
    mode: V1RespMode,
) -> ReceiptV1Dto {
    use rust_decimal::prelude::*;
    let mut total_profit = Decimal::ZERO;
    let mut total_vat = Decimal::ZERO;
    let mut items_dto = Vec::with_capacity(item_rows.len());
    for r in &item_rows {
        let (dto, profit, vat) = build_v1_item_from_row(r, true, mode == V1RespMode::List);
        total_profit += profit;
        total_vat += vat;
        items_dto.push(dto);
    }
    let change: Option<String> = row.try_get("change_amount").ok().flatten();
    let cashier_name: Option<String> = row.try_get("cashier_name").ok().flatten();
    ReceiptV1Dto {
        id: row.get("id"),
        receipt_number: row.get("receipt_number"),
        receipt_type: row.get("receipt_type"),
        cashier_id: row.get("cashier_id"),
        total_amount: row.get("total_amount"),
        paid_amount: row.try_get("paid_amount").ok().flatten(),
        change_amount: change,
        debtor_id: row.try_get("debtor_id").ok().flatten(),
        is_return: row.get("is_return"),
        notes: row.try_get("notes").ok().flatten(),
        created_at: iso_naive_str(&row.get::<String, _>("created_at")),
        items: items_dto,
        total_profit: match mode {
            V1RespMode::Get => {
                serde_json::Value::String(py_float_str(total_profit.to_f64().unwrap_or_default()))
            }
            V1RespMode::List => serde_json::Value::from(total_profit.to_f64().unwrap_or_default()),
        },
        vat_amount: match mode {
            V1RespMode::Get => {
                serde_json::Value::String(py_float_str(total_vat.to_f64().unwrap_or_default()))
            }
            V1RespMode::List => serde_json::Value::from(total_vat.to_f64().unwrap_or_default()),
        },
        cashier_name: cashier_name.unwrap_or_else(|| "Невідомо".to_string()),
        payment_method: row.try_get("payment_method").ok().flatten(),
    }
}

const V1_ITEMS_SELECT: &str = r#"
    SELECT ri.id, ri.receipt_id, ri.product_id, p.title, p.barcode,
           ri.quantity::text, ri.price::text, ri.total::text,
           ri.purchase_price::text, ri.created_at::text,
           p.cost_price::text, p.tax_rate::text
    FROM receipt_items ri
    LEFT JOIN products p ON p.id = ri.product_id
"#;

/// GET /api/v1/receipts/{id} — 1:1 Python get_receipt.
async fn get_receipt_v1_impl(pool: &StorePool, id: Uuid) -> Result<ReceiptV1Dto, PosError> {
    let row = sqlx::query(
        r#"
        SELECT r.id, r.receipt_number, r.receipt_type::text, r.cashier_id,
               r.total_amount::text, r.paid_amount::text, r.change_amount::text,
               r.debtor_id, r.is_return, r.notes, r.created_at::text,
               r.payment_method::text, u.name AS cashier_name
        FROM receipts r
        LEFT JOIN users u ON u.id = r.cashier_id
        WHERE r.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .pe()?;
    let Some(row) = row else {
        return Err(PosError::NotFound(format!("Чек з ID '{id}' не знайдено")));
    };
    let item_rows = sqlx::query(&format!("{V1_ITEMS_SELECT} WHERE ri.receipt_id = $1"))
        .bind(id)
        .fetch_all(pool)
        .await
        .pe()?;
    Ok(build_v1_receipt_dto(row, item_rows, V1RespMode::Get))
}

/// GET /api/v1/receipts — 1:1 Python list_receipts (фільтри + пагінація).
async fn list_receipts_v1_impl(
    pool: &StorePool,
    q: &ReceiptV1ListQuery,
) -> Result<ReceiptV1ListDto, PosError> {
    // Значення фільтрів валідовані парсерами (Uuid/enum/NaiveDateTime) — format! безпечний.
    let mut conds: Vec<String> = Vec::new();
    if let Some(cid) = q.cashier_id {
        conds.push(format!("cashier_id = '{cid}'"));
    }
    if let Some(rt) = &q.receipt_type {
        if rt == "sale" || rt == "return" {
            conds.push(format!("receipt_type = '{rt}'"));
        }
    }
    if let Some(df) = q.date_from {
        conds.push(format!(
            "created_at >= '{}'",
            df.format("%Y-%m-%d %H:%M:%S%.f")
        ));
    }
    if let Some(dt) = q.date_to {
        conds.push(format!(
            "created_at <= '{}'",
            dt.format("%Y-%m-%d %H:%M:%S%.f")
        ));
    }
    if let Some(pm) = &q.payment_method {
        if matches!(pm.as_str(), "cash" | "card" | "mixed") {
            conds.push(format!("payment_method = '{pm}'"));
        }
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };

    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM receipts{where_sql}"))
        .fetch_one(pool)
        .await
        .pe()?;
    let offset = (q.page - 1) * q.size;
    let rows = sqlx::query(&format!(
        r#"
        SELECT r.id, r.receipt_number, r.receipt_type::text, r.cashier_id,
               r.total_amount::text, r.paid_amount::text, r.change_amount::text,
               r.debtor_id, r.is_return, r.notes, r.created_at::text,
               r.payment_method::text, u.name AS cashier_name
        FROM receipts r
        LEFT JOIN users u ON u.id = r.cashier_id
        {where_sql}
        ORDER BY r.created_at DESC
        LIMIT {size} OFFSET {offset}
        "#,
        size = q.size,
        offset = offset,
    ))
    .fetch_all(pool)
    .await
    .pe()?;

    let mut items_map: std::collections::HashMap<Uuid, Vec<sqlx::postgres::PgRow>> =
        std::collections::HashMap::new();
    if !rows.is_empty() {
        let ids: Vec<String> = rows
            .iter()
            .map(|r| r.get::<Uuid, _>("id").to_string())
            .collect();
        let item_rows = sqlx::query(&format!(
            "{V1_ITEMS_SELECT} WHERE ri.receipt_id = ANY('{{{}}}'::uuid[])",
            ids.join(",")
        ))
        .fetch_all(pool)
        .await
        .pe()?;
        for r in item_rows {
            items_map.entry(r.get("receipt_id")).or_default().push(r);
        }
    }

    let items = rows
        .into_iter()
        .map(|row| {
            let id: Uuid = row.get("id");
            let it_rows = items_map.remove(&id).unwrap_or_default();
            build_v1_receipt_dto(row, it_rows, V1RespMode::List)
        })
        .collect();

    let pages = if total > 0 {
        (total + q.size - 1) / q.size
    } else {
        1
    };
    Ok(ReceiptV1ListDto {
        items,
        total,
        page: q.page,
        page_size: q.size,
        pages: pages.max(1),
    })
}

/// GET /api/v1/receipts/{id}/items — 1:1 Python get_receipt_items.
async fn receipt_items_v1_impl(pool: &StorePool, id: Uuid) -> Result<Vec<ReceiptV1ItemDto>, PosError> {
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM receipts WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .pe()?;
    if exists.is_none() {
        return Err(PosError::NotFound(format!("Чек з ID '{id}' не знайдено")));
    }
    let rows = sqlx::query(&format!(
        "{V1_ITEMS_SELECT} WHERE ri.receipt_id = $1 ORDER BY ri.created_at"
    ))
    .bind(id)
    .fetch_all(pool)
    .await
    .pe()?;
    Ok(rows
        .iter()
        .map(|r| build_v1_item_from_row(r, false, false).0)
        .collect())
}

/// GET /api/v1/receipts/search — 1:1 Python v1 search_receipts.
/// Відмінності від v2: total = count БЕЗ DISTINCT (Python count(r.id) з JOIN —
/// дублікати позицій), total_amount — Decimal-рядок ("120.00").
async fn search_receipts_v1_impl(
    pool: &StorePool,
    q: &ReceiptSearchQuery,
) -> Result<ReceiptV1SearchDto, PosError> {
    let rtype = q.receipt_type.as_deref().unwrap_or("sale");
    let mut count_sql = format!(
        "SELECT count(r.id) FROM receipts r \
         LEFT JOIN receipt_items ri ON ri.receipt_id = r.id \
         LEFT JOIN products p ON p.id = ri.product_id \
         WHERE r.receipt_type = '{}'",
        rtype
    );
    let mut base = format!(
        "SELECT r.id, r.receipt_number, r.receipt_type::text, r.total_amount::text, \
         r.created_at::text, u.name AS cashier_name \
         FROM receipts r \
         LEFT JOIN users u ON u.id = r.cashier_id \
         LEFT JOIN receipt_items ri ON ri.receipt_id = r.id \
         LEFT JOIN products p ON p.id = ri.product_id \
         WHERE r.receipt_type = '{}'",
        rtype
    );
    // Python додає JOIN тільки якщо q непорожній; у нас JOIN завжди — але
    // фільтр (number ILIKE OR title ILIKE) додається тільки при q.
    let mut binds: Vec<String> = Vec::new();
    if let Some(dt) = q.date_from {
        binds.push(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string());
        let i = binds.len();
        count_sql.push_str(&format!(" AND r.created_at >= ${i}::timestamp"));
        base.push_str(&format!(" AND r.created_at >= ${i}::timestamp"));
    }
    if let Some(dt) = q.date_to {
        binds.push(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string());
        let i = binds.len();
        count_sql.push_str(&format!(" AND r.created_at <= ${i}::timestamp"));
        base.push_str(&format!(" AND r.created_at <= ${i}::timestamp"));
    }
    let qq = q.q.trim();
    if !qq.is_empty() {
        binds.push(format!("%{qq}%"));
        let i = binds.len();
        count_sql.push_str(&format!(
            " AND (r.receipt_number ILIKE ${i} OR p.title ILIKE ${i})"
        ));
        base.push_str(&format!(
            " AND (r.receipt_number ILIKE ${i} OR p.title ILIKE ${i})"
        ));
    }
    // Python: count БЕЗ distinct() → з дублікатами рядків JOIN (баг 1:1).
    let total: i64 = {
        let mut cq = sqlx::query(&count_sql);
        for b in &binds {
            cq = cq.bind(b);
        }
        cq.fetch_one(pool).await.pe()?.get("count")
    };
    let offset = (q.page - 1) * q.size;
    base.push_str(
        " GROUP BY r.id, r.receipt_number, r.receipt_type, r.total_amount, r.created_at, u.name",
    );
    base.push_str(&format!(
        " ORDER BY r.created_at DESC LIMIT {} OFFSET {}",
        q.size, offset
    ));
    let mut qr = sqlx::query(&base);
    for b in &binds {
        qr = qr.bind(b);
    }
    let rows = qr.fetch_all(pool).await.pe()?;
    let mut items = Vec::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let created: String = row.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let item_count: i64 =
            sqlx::query("SELECT count(*) FROM receipt_items WHERE receipt_id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .pe()?
                .get("count");
        items.push(ReceiptV1SearchItemDto {
            id,
            receipt_number: row.get("receipt_number"),
            receipt_type: row.get("receipt_type"),
            total_amount: row
                .get::<Option<String>, _>("total_amount")
                .unwrap_or_else(|| "0".to_string()),
            created_at: Some(iso_utc_z(created)),
            cashier_name: row
                .get::<Option<String>, _>("cashier_name")
                .unwrap_or_default(),
            items_count: item_count,
        });
    }
    let pages = if total > 0 {
        (total + q.size - 1) / q.size
    } else {
        1
    };
    Ok(ReceiptV1SearchDto {
        items,
        total,
        page: q.page,
        page_size: q.size,
        pages,
    })
}

// ─── impl PosService ────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl PosService for SqlxPos {
    // ── Чеки v2 ─────────────────────────────────────────────────────────────
    async fn create_sale_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        create_receipt_impl(&self.pool, input, "sale").await
    }

    async fn create_return_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        create_receipt_impl(&self.pool, input, "return").await
    }

    async fn create_receipt_v1(
        &self,
        input: &ReceiptV1CreateInput,
    ) -> Result<ReceiptV1Dto, PosError> {
        create_receipt_v1_impl(&self.pool, input).await
    }

    async fn get_receipt(&self, id: Uuid) -> Result<ReceiptDto, PosError> {
        read_receipt_dto(&self.pool, id)
            .await?
            .ok_or_else(|| PosError::NotFound(format!("Чек з ID '{id}' не знайдено")))
    }

    async fn list_receipts(&self, q: &ReceiptListQuery) -> Result<ReceiptListDto, PosError> {
        list_receipts_impl(&self.pool, q).await
    }

    async fn today_stats(&self) -> Result<ReceiptStatsDto, PosError> {
        today_stats_impl(&self.pool).await
    }

    async fn search_receipts(&self, q: &ReceiptSearchQuery) -> Result<ReceiptSearchDto, PosError> {
        search_receipts_impl(&self.pool, q).await
    }

    async fn recent_sales_by_product(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ProductRecentSalesDto>, PosError> {
        let items = recent_sales_impl(&self.pool, query, limit).await?;
        if items.is_empty() {
            return Err(PosError::NotFound(format!(
                "Товарів за запитом '{query}' не знайдено. Спробуйте ввести штрих-код або назву товару"
            )));
        }
        Ok(items)
    }

    async fn returnable_quantity(&self, product_id: Uuid) -> Result<ReturnableQtyDto, PosError> {
        let exists: Option<i64> = sqlx::query("SELECT 1 FROM products WHERE id = $1")
            .bind(product_id)
            .fetch_optional(&self.pool)
            .await
            .pe()?
            .map(|_| 1);
        if exists.is_none() {
            return Err(PosError::NotFound(format!(
                "Товар з ID '{product_id}' не знайдено"
            )));
        }
        let (sold, returned) = sold_returned_totals(&self.pool, product_id).await?;
        Ok(ReturnableQtyDto {
            product_id: product_id.to_string(),
            total_sold: sold,
            total_returned: returned,
            returnable: (sold - returned).max(0.0),
        })
    }

    async fn list_receipts_v1(&self, q: &ReceiptV1ListQuery) -> Result<ReceiptV1ListDto, PosError> {
        list_receipts_v1_impl(&self.pool, q).await
    }

    async fn get_receipt_v1(&self, id: Uuid) -> Result<ReceiptV1Dto, PosError> {
        get_receipt_v1_impl(&self.pool, id).await
    }

    async fn receipt_items_v1(&self, receipt_id: Uuid) -> Result<Vec<ReceiptV1ItemDto>, PosError> {
        receipt_items_v1_impl(&self.pool, receipt_id).await
    }

    async fn search_receipts_v1(
        &self,
        q: &ReceiptSearchQuery,
    ) -> Result<ReceiptV1SearchDto, PosError> {
        search_receipts_v1_impl(&self.pool, q).await
    }

    async fn receipt_items(&self, receipt_id: Uuid) -> Result<Vec<ReceiptItemDetailDto>, PosError> {
        let exists: Option<i64> = sqlx::query("SELECT 1 FROM receipts WHERE id = $1")
            .bind(receipt_id)
            .fetch_optional(&self.pool)
            .await
            .pe()?
            .map(|_| 1);
        if exists.is_none() {
            return Err(PosError::NotFound(format!(
                "Чек з ID '{receipt_id}' не знайдено"
            )));
        }
        fetch_receipt_items_detail(&self.pool, receipt_id).await
    }

    // ── Робочі сесії ────────────────────────────────────────────────────────
    async fn my_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<MySessionsDto, PosError> {
        my_sessions_impl(&self.pool, user_id, month, year).await
    }

    async fn work_report(&self, month: i64, year: i64) -> Result<WorkReportDto, PosError> {
        work_report_impl(&self.pool, month, year).await
    }

    async fn user_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<UserSessionsDto, PosError> {
        user_sessions_impl(&self.pool, user_id, month, year).await
    }

    // ── Списання ────────────────────────────────────────────────────────────
    async fn list_write_offs(&self, page: i64, size: i64) -> Result<WriteOffListDto, PosError> {
        let total: i64 = sqlx::query("SELECT count(*) FROM write_offs")
            .fetch_one(&self.pool)
            .await
            .pe()?
            .get("count");
        let offset = (page - 1) * size;
        let rows =
            sqlx::query("SELECT id FROM write_offs ORDER BY created_at DESC LIMIT $1 OFFSET $2")
                .bind(size)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .pe()?;
        let mut items = Vec::new();
        for r in rows {
            items.push(read_write_off(&self.pool, r.get("id")).await?);
        }
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        Ok(WriteOffListDto {
            items,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn get_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError> {
        read_write_off(&self.pool, id).await
    }

    async fn create_write_off(&self, input: &WriteOffCreateInput) -> Result<WriteOffDto, PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let number = match &input.number {
            Some(n) if !n.is_empty() => n.clone(),
            _ => next_doc_number(&mut tx, "write_offs", "СП").await?,
        };
        let id = Uuid::new_v4();
        let mut total_cents: i128 = 0;
        for item in &input.items {
            let qty = parse_scaled3(&item.quantity).unwrap_or(0) as i128;
            let (_cost, price) = resolve_item_prices(&mut tx, item).await?;
            let prc = parse_scaled2(&price).unwrap_or(0) as i128;
            total_cents += qty * prc / 1000;
        }
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| PosError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        let row = sqlx::query(
            r#"
            INSERT INTO write_offs (id, number, reason, write_off_date, notes, created_by_id,
                status, total_amount, store_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, 'draft', $7::numeric, $8,
                (now() AT TIME ZONE 'UTC')::timestamp, (now() AT TIME ZONE 'UTC')::timestamp)
            RETURNING created_at::text, updated_at::text
            "#,
        )
        .bind(id)
        .bind(&number)
        .bind(&input.reason)
        .bind(input.write_off_date)
        .bind(input.notes.as_deref())
        .bind(input.created_by)
        .bind(dec2(total_cents as i64))
        .bind(store_id)
        .fetch_one(&mut *tx)
        .await
        .pe()?;
        let created: String = row.get("created_at");
        let updated: String = row.get("updated_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let updated = NaiveDateTime::parse_from_str(&updated, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());

        let mut resolved = Vec::new();
        for item in &input.items {
            let (cost, price) = resolve_item_prices(&mut tx, item).await?;
            let qty = parse_scaled3(&item.quantity).unwrap_or(0);
            sqlx::query(
                r#"
                INSERT INTO write_off_items (id, write_off_id, product_id, quantity, cost_price,
                    price, store_id, created_at)
                VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7,
                    (now() AT TIME ZONE 'UTC')::timestamp)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(item.product_id)
            .bind(dec3(qty))
            .bind(dec2(parse_scaled2(&cost).unwrap_or(0)))
            .bind(dec2(parse_scaled2(&price).unwrap_or(0)))
            .bind(store_id)
            .execute(&mut *tx)
            .await
            .pe()?;
            resolved.push((item.clone(), cost, price));
        }
        // Чернетка (draft): залишки не змінюються. Проведення — confirm_write_off.
        tx.commit().await.pe()?;
        build_write_off_create(
            &self.pool,
            id,
            number,
            input.reason.clone(),
            input.write_off_date,
            input.notes.clone(),
            created,
            updated,
            &resolved,
        )
        .await
    }

    async fn update_write_off(
        &self,
        id: Uuid,
        input: &WriteOffUpdateInput,
    ) -> Result<WriteOffDto, PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| PosError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        let row = sqlx::query("SELECT id FROM write_offs WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?;
        if row.is_none() {
            return Err(PosError::NotFound(format!(
                "Списання з ID '{id}' не знайдено"
            )));
        }
        if let Some(Some(n)) = &input.number {
            sqlx::query("UPDATE write_offs SET number = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(n).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(r) = &input.reason {
            sqlx::query("UPDATE write_offs SET reason = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(r).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(d) = input.write_off_date {
            sqlx::query("UPDATE write_offs SET write_off_date = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(d).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(n) = &input.notes {
            sqlx::query("UPDATE write_offs SET notes = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(n.as_deref()).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(items) = &input.items {
            sqlx::query("DELETE FROM write_off_items WHERE write_off_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .pe()?;
            let mut total_cents: i128 = 0;
            for item in items {
                let (cost, price) = resolve_item_prices(&mut tx, item).await?;
                let qty = parse_scaled3(&item.quantity).unwrap_or(0);
                let prc = parse_scaled2(&price).unwrap_or(0) as i128;
                total_cents += parse_scaled3(&item.quantity).unwrap_or(0) as i128 * prc / 1000;
                sqlx::query(
                    r#"
                    INSERT INTO write_off_items (id, write_off_id, product_id, quantity, cost_price,
                        price, store_id, created_at)
                    VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7,
                        (now() AT TIME ZONE 'UTC')::timestamp)
                    "#,
                )
                .bind(Uuid::new_v4())
                .bind(id)
                .bind(item.product_id)
                .bind(dec3(qty))
                .bind(dec2(parse_scaled2(&cost).unwrap_or(0)))
                .bind(dec2(parse_scaled2(&price).unwrap_or(0)))
                .bind(store_id)
                .execute(&mut *tx)
                .await
                .pe()?;
            }
            sqlx::query("UPDATE write_offs SET total_amount = $1::numeric, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(dec2(total_cents as i64))
                .bind(id)
                .execute(&mut *tx)
                .await
                .pe()?;
        }
        tx.commit().await.pe()?;
        read_write_off(&self.pool, id).await
    }

    async fn delete_write_off(&self, id: Uuid) -> Result<(), PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let exists = sqlx::query("SELECT 1 FROM write_offs WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?
            .is_some();
        if !exists {
            return Err(PosError::NotFound(format!(
                "Списання з ID '{id}' не знайдено"
            )));
        }
        sqlx::query("DELETE FROM write_offs WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .pe()?;
        tx.commit().await.pe()?;
        Ok(())
    }

    async fn confirm_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let row = sqlx::query("SELECT status, store_id FROM write_offs WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?;
        let Some(row) = row else {
            return Err(PosError::NotFound(format!(
                "Списання з ID '{id}' не знайдено"
            )));
        };
        let store_id: Option<Uuid> = row.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            PosError::BadRequest(format!("Списання '{id}' не прив'язане до точки"))
        })?;
        // Ідемпотентність: вже проведений документ не зменшує залишки повторно.
        if row.get::<String, _>("status") == "confirmed" {
            tx.commit().await.pe()?;
            return read_write_off(&self.pool, id).await;
        }
        let items = sqlx::query(
            "SELECT product_id, quantity::text FROM write_off_items WHERE write_off_id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .pe()?;
        for it in items {
            let qty = parse_scaled3(&it.get::<String, _>("quantity")).unwrap_or(0);
            sqlx::query(
                "UPDATE stock SET quantity = quantity - $1::numeric, updated_at = now()
                 WHERE store_id = $2 AND product_id = $3",
            )
            .bind(dec3(qty))
            .bind(store_id)
            .bind(it.get::<Uuid, _>("product_id"))
            .execute(&mut *tx)
            .await
            .pe()?;
        }
        sqlx::query("UPDATE write_offs SET status = 'confirmed', updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .pe()?;
        tx.commit().await.pe()?;
        read_write_off(&self.pool, id).await
    }

    // ── Довідник причин списання ────────────────────────────────────────────
    async fn list_write_off_reasons(&self) -> Result<WriteOffReasonsListDto, PosError> {
        let rows = sqlx::query(
            "SELECT id, name, is_active, created_at::text FROM write_off_reasons              WHERE is_active = true ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .pe()?;
        let mut items = Vec::new();
        for r in rows {
            let created: String = r.get("created_at");
            let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
                .unwrap_or_else(|_| Utc::now().naive_utc());
            items.push(WriteOffReasonItem {
                id: r.get("id"),
                name: r.get("name"),
                is_active: r.get("is_active"),
                created_at: iso_utc_z(created),
            });
        }
        let total = items.len() as i64;
        Ok(WriteOffReasonsListDto { items, total })
    }

    async fn create_write_off_reason(&self, name: &str) -> Result<WriteOffReasonItem, PosError> {
        // 409: дублікат назви (case-insensitive) — як Python.
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM write_off_reasons WHERE lower(name) = lower($1))",
        )
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .pe()?;
        if exists {
            return Err(PosError::Conflict(format!("Причина «{name}» вже існує")));
        }
        let row = sqlx::query(
            "INSERT INTO write_off_reasons (id, name) VALUES (gen_random_uuid(), $1)              RETURNING id, name, is_active, created_at::text",
        )
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .pe()?;
        let created: String = row.get("created_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        Ok(WriteOffReasonItem {
            id: row.get("id"),
            name: row.get("name"),
            is_active: row.get("is_active"),
            created_at: iso_utc_z(created),
        })
    }

    // ── Переміщення ─────────────────────────────────────────────────────────
    async fn list_transfers(&self, page: i64, size: i64) -> Result<TransferListDto, PosError> {
        let total: i64 = sqlx::query("SELECT count(*) FROM transfers")
            .fetch_one(&self.pool)
            .await
            .pe()?
            .get("count");
        let offset = (page - 1) * size;
        let rows =
            sqlx::query("SELECT id FROM transfers ORDER BY created_at DESC LIMIT $1 OFFSET $2")
                .bind(size)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .pe()?;
        let mut items = Vec::new();
        for r in rows {
            items.push(read_transfer(&self.pool, r.get("id")).await?);
        }
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        Ok(TransferListDto {
            items,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn get_transfer(&self, id: Uuid) -> Result<TransferDto, PosError> {
        read_transfer(&self.pool, id).await
    }

    async fn create_transfer(&self, input: &TransferCreateInput) -> Result<TransferDto, PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| PosError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        let number = match &input.number {
            Some(n) if !n.is_empty() => n.clone(),
            _ => next_doc_number(&mut tx, "transfers", "ПМ").await?,
        };
        let id = Uuid::new_v4();
        let row = sqlx::query(
            r#"
            INSERT INTO transfers (id, number, from_location, to_location, transfer_date, status,
                notes, created_by_id, store_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, 'draft', $6, $7, $8,
                (now() AT TIME ZONE 'UTC')::timestamp, (now() AT TIME ZONE 'UTC')::timestamp)
            RETURNING created_at::text, updated_at::text
            "#,
        )
        .bind(id)
        .bind(&number)
        .bind(&input.from_location)
        .bind(&input.to_location)
        .bind(input.transfer_date)
        .bind(input.notes.as_deref())
        .bind(input.created_by)
        .bind(store_id)
        .fetch_one(&mut *tx)
        .await
        .pe()?;
        let created: String = row.get("created_at");
        let updated: String = row.get("updated_at");
        let created = NaiveDateTime::parse_from_str(&created, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let updated = NaiveDateTime::parse_from_str(&updated, "%Y-%m-%d %H:%M:%S%.f")
            .unwrap_or_else(|_| Utc::now().naive_utc());
        let mut resolved = Vec::new();
        for item in &input.items {
            let (cost, price) = resolve_item_prices(&mut tx, item).await?;
            let qty = parse_scaled3(&item.quantity).unwrap_or(0);
            sqlx::query(
                r#"
                INSERT INTO transfer_items (id, transfer_id, product_id, quantity, cost_price, price, store_id, created_at)
                VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7,
                    (now() AT TIME ZONE 'UTC')::timestamp)
                "#,
            )
            .bind(Uuid::new_v4()).bind(id).bind(item.product_id).bind(dec3(qty))
            .bind(dec2(parse_scaled2(&cost).unwrap_or(0))).bind(dec2(parse_scaled2(&price).unwrap_or(0)))
            .bind(store_id)
            .execute(&mut *tx).await.pe()?;
            resolved.push((item.clone(), cost, price));
        }
        tx.commit().await.pe()?;
        build_transfer_create(
            &self.pool,
            id,
            number,
            input.from_location.clone(),
            input.to_location.clone(),
            input.transfer_date,
            input.notes.clone(),
            created,
            updated,
            &resolved,
        )
        .await
    }

    async fn update_transfer(
        &self,
        id: Uuid,
        input: &TransferUpdateInput,
    ) -> Result<TransferDto, PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let store_id = current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| PosError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string()))?;
        let row = sqlx::query("SELECT status::text FROM transfers WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?;
        let Some(row) = row else {
            return Err(PosError::NotFound(format!(
                "Переміщення з ID '{id}' не знайдено"
            )));
        };
        let status: String = row.get("status");
        if status != "draft" {
            return Err(PosError::BadRequest(
                "Можна редагувати тільки чернетки".to_string(),
            ));
        }
        if let Some(Some(n)) = &input.number {
            sqlx::query("UPDATE transfers SET number = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(n).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(f) = &input.from_location {
            sqlx::query("UPDATE transfers SET from_location = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(f).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(t) = &input.to_location {
            sqlx::query("UPDATE transfers SET to_location = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(t).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(d) = input.transfer_date {
            sqlx::query("UPDATE transfers SET transfer_date = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(d).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(n) = &input.notes {
            sqlx::query("UPDATE transfers SET notes = $1, updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $2")
                .bind(n.as_deref()).bind(id).execute(&mut *tx).await.pe()?;
        }
        if let Some(items) = &input.items {
            sqlx::query("DELETE FROM transfer_items WHERE transfer_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .pe()?;
            for item in items {
                let (cost, price) = resolve_item_prices(&mut tx, item).await?;
                let qty = parse_scaled3(&item.quantity).unwrap_or(0);
                sqlx::query(
                    r#"
                    INSERT INTO transfer_items (id, transfer_id, product_id, quantity, cost_price, price, store_id, created_at)
                    VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7,
                        (now() AT TIME ZONE 'UTC')::timestamp)
                    "#,
                )
                .bind(Uuid::new_v4()).bind(id).bind(item.product_id).bind(dec3(qty))
                .bind(dec2(parse_scaled2(&cost).unwrap_or(0))).bind(dec2(parse_scaled2(&price).unwrap_or(0)))
                .bind(store_id)
                .execute(&mut *tx).await.pe()?;
            }
        }
        tx.commit().await.pe()?;
        read_transfer(&self.pool, id).await
    }

    async fn delete_transfer(&self, id: Uuid) -> Result<(), PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let row = sqlx::query("SELECT status::text FROM transfers WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?;
        let Some(row) = row else {
            return Err(PosError::NotFound(format!(
                "Переміщення з ID '{id}' не знайдено"
            )));
        };
        let status: String = row.get("status");
        if status != "draft" {
            return Err(PosError::BadRequest(
                "Можна видалити тільки чернетку".to_string(),
            ));
        }
        sqlx::query("DELETE FROM transfers WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .pe()?;
        tx.commit().await.pe()?;
        Ok(())
    }

    async fn confirm_transfer(&self, id: Uuid, status: &str) -> Result<TransferDto, PosError> {
        let mut tx = self.pool.begin().await.pe()?;
        let row = sqlx::query("SELECT status::text, store_id FROM transfers WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .pe()?;
        let Some(row) = row else {
            return Err(PosError::NotFound(format!(
                "Переміщення з ID '{id}' не знайдено"
            )));
        };
        let store_id: Option<Uuid> = row.try_get("store_id").ok().flatten();
        let store_id = store_id.ok_or_else(|| {
            PosError::BadRequest(format!("Переміщення '{id}' не прив'язане до точки"))
        })?;
        let cur: String = row.get("status");
        match status {
            "confirmed" => {
                if cur != "draft" {
                    return Err(PosError::BadRequest(format!(
                        "Переміщення вже має статус '{cur}'"
                    )));
                }
                let items = sqlx::query(
                    "SELECT product_id, quantity::text FROM transfer_items WHERE transfer_id = $1",
                )
                .bind(id)
                .fetch_all(&mut *tx)
                .await
                .pe()?;
                for it in items {
                    let qty = parse_scaled3(&it.get::<String, _>("quantity")).unwrap_or(0);
                    sqlx::query(
                        "UPDATE stock SET quantity = quantity - $1::numeric, updated_at = now()
                         WHERE store_id = $2 AND product_id = $3",
                    )
                    .bind(dec3(qty))
                    .bind(store_id)
                    .bind(it.get::<Uuid, _>("product_id"))
                    .execute(&mut *tx)
                    .await
                    .pe()?;
                }
                sqlx::query("UPDATE transfers SET status = 'confirmed', updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1")
                    .bind(id).execute(&mut *tx).await.pe()?;
            }
            "cancelled" => {
                if cur != "confirmed" {
                    return Err(PosError::BadRequest(
                        "Скасувати можна лише підтверджене переміщення".to_string(),
                    ));
                }
                let items = sqlx::query(
                    "SELECT product_id, quantity::text FROM transfer_items WHERE transfer_id = $1",
                )
                .bind(id)
                .fetch_all(&mut *tx)
                .await
                .pe()?;
                for it in items {
                    let qty = parse_scaled3(&it.get::<String, _>("quantity")).unwrap_or(0);
                    sqlx::query(
                        "UPDATE stock SET quantity = quantity + $1::numeric, updated_at = now()
                         WHERE store_id = $2 AND product_id = $3",
                    )
                    .bind(dec3(qty))
                    .bind(store_id)
                    .bind(it.get::<Uuid, _>("product_id"))
                    .execute(&mut *tx)
                    .await
                    .pe()?;
                }
                sqlx::query("UPDATE transfers SET status = 'cancelled', updated_at = (now() AT TIME ZONE 'UTC')::timestamp WHERE id = $1")
                    .bind(id).execute(&mut *tx).await.pe()?;
            }
            _ => {
                return Err(PosError::BadRequest(
                    "Невірний статус. Використовуйте 'confirmed' або 'cancelled'".to_string(),
                ));
            }
        }
        tx.commit().await.pe()?;
        read_transfer(&self.pool, id).await
    }

    // ── Зміни ПРРО ──────────────────────────────────────────────────────────
    async fn list_shifts(&self, page: i64, size: i64) -> Result<ShiftListDto, PosError> {
        let total: i64 = sqlx::query("SELECT count(*) FROM prro_shifts")
            .fetch_one(&self.pool)
            .await
            .pe()?
            .get("count");
        let offset = (page - 1) * size;
        let rows = sqlx::query(
            "SELECT id, shift_number, opened_at::text, closed_at::text, signer_name, status::text, \
             receipt_count, total_amount::text, zreport_number \
             FROM prro_shifts ORDER BY opened_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(size).bind(offset).fetch_all(&self.pool).await.pe()?;
        let mut items = Vec::new();
        for r in rows {
            let opened: String = r.get("opened_at");
            let opened = NaiveDateTime::parse_from_str(&opened, "%Y-%m-%d %H:%M:%S%.f")
                .unwrap_or_else(|_| Utc::now().naive_utc());
            let closed: Option<String> = r.get("closed_at");
            let closed =
                closed.and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f").ok());
            items.push(PrroShiftDto {
                id: r.get("id"),
                shift_number: r.get("shift_number"),
                opened_at: iso_utc_z(opened),
                closed_at: closed.map(iso_utc_z),
                signer_name: r.get("signer_name"),
                status: r.get("status"),
                receipt_count: r.get("receipt_count"),
                total_amount: r.get("total_amount"),
                zreport_number: r.get("zreport_number"),
            });
        }
        Ok(ShiftListDto {
            items,
            total,
            page,
            size,
        })
    }

    async fn open_shift(&self, _comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        // ПРРО (зовнішній gRPC) недоступний — Python повертає 400 з цим текстом.
        Err(PosError::BadRequest(
            "Не вдалося відкрити зміну: status=-13".to_string(),
        ))
    }

    async fn close_shift(&self, _comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        // Спершу — перевірка відкритої зміни (локальна, як Python).
        let open: Option<i64> = sqlx::query(
            "SELECT shift_number FROM prro_shifts WHERE status = 'open' ORDER BY opened_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .pe()?
        .map(|r| r.get("shift_number"));
        match open {
            None => Err(PosError::BadRequest(
                "Немає відкритої зміни ПРРО".to_string(),
            )),
            Some(_) => Err(PosError::BadRequest(
                "Не вдалося закрити зміну: status=-13".to_string(),
            )),
        }
    }

    // ─── Готівкові операції (внесення/інкасація) ───────────────────────────

    async fn create_cash_operation(
        &self,
        store_id: Uuid,
        user_id: Uuid,
        input: &CashOperationCreateInput,
    ) -> Result<CashOperationDto, PosError> {
        // INSERT ... RETURNING + JOIN users (user_name) — один запит (CTE).
        let row = sqlx::query(
            r#"
            WITH ins AS (
                INSERT INTO cash_operations (id, store_id, user_id, operation_type, cash_type, amount, comment, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, now())
                RETURNING id, store_id, user_id, operation_type, cash_type, amount, comment, created_at
            )
            SELECT ins.id, ins.store_id, ins.user_id, ins.operation_type, ins.cash_type,
                   ins.amount::text, ins.comment, ins.created_at, u.name AS user_name
            FROM ins JOIN users u ON u.id = ins.user_id
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(store_id)
        .bind(user_id)
        .bind(input.operation_type.as_str())
        .bind(input.cash_type.as_str())
        .bind(&input.amount)
        .bind(input.comment.as_deref())
        .fetch_one(&self.pool)
        .await
        .pe()?;
        let amount: BigDecimal = row
            .get::<String, _>("amount")
            .parse()
            .map_err(|e| PosError::Infrastructure(format!("некоректна сума в БД: {e}")))?;
        Ok(CashOperationDto {
            id: row.get("id"),
            store_id: row.get("store_id"),
            user_id: row.get("user_id"),
            user_name: row.get("user_name"),
            operation_type: input.operation_type,
            cash_type: input.cash_type,
            amount,
            comment: row.get("comment"),
            created_at: row.get::<DateTime<Utc>, _>("created_at").naive_utc(),
        })
    }

    async fn list_cash_operations(
        &self,
        store_id: Uuid,
    ) -> Result<CashOperationsListDto, PosError> {
        let rows = sqlx::query(
            r#"
            SELECT co.id, co.store_id, co.user_id, co.operation_type, co.cash_type, co.amount::text,
                   co.comment, co.created_at, u.name AS user_name
            FROM cash_operations co
            JOIN users u ON u.id = co.user_id
            WHERE co.store_id = $1
            ORDER BY co.created_at DESC
            "#,
        )
        .bind(store_id)
        .fetch_all(&self.pool)
        .await
        .pe()?;
        let mut operations = Vec::with_capacity(rows.len());
        for r in &rows {
            let amount: BigDecimal = r
                .get::<String, _>("amount")
                .parse()
                .map_err(|e| PosError::Infrastructure(format!("некоректна сума в БД: {e}")))?;
            let operation_type =
                CashOperationType::parse(r.get("operation_type")).unwrap_or(CashOperationType::Deposit);
            let cash_type = CashType::parse(r.get("cash_type")).unwrap_or(CashType::Cash);
            operations.push(CashOperationDto {
                id: r.get("id"),
                store_id: r.get("store_id"),
                user_id: r.get("user_id"),
                user_name: r.get("user_name"),
                operation_type,
                cash_type,
                amount,
                comment: r.get("comment"),
                created_at: r.get::<DateTime<Utc>, _>("created_at").naive_utc(),
            });
        }
        // Баланси кас точки: внесення − інкасація, окремо cash і card.
        // ::text — sqlx binary-декод numeric втрачає scale (300.00 → 300);
        // рядок зберігає scale колонки, як і в інших Decimal-полях проєкту.
        let (cash_raw, card_raw): (String, String) = sqlx::query_as(
            r#"
            SELECT COALESCE(
                       SUM(CASE WHEN operation_type = 'deposit' THEN amount ELSE -amount END)
                           FILTER (WHERE cash_type = 'cash'),
                       0
                   )::numeric(12,2)::text,
                   COALESCE(
                       SUM(CASE WHEN operation_type = 'deposit' THEN amount ELSE -amount END)
                           FILTER (WHERE cash_type = 'card'),
                       0
                   )::numeric(12,2)::text
            FROM cash_operations WHERE store_id = $1
            "#,
        )
        .bind(store_id)
        .fetch_one(&self.pool)
        .await
        .pe()?;
        let parse_balance = |raw: String| -> Result<BigDecimal, PosError> {
            raw.parse()
                .map_err(|e| PosError::Infrastructure(format!("некоректний баланс у БД: {e}")))
        };
        let balances = CashBalances {
            cash: parse_balance(cash_raw)?,
            card: parse_balance(card_raw)?,
        };
        Ok(CashOperationsListDto { operations, balances })
    }
}
