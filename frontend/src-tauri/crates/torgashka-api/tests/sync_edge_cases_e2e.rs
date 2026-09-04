//! E2E: edge-cases offline-first (ЕТАП 5) — реальний POST /api/v1/sync/push
//! (torgashka-api фасад + PostgreSQL).
//!
//! Сценарії (дизайн sync-schema-design.md розділ 6.2):
//!   1. price_snapshot_old_price_kept — зміна ціни: чек продає за СТАРОЮ
//!      ціною (price=90, price_snapshot=90), актуальна ціна товару в
//!      довіднику/stocks — 100. Сервер приймає чек як є (created) —
//!      виручка за снапшотом, НЕ перераховує на актуальну ціну.
//!   2. deleted_product_offline_receipt_pushed — товар видалено на сервері
//!      (is_deleted=true) ПІСЛЯ створення офлайн-чека касою (чек у outbox
//!      зі снапшотом, створений ДО видалення) → push успішний (created):
//!      сервер приймає за снапшотом, валідація is_deleted на касі (при
//!      НОВОМУ продажу) не зачіпає вже записаний outbox.
//!   3. unsupported_transfer_marks_failed — агрегат, який каса не може
//!      створити сьогодні (transfer_in — «точка Б не існує/не підтримується
//!      push ЕТАП 4») → сервер per-item error → каса ставить outbox
//!      failed + last_error (аномалія видима оператору, дизайн 6.2).
//!
//! Потребує доступної PostgreSQL (backend/.env) — як sync_push_e2e.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_push::{
    enqueue_receipt, open_connection, outbox_stats, pending_outbox, push_pending_batch, PushConfig,
};
use uuid::Uuid;

/// Тестова точка (seed онбордингу) — та сама, що в sync_push_e2e.
const STORE1: &str = "d9be9608-c011-49be-b776-3317ca5e9af6";

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

async fn api_pool() -> sqlx::PgPool {
    let _ = torgashka_infrastructure::db::resolve_database_url()
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env");
    torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool")
}

/// Застосувати схему БД (довідники + sync-таблиці + owner-DDL) ОДИН раз на
/// процес. ensure_schema ідемпотентний: порожня БД отримує повну схему
/// (SCHEMA_SQL), мігрована — лише додатки. Без цього ensure_seed падає на
/// порожній тестовій БД («relation "stores" does not exist»), бо ensure_schema
/// виконується фасадом лише при старті (run_facade).
/// Один виклик на процес (OnceCell) — паралельні тести не конкурують за
/// CREATE TABLE на fresh-БД.
#[path = "common/sync_schema.rs"]
mod sync_schema;

static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn apply_schema() {
    SCHEMA_ONCE
        .get_or_init(|| async {
            let p = torgashka_infrastructure::db::connect_test_pool(5)
                .await
                .expect("тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test");
            torgashka_infrastructure::db::ensure_schema(&p)
                .await
                .expect("ensure_schema на тестовій БД");
            // Sync-шар (Alembic 0011-0014): server_version, sync_meta,
            // soft-delete, client_uuid — ensure_schema (schema.sql) їх не має.
            sync_schema::apply(&p).await;
            p.close().await;
        })
        .await;
}

/// Seed: точка + адмін + user_stores. Повертає (store, product_id).
async fn ensure_seed(pool: &sqlx::PgPool) -> Uuid {
    apply_schema().await;
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Edge Точка') ON CONFLICT (id) DO NOTHING",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .execute(pool)
    .await
    .expect("seed store");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'Seed Адмін', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind("$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e")
    .execute(pool)
    .await
    .expect("seed admin");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, s.id, 'owner', '{}'::jsonb, true, now()
         FROM users u, stores s
         WHERE u.login = 'admin' AND s.id = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .execute(pool)
    .await
    .expect("seed user_stores");

    let product = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E Edge Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(pool)
    .await
    .expect("seed product");
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, price) VALUES ($1, $2, 1000, 100.00)",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .bind(product)
    .execute(pool)
    .await
    .expect("seed stock");
    product
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
                let v: Value = r.json().await.expect("login json");
                return v["access_token"]
                    .as_str()
                    .expect("access_token")
                    .to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("login: сервер не піднявся");
}

fn push_cfg(db_path: &PathBuf, base: &str, token: &str) -> PushConfig {
    PushConfig {
        base_url: base.to_string(),
        token: token.to_string(),
        store_id: Some(STORE1.to_string()),
        db_path: db_path.clone(),
        interval_secs: 30,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Зміна ціни: чек за старою ціною (price_snapshot) → created, PG=90.00
// ─────────────────────────────────────────────────────────────────────────────

mod common;

#[tokio::test]
async fn price_snapshot_old_price_kept() {
    common::force_test_db();
    let pool = api_pool().await;
    let product = ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    // Каса offline: локальна копія товару ще зі СТАРОЮ ціною 90 (сервер
    // вже оновив до 100 — але чек створено ДО pull). Снапшот зафіксував
    // ціну продажу 90 (enrich на касі, ЕТАП 5).
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let db_path = dir.path().join("cash-price.db");
    let mut conn = open_connection(&db_path).expect("каса БД");
    let receipt = json!({
        "receipt_type": "sale",
        "receipt_number": 7001,
        "items": [{
            "product_id": product.to_string(),
            "quantity": 2,
            "price": "90.00",
            "price_snapshot": "90.00",
            "name_snapshot": "E2E Edge Товар",
        }],
        "total_amount": "180.00",
        "payment_method": "cash",
        "cash_amount": "180.00",
    })
    .to_string();
    let out = enqueue_receipt(&mut conn, &receipt, Some(STORE1)).expect("enqueue");
    drop(conn);

    let cfg = push_cfg(&db_path, &base, &token);
    let client = reqwest::Client::new();
    let s = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push");
    assert_eq!(s.done, 1, "чек за старою ціною прийнято (created)");
    assert_eq!(s.failed, 0);

    // КРИТЕРІЙ: у PG збережено price=90.00 (виручка за снапшотом),
    // сервер НЕ перерахував на актуальну ціну 100.
    let (price, qty): (String, String) = sqlx::query_as(
        "SELECT ri.price::text, ri.quantity::text \
         FROM receipt_items ri JOIN receipts r ON r.id = ri.receipt_id \
         WHERE r.client_uuid = $1",
    )
    .bind(Uuid::parse_str(&out.client_uuid).unwrap())
    .fetch_one(&pool)
    .await
    .expect("receipt_items");
    assert_eq!(
        price, "90.00",
        "ціна за снапшотом (стара), не актуальна 100"
    );
    assert_eq!(
        qty.parse::<f64>().unwrap(),
        2.0,
        "кількість 2 (numeric(10,3) → '2.000')"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Товар видалено на сервері після створення офлайн-чека → push успішний
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn deleted_product_offline_receipt_pushed() {
    common::force_test_db();
    let pool = api_pool().await;
    let product = ensure_seed(&pool).await;

    // Офлайн-чек створено, коли товар був живий (снапшот зафіксовано).
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let db_path = dir.path().join("cash-del.db");
    let mut conn = open_connection(&db_path).expect("каса БД");
    let receipt = json!({
        "receipt_type": "sale",
        "receipt_number": 7002,
        "items": [{
            "product_id": product.to_string(),
            "quantity": 1,
            "price": "100.00",
            "price_snapshot": "100.00",
            "name_snapshot": "E2E Edge Товар",
        }],
        "total_amount": "100.00",
        "payment_method": "cash",
        "cash_amount": "100.00",
    })
    .to_string();
    let out = enqueue_receipt(&mut conn, &receipt, Some(STORE1)).expect("enqueue");
    drop(conn);

    // ...ПОТІМ сервер видаляє товар (як було б через адмінку/іншу точку).
    sqlx::query("UPDATE products SET is_deleted = true WHERE id = $1")
        .bind(product)
        .execute(&pool)
        .await
        .expect("видалити продукт");

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    let cfg = push_cfg(&db_path, &base, &token);
    let client = reqwest::Client::new();
    let s = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push");
    assert_eq!(
        s.done, 1,
        "офлайн-чек ДО видалення приймається за снапшотом"
    );
    assert_eq!(s.failed, 0);

    // На сервері РІВНО 1 запис; outbox каси — done.
    let cnt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE client_uuid = $1")
        .bind(Uuid::parse_str(&out.client_uuid).unwrap())
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(cnt, 1);
    let stats = outbox_stats(&open_connection(&db_path).expect("БД")).expect("stats");
    assert_eq!(stats.pending, 0, "outbox вивантажено");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Тип поза приймачами push (ЕТАП 7b приймає receipt/return_receipt/
// purchase_order/inventory/transfer/write_off; work_session каса офлайн не
// генерує — див. transactions.rs) → per-item error → failed + last_error
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn unsupported_type_marks_failed_with_error() {
    common::force_test_db();
    let pool = api_pool().await;
    let _product = ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    // Агрегат невідомого типу (work_session каса локально НЕ створює) →
    // сервер відхиляє: per-item error → каса failed + last_error (аномалія).
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let db_path = dir.path().join("cash-unknown.db");
    let conn = open_connection(&db_path).expect("каса БД");
    let client_uuid = Uuid::new_v4().to_string();
    let payload = json!({
        "type": "work_session",
        "client_uuid": client_uuid,
        "store_id": STORE1,
        "created_at": "2026-09-02T00:00:00Z",
        "payload": {},
    })
    .to_string();
    conn.execute(
        "INSERT INTO outbox (type, client_uuid, payload, status) \
         VALUES ('work_session', ?1, ?2, 'pending')",
        rusqlite::params![client_uuid, payload],
    )
    .expect("insert work_session");
    drop(conn);

    let cfg = push_cfg(&db_path, &base, &token);
    let client = reqwest::Client::new();
    let s = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push (HTTP 200 з per-item error)");
    assert_eq!(s.failed, 1, "сервер відхилив невідомий тип → failed");
    assert_eq!(s.done, 0);

    // КРИТЕРІЙ: outbox status=failed + last_error заповнено (аномалія видима).
    let conn = open_connection(&db_path).expect("БД");
    let (status, last_error): (String, Option<String>) = conn
        .query_row(
            "SELECT status, last_error FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![client_uuid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("outbox стан");
    assert_eq!(status, "failed", "непідтримуваний тип → failed (без retry)");
    let err = last_error.expect("last_error заповнено");
    assert!(
        err.contains("не підтримується"),
        "last_error пояснює аномалію: {err}"
    );
    // pending більше немає — failed-агрегат не зациклюється.
    let batch = pending_outbox(&conn, 10).expect("pending");
    assert_eq!(batch.len(), 0, "failed поза чергою повторів");
    let stats = outbox_stats(&conn).expect("stats");
    assert_eq!(stats.failed, 1, "failed_count = 1 (потребує уваги)");
    assert_eq!(stats.last_error.as_deref(), Some(err.as_str()));
}
