//! Інтеграційний тест: ФІКС 2026-08-21 — прибуткова накладна оновлює
//! І `stock` (по точках), І `products.stock` (сумарний залишок).
//!
//! Python-еталон: invoice_use_cases.confirm → product.update_stock(qty)
//! → products.stock += qty. Rust раніше оновлював ЛИШЕ stock table →
//! список товарів (directories.rs читає p.stock) не змінювався (0 → 0).
//!
//! Тест: confirm прибуткової (qty=1) → products.stock +1, stock.quantity +1.
//! Cancel → обидва повертаються до 0.
//!
//! Потребує PostgreSQL (DATABASE_URL або DB_* у backend/.env, як Python-еталон).

use torgashka_domain::{
    invoices::{InvoiceCreateV1Input, InvoiceItemV1Input},
    InvoicesV1Service,
};
use torgashka_infrastructure::{
    db,
    repositories::invoices::SqlxInvoices,
    store_ctx::{with_store_ctx, StoreCtx, StorePool},
};
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    db::connect_test_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

fn repo(p: &sqlx::PgPool) -> SqlxInvoices {
    SqlxInvoices::new(StorePool::new(p.clone()))
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

async fn any_store_ctx(p: &sqlx::PgPool) -> StoreCtx {
    let store_id: Uuid = sqlx::query_scalar("SELECT id FROM stores ORDER BY created_at LIMIT 1")
        .fetch_one(p)
        .await
        .expect("у БД має бути хоча б одна точка");
    let user_id = any_user_id(p).await;
    StoreCtx {
        user_id,
        store_id,
        role: "owner".to_string(),
    }
}

async fn make_product(p: &sqlx::PgPool, barcode: &str, store_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, cost_price, stock, tax_rate, tax_group, unit, is_weight, scan_excise, is_fiscal, fiscal_stock, created_at, updated_at) \
         VALUES ($1, $2, $3, 100.00, 50.00, 0, 20.00, 'А', 'шт', false, false, false, 0, now(), now())",
    )
    .bind(id)
    .bind(barcode)
    .bind(format!("ТЕСТ-STOCK-{barcode}"))
    .execute(p)
    .await
    .expect("create product");
    // stock row для поточної точки (quantity = 0)
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, price) VALUES ($1, $2, 0, 100.00)",
    )
    .bind(store_id)
    .bind(id)
    .execute(p)
    .await
    .expect("create stock row");
    id
}

async fn make_supplier(p: &sqlx::PgPool, name: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO suppliers (name, phone, address, created_at, updated_at) \
         VALUES ($1, '', '', now(), now()) RETURNING id",
    )
    .bind(name)
    .fetch_one(p)
    .await
    .expect("supplier insert")
}

async fn product_stock(p: &sqlx::PgPool, id: Uuid) -> rust_decimal::Decimal {
    let s: String =
        sqlx::query_scalar("SELECT COALESCE(stock, 0)::text FROM products WHERE id = $1")
            .bind(id)
            .fetch_one(p)
            .await
            .expect("read products.stock");
    s.parse().expect("products.stock decimal")
}

async fn store_stock(p: &sqlx::PgPool, id: Uuid, store_id: Uuid) -> rust_decimal::Decimal {
    let s: String = sqlx::query_scalar(
        "SELECT COALESCE(quantity, 0)::text FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(store_id)
    .bind(id)
    .fetch_one(p)
    .await
    .expect("read stock.quantity");
    s.parse().expect("stock.quantity decimal")
}

#[tokio::test]
async fn confirm_invoice_updates_products_stock_and_store_stock() {
    let p = pool().await;
    let ctx = any_store_ctx(&p).await;
    let product_id = make_product(&p, &format!("STK{}", uniq()), ctx.store_id).await;
    let supplier_id = make_supplier(&p, &format!("SUP-{}", uniq())).await;

    let input = InvoiceCreateV1Input {
        number: None,
        supplier_id,
        invoice_date: chrono::Local::now().naive_local(),
        payment_method: Some("cash".into()),
        is_fiscal: false,
        notes: None,
        total_amount: Some("100.00".into()),
        items: vec![InvoiceItemV1Input {
            product_id,
            quantity: "1".into(),
            price: "100.00".into(),
            total: "100.00".into(),
            cost_price: Some("50.00".into()),
            markup_percent: None,
        }],
    };

    let r = with_store_ctx(ctx.clone(), async {
        repo(&p).create_v1(&input, ctx.user_id).await
    })
    .await
    .expect("create invoice");

    // До confirm: products.stock = 0, stock.quantity = 0.
    assert_eq!(
        product_stock(&p, product_id).await,
        rust_decimal::Decimal::ZERO
    );
    assert_eq!(
        store_stock(&p, product_id, ctx.store_id).await,
        rust_decimal::Decimal::ZERO
    );

    // Confirm прибуткової (qty=1).
    with_store_ctx(ctx.clone(), async {
        repo(&p).confirm_v1(r.id, "confirmed").await
    })
    .await
    .expect("confirm invoice");

    // ФІКС: products.stock збільшився на 1 (Python-еталон), stock.quantity теж.
    let ps = product_stock(&p, product_id).await;
    let ss = store_stock(&p, product_id, ctx.store_id).await;
    assert_eq!(
        ps,
        rust_decimal::Decimal::ONE,
        "products.stock має бути 1 після прибуткової qty=1, отримано {ps}"
    );
    assert_eq!(
        ss,
        rust_decimal::Decimal::ONE,
        "stock.quantity має бути 1 після прибуткової qty=1, отримано {ss}"
    );

    // Cancel: обидва повертаються до 0 (Python cancel_invoice: quantity_change=-qty).
    with_store_ctx(ctx.clone(), async {
        repo(&p).confirm_v1(r.id, "cancelled").await
    })
    .await
    .expect("cancel invoice");

    let ps = product_stock(&p, product_id).await;
    let ss = store_stock(&p, product_id, ctx.store_id).await;
    assert_eq!(
        ps,
        rust_decimal::Decimal::ZERO,
        "products.stock має бути 0 після cancel, отримано {ps}"
    );
    assert_eq!(
        ss,
        rust_decimal::Decimal::ZERO,
        "stock.quantity має бути 0 після cancel, отримано {ss}"
    );

    // cleanup
    let _ = sqlx::query("DELETE FROM invoice_items WHERE invoice_id = $1")
        .bind(r.id)
        .execute(&p)
        .await;
    let _ = sqlx::query("DELETE FROM invoices WHERE id = $1")
        .bind(r.id)
        .execute(&p)
        .await;
    let _ = sqlx::query("DELETE FROM supplier_ledger WHERE supplier_id = $1")
        .bind(supplier_id)
        .execute(&p)
        .await;
    let _ = sqlx::query("DELETE FROM suppliers WHERE id = $1")
        .bind(supplier_id)
        .execute(&p)
        .await;
    let _ = sqlx::query("DELETE FROM stock WHERE product_id = $1")
        .bind(product_id)
        .execute(&p)
        .await;
    let _ = sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(product_id)
        .execute(&p)
        .await;
}
