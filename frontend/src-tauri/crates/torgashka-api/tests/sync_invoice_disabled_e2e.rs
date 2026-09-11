//! Негативний E2E ADR-0007 §3.4: Rust-гілка інвойсів ВИМКНЕНА
//! (`TORGASHKA_RUST_INVOICES=0`) — приймач /sync/push НЕ робить тихого ack.
//!
//! Окремий тест-бінар: env-змінна процес-глобальна, тому «вимкнено» ізольовано
//! від позитивних e2e (sync_invoice_push_e2e.rs).
//!
//! Очікування: status='error' з текстом «Rust-гілка інвойсів вимкнена»,
//! 0 рядків у invoices, оп каси ЛИШАЄТЬСЯ в outbox (каса бачить проблему —
//! жодних втрат/тиші).

mod common;

use std::time::Duration;

use serde_json::json;
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_push::{open_connection, pending_count};
use torgashka_infrastructure::offline::transactions;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn apply_schema() {
    SCHEMA_ONCE
        .get_or_init(|| async {
            let p = torgashka_infrastructure::db::connect_test_pool(5)
                .await
                .expect("тестова БД недоступна");
            torgashka_infrastructure::db::ensure_schema(&p)
                .await
                .expect("ensure_schema");
            sync_schema::apply(&p).await;
            p.close().await;
        })
        .await;
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

async fn login(base: &str) -> String {
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if let Ok(r) = client
            .post(format!("{base}/api/v1/auth/login"))
            .json(&json!({"login": "admin", "password": "admin123"}))
            .send()
            .await
        {
            if r.status().is_success() {
                let v: serde_json::Value = r.json().await.expect("login json");
                return v["access_token"].as_str().expect("token").to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("login не вдався");
}

/// Локальний каталог каси (SQLite `products_v2`) — як після master-pull:
/// приймання товару валідує позиції проти нього (ADR-0007 §5 AT-14).
fn seed_local_catalog(db: &std::path::Path, product: Uuid) {
    let conn = torgashka_infrastructure::offline::sync_push::open_connection(db)
        .expect("SQLite каси + міграції");
    conn.execute(
        "INSERT INTO products_v2 (id, name, price, is_deleted, server_version) \
         VALUES (?1, 'E2E каталог (pull)', 100.0, 0, 1) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name",
        [product.to_string()],
    )
    .expect("products_v2 (локальний каталог)");
}

#[tokio::test]
async fn invoice_push_with_rust_invoices_disabled_is_not_silently_acked() {
    common::force_test_db();
    apply_schema().await;
    let pool = torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool");

    // Seed: адмін + точка + постачальник + товар.
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Admin', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind("$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e")
    .execute(&pool)
    .await
    .expect("seed admin");
    let store = Uuid::new_v4();
    let supplier = Uuid::new_v4();
    let product = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E InvOff Точка')")
        .bind(store)
        .execute(&pool)
        .await
        .expect("store");
    sqlx::query("INSERT INTO suppliers (id, name) VALUES ($1, 'E2E InvOff Постачальник')")
        .bind(supplier)
        .execute(&pool)
        .await
        .expect("supplier");
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) VALUES ($1, NULL, 'E2E InvOff Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(&pool)
    .await
    .expect("product");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, $1, 'owner', '{}'::jsonb, true, now() FROM users u WHERE u.login = 'admin'
         ON CONFLICT DO NOTHING",
    )
    .bind(store)
    .execute(&pool)
    .await
    .expect("user_stores");

    // Каса: накладна локально (outbox pending).
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let db = dir.path().join("invoice-off.db");
    let mut conn = open_connection(&db).expect("каса БД");
    seed_local_catalog(&db, product);
    let payload = json!({
        "number": "INV-E2E-OFF",
        "supplier_id": supplier.to_string(),
        "invoice_date": "2026-08-30T12:00:00+03:00",
        "payment_method": null,
        "is_fiscal": false,
        "notes": "накладна при вимкненій Rust-гілці",
        "total_amount": "300.00",
        "items": [{"product_id": product.to_string(), "quantity": "3", "price": "100.00", "total": "300.00"}],
    })
    .to_string();
    let out = transactions::enqueue_invoice(&mut conn, &payload, &store.to_string())
        .expect("enqueue_invoice");
    let envelope: String = conn
        .query_row(
            "SELECT payload FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![out.client_uuid],
            |r| r.get(0),
        )
        .expect("конверт");
    assert_eq!(pending_count(&conn).expect("pending"), 1);
    drop(conn);

    // Сервер: Rust-гілка інвойсів ВИМКНЕНА.
    std::env::set_var(torgashka_api::RUST_INVOICES_ENV, "0");
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    let envelope: serde_json::Value = serde_json::from_str(&envelope).expect("JSON");
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/sync/push"))
        .bearer_auth(&token)
        .header("X-Store-Id", store.to_string())
        .json(&vec![envelope])
        .send()
        .await
        .expect("push HTTP");
    assert!(resp.status().is_success(), "HTTP {}", resp.status());
    let body: Vec<serde_json::Value> = resp.json().await.expect("per-item JSON");
    eprintln!("[invoice-off e2e] per-item: {body:?}");
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["status"], "error", "НЕ тихий ack: статус = error");
    let err = body[0]["error"].as_str().unwrap_or_default().to_string();
    assert!(
        err.contains("вимкнена"),
        "текст про вимкнену Rust-гілку, маємо: {err}"
    );

    // Нічого не застосовано на сервері.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE client_uuid = $1")
        .bind(out.client_uuid.parse::<Uuid>().expect("uuid"))
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(n, 0, "накладну не створено");

    // Каса: оп ЛИШИВСЯ в outbox (ні втрат, ні тиші).
    let conn = open_connection(&db).expect("БД");
    let (status, left): (String, i64) = conn
        .query_row(
            "SELECT status, COUNT(*) FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![out.client_uuid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("outbox");
    assert_eq!(left, 1, "оп лишився в outbox каси");
    assert_eq!(status, "pending", "статус не змінився (тихого ack немає)");
    drop(conn);

    eprintln!(
        "[sync_invoice_disabled_e2e] ✅ вимкнена Rust-гілка → error + оп лишився pending (не тихий ack)"
    );
}
