//! Інтеграційні тести POS (етап 3): чеки v2, транзакційність, конкурентність,
//! списання та переміщення.
//!
//! Потребують доступної PostgreSQL (як Python-еталон): `DATABASE_URL` або
//! `DB_*` у backend/.env.
//!
//! Конкурентність: два паралельні `create_sale_receipt` на одному товарі —
//! `SELECT ... FOR UPDATE` серіалізує продажі, кінцевий залишок = стартовий
//! мінус сума кількостей, нуль втрат.
//!
//! Транзакційність: sale з двома позиціями, де друга має недостатній залишок
//! → 400, перший товар НЕ змінюється, чек не створюється.

use kasa_domain::{
    PosService, ReceiptCreateInput, ReceiptItemInput, TransferCreateInput, WriteOffCreateInput,
};
use kasa_infrastructure::{db, repositories::pos::SqlxPos};
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    db::connect_readonly_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

fn repo(p: &sqlx::PgPool) -> SqlxPos {
    SqlxPos::new(p.clone())
}

fn uniq() -> String {
    chrono::Utc::now().timestamp_micros().to_string()
}

async fn any_user_id(p: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users ORDER BY created_at LIMIT 1")
        .fetch_one(p)
        .await
        .expect("у БД має бути хоча б один користувач")
}

async fn make_product(p: &sqlx::PgPool, barcode: &str, stock: i64) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, cost_price, stock, tax_rate, tax_group, unit, is_weight, scan_excise, is_fiscal, fiscal_stock, created_at, updated_at) \
         VALUES ($1, $2, $3, 100.00, 50.00, $4, 20.00, 'А', 'шт', false, false, false, 0, now(), now())",
    )
    .bind(id)
    .bind(barcode)
    .bind(format!("ТЕСТ-POS-{barcode}"))
    .bind(stock)
    .execute(p)
    .await
    .expect("create product");
    id
}

async fn stock_of(p: &sqlx::PgPool, id: Uuid) -> i64 {
    let s: String = sqlx::query_scalar("SELECT stock::text FROM products WHERE id = $1")
        .bind(id)
        .fetch_one(p)
        .await
        .unwrap();
    s.trim_end_matches('0')
        .trim_end_matches('.')
        .parse::<f64>()
        .map(|v| (v * 1000.0).round() as i64)
        .unwrap_or(0)
}

fn sale_input(product: Uuid, qty: &str) -> ReceiptCreateInput {
    sale_input_cash(product, qty, "250")
}

fn sale_input_cash(product: Uuid, qty: &str, cash: &str) -> ReceiptCreateInput {
    ReceiptCreateInput {
        items: vec![ReceiptItemInput {
            product_id: product,
            name: String::new(),
            quantity: qty.to_string(),
            price: "100".to_string(),
            tax_rate: 20,
        }],
        payment_method: "cash".to_string(),
        cash_amount: Some(cash.to_string()),
        card_amount: None,
        customer_id: None,
        cashier_id: None,
        notes: String::new(),
        terminal_rrn: None,
        terminal_approval_code: None,
        terminal_invoice_number: None,
        terminal_transaction_id: None,
        terminal_response_code: None,
        terminal_status: None,
        terminal_receipt: None,
        terminal_card_pan: None,
        terminal_payment_system: None,
        terminal_merchant: None,
        terminal_created_at: None,
        is_fiscal: false,
        split_group_id: None,
    }
}

async fn cleanup_product(p: &sqlx::PgPool, id: Uuid) {
    let _ = sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(id)
        .execute(p)
        .await;
}

#[tokio::test]
async fn sale_receipt_flow_stock_and_change() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let pid = make_product(&p, &format!("{ts}S"), 100).await;
    let cashier = any_user_id(&p).await;

    let mut input = sale_input(pid, "2");
    input.cashier_id = Some(cashier);
    let receipt = r.create_sale_receipt(&input).await.expect("sale ok");
    assert_eq!(receipt.number.len(), 20); // RCPT-YYYYMMDD-XXXXXX
    assert!(receipt.number.starts_with("RCPT-"));
    assert_eq!(receipt.items[0].quantity, 2.0);
    assert_eq!(receipt.total.unwrap(), 200.0);
    assert_eq!(receipt.change_amount, Some(50.0));
    assert_eq!(receipt.fiscal_status, "none");
    assert_eq!(stock_of(&p, pid).await, 98000); // 100.000 - 2.000

    // Недостатньо залишку → 400.
    let mut big = sale_input_cash(pid, "100", "10000");
    big.cashier_id = Some(cashier);
    let err = r.create_sale_receipt(&big).await;
    let err = err.expect_err("sale має впасти");
    assert!(
        matches!(&err, kasa_domain::PosError::BadRequest(msg) if msg.contains("Недостатньо залишку")),
        "{err:?}"
    );
    assert_eq!(stock_of(&p, pid).await, 98000);

    // Return → stock збільшується.
    let mut ret = sale_input(pid, "1");
    ret.cashier_id = Some(cashier);
    r.create_return_receipt(&ret).await.expect("return ok");
    assert_eq!(stock_of(&p, pid).await, 99000);

    cleanup_product(&p, pid).await;
}

#[tokio::test]
async fn concurrent_sales_no_data_loss() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let pid = make_product(&p, &format!("{ts}C"), 100).await;
    let cashier = any_user_id(&p).await;

    let mut a = sale_input_cash(pid, "7", "700");
    a.cashier_id = Some(cashier);
    let mut b = sale_input_cash(pid, "3", "300");
    b.cashier_id = Some(cashier);

    let (ra, rb) = tokio::join!(r.create_sale_receipt(&a), r.create_sale_receipt(&b));
    ra.expect("sale A");
    rb.expect("sale B");
    assert_eq!(stock_of(&p, pid).await, 90000); // 100 - 7 - 3

    cleanup_product(&p, pid).await;
}

#[tokio::test]
async fn sale_transaction_rolls_back_on_second_item() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let ok_pid = make_product(&p, &format!("{ts}T1"), 100).await;
    let low_pid = make_product(&p, &format!("{ts}T2"), 0).await;
    let cashier = any_user_id(&p).await;

    let mut input = sale_input(ok_pid, "5");
    input.cashier_id = Some(cashier);
    input.items.push(ReceiptItemInput {
        product_id: low_pid,
        name: String::new(),
        quantity: "1".to_string(),
        price: "100".to_string(),
        tax_rate: 20,
    });
    let before = stock_of(&p, ok_pid).await;
    let count_before: i64 = sqlx::query_scalar("SELECT count(*) FROM receipts")
        .fetch_one(&p)
        .await
        .unwrap();

    let err = r
        .create_sale_receipt(&input)
        .await
        .expect_err("sale має впасти");
    assert!(matches!(err, kasa_domain::PosError::BadRequest(_)));

    // Нічого не записано: stock першого не змінився, чек не створено.
    assert_eq!(stock_of(&p, ok_pid).await, before);
    let count_after: i64 = sqlx::query_scalar("SELECT count(*) FROM receipts")
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(count_before, count_after);

    cleanup_product(&p, ok_pid).await;
    cleanup_product(&p, low_pid).await;
}

#[tokio::test]
async fn write_off_and_transfer_flow() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let pid = make_product(&p, &format!("{ts}W"), 100).await;
    let user = any_user_id(&p).await;

    // Write-off: create авто-confirm → stock зменшується.
    let wo = WriteOffCreateInput {
        number: None,
        reason: "expired".to_string(),
        write_off_date: chrono::Utc::now().naive_utc(),
        notes: Some("test".to_string()),
        created_by: user,
        items: vec![kasa_domain::DocItemInput {
            product_id: pid,
            quantity: "2".to_string(),
            cost_price: None,
            price: None,
        }],
    };
    let wo = r.create_write_off(&wo).await.expect("write-off");
    assert_eq!(wo.status, "confirmed");
    assert!(wo.number.starts_with("СП-"));
    assert_eq!(wo.items[0].quantity, "2"); // вхідна scale
    assert_eq!(wo.total_amount.as_deref(), Some("0.0")); // float-джерело Python
    assert_eq!(stock_of(&p, pid).await, 98000);

    // GET → scale БД.
    let got = r.get_write_off(wo.id).await.expect("get");
    assert_eq!(got.items[0].quantity, "2.000");
    assert_eq!(got.total_amount.as_deref(), Some("0.00"));

    // Transfer: create draft → confirm → cancel (відкат).
    let tr = TransferCreateInput {
        number: None,
        from_location: "Склад-1".to_string(),
        to_location: "Склад-2".to_string(),
        transfer_date: chrono::Utc::now().naive_utc(),
        notes: None,
        created_by: user,
        items: vec![kasa_domain::DocItemInput {
            product_id: pid,
            quantity: "3".to_string(),
            cost_price: None,
            price: None,
        }],
    };
    let tr = r.create_transfer(&tr).await.expect("transfer");
    assert_eq!(tr.status, "draft");
    assert_eq!(stock_of(&p, pid).await, 98000); // draft не змінює stock
    r.confirm_transfer(tr.id, "confirmed")
        .await
        .expect("confirm");
    assert_eq!(stock_of(&p, pid).await, 95000);
    r.confirm_transfer(tr.id, "cancelled")
        .await
        .expect("cancel");
    assert_eq!(stock_of(&p, pid).await, 98000);

    let _ = sqlx::query("DELETE FROM write_offs WHERE id = $1")
        .bind(wo.id)
        .execute(&p)
        .await;
    cleanup_product(&p, pid).await;
}

#[tokio::test]
async fn today_stats_delta_matches_python_formula() {
    // Статистика за сьогодні: продаж 2 шт × 100 грн (собівартість 50, ПДВ 20%)
    // має змінити агрегати на детерміновану дельту.
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let pid = make_product(&p, &format!("{ts}ST"), 100).await;
    let cashier = any_user_id(&p).await;

    let before = r.today_stats().await.expect("stats before");

    let mut input = sale_input_cash(pid, "2", "200");
    input.cashier_id = Some(cashier);
    r.create_sale_receipt(&input).await.expect("sale");

    let after = r.today_stats().await.expect("stats after");
    assert_eq!(after.receipts_count - before.receipts_count, 1);
    assert_eq!(after.items_sold - before.items_sold, 2);
    // total_sales += 200.0 (НЕ залежить від паралельних продажів nastya:
    // порівнюємо дельту, але інші продажі можуть додатись — тому перевіряємо
    // лише мінімальну дельту, а не точну рівність).
    assert!(after.total_sales - before.total_sales >= 200.0);
    // ПДВ позиції: 200 * 20/120 = 33.333... → float(Decimal) Python.
    assert!((after.total_vat - before.total_vat - 33.33333333333333).abs() < 1e-9);

    let _ = sqlx::query("DELETE FROM receipt_items WHERE receipt_id IN (SELECT id FROM receipts WHERE id IN (SELECT receipt_id FROM receipt_items WHERE product_id = $1))")
        .bind(pid).execute(&p).await;
    let _ = sqlx::query("DELETE FROM receipts WHERE id IN (SELECT receipt_id FROM receipt_items WHERE product_id = $1)")
        .bind(pid).execute(&p).await;
    cleanup_product(&p, pid).await;
}
