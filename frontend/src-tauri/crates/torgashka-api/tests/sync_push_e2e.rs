//! E2E: push-клієнт каси (ЕТАП 4 offline-first) з РЕАЛЬНИМ ендпоінтом
//! POST /api/v1/sync/push (torgashka-api фасад + PostgreSQL).
//!
//! Повний цикл каса → сервер: outbox каси (SQLite, enqueue_receipt з
//! client_uuid) → push_pending_batch (POST /sync/push) → серверний прийом
//! (created, PG receipts + sync_log) → outbox done.
//!
//! Сценарії:
//!   1. push_idempotent_single_server_record — 2× push одного client_uuid →
//!      created, потім already_exists; на сервері РІВНО 1 запис (сценарій
//!      «відповідь загубилась після COMMIT сервера» — каса повторює push,
//!      дизайн 3.3);
//!   2. server_down_outbox_grows_then_flushes_fifo — вимкнений сервер →
//!      outbox росте (статуси pending, без backoff — «немає мережі»);
//!      після запуску сервера — весь outbox вивантажується FIFO (done).
//!
//! Потребує доступної PostgreSQL (backend/.env) — як інші інтеграційні
//! тести крейта. Схема: Alembic head (0011 + 0012 + 0013_sync_push_idempotency).

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use sqlx::Row;
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_push::{
    enqueue_receipt, open_connection, pending_count, push_pending_batch, PushConfig,
};
use uuid::Uuid;

/// Тестова точка (seed онбордингу).
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

/// Seed: точка + адмін + user_stores + продукт із залишком (для чека sale).
async fn ensure_seed(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Push Точка') ON CONFLICT (id) DO NOTHING",
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

    // Продукт + залишок точки (створення чека sale потребує stock ≥ qty).
    let product = Uuid::new_v4();
    sqlx::query(
        // barcode NULL: унікальний індекс ix_products_barcode — спільний для
        // всіх тестів на одній dev-БД (паралельний прогін → дублікат).
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E Push Товар', 100.00, 20.00)",
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

/// Тимчасова SQLite-БД каси (міграції 0001-0004) + enqueue чека sale.
fn new_cash_db(dir: &tempfile::TempDir, product: Uuid, n: i64) -> (PathBuf, String) {
    let db_path = dir.path().join(format!("cash-{n}.db"));
    let mut conn = open_connection(&db_path).expect("каса БД");
    let receipt = json!({
        "receipt_type": "sale",
        "receipt_number": n,
        "items": [{"product_id": product.to_string(), "quantity": 1, "price": "100.00"}],
        "total_amount": "100.00",
        "payment_method": "cash",
        "cash_amount": "100.00",
    })
    .to_string();
    let out = enqueue_receipt(&mut conn, &receipt, Some(STORE1)).expect("enqueue чек");
    (db_path, out.client_uuid)
}

fn push_cfg(db_path: &PathBuf, base: &str, token: &str) -> PushConfig {
    PushConfig {
        base_url: base.to_string(),
        token: token.to_string(),
        store_id: STORE1.to_string(),
        db_path: db_path.clone(),
        interval_secs: 30,
    }
}

/// Кількість записів у PG receipts за client_uuid (приймач push).
async fn server_receipt_count(pool: &sqlx::PgPool, client_uuid: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM receipts WHERE client_uuid = $1",
    )
    .bind(Uuid::parse_str(client_uuid).unwrap())
    .fetch_one(pool)
    .await
    .expect("count receipts")
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Ідемпотентність: 2× push одного client_uuid → already_exists, 1 запис
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn push_idempotent_single_server_record() {
    let pool = api_pool().await;
    let product = ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    let dir = tempfile::TempDir::new().expect("tmpdir");
    let (db_path, client_uuid) = new_cash_db(&dir, product, 1);
    let cfg = push_cfg(&db_path, &base, &token);
    let client = reqwest::Client::new();

    // 1-й push: чек створюється на сервері.
    let s1 = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("1-й push (сервер увімкнено)");
    assert_eq!(s1.done, 1, "перший push → created → done");
    assert_eq!(
        server_receipt_count(&pool, &client_uuid).await,
        1,
        "на сервері 1 запис"
    );

    // Симуляція «відповідь загубилась після COMMIT сервера»: каса не знає
    // про успіх → outbox знову pending → повторний push ТОГО САМОГО
    // client_uuid (дизайн 3.3: retry безпечний).
    let conn = open_connection(&db_path).expect("БД");
    conn.execute(
        "UPDATE outbox SET status = 'pending', next_attempt_at = datetime('now')",
        [],
    )
    .expect("скинути у pending");
    drop(conn);

    let s2 = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("2-й push");
    assert_eq!(s2.already_exists, 1, "повторний push → already_exists → done");
    assert_eq!(
        server_receipt_count(&pool, &client_uuid).await,
        1,
        "КРИТЕРІЙ: на сервері РІВНО 1 запис після 2× push"
    );

    // sync_log: ok + already_exists зафіксовані (аудит push).
    let log_statuses: Vec<String> = sqlx::query(
        "SELECT status FROM sync_log WHERE direction = 'push' AND client_uuid = $1 \
         ORDER BY id",
    )
    .bind(Uuid::parse_str(&client_uuid).unwrap())
    .fetch_all(&pool)
    .await
    .expect("sync_log")
    .into_iter()
    .map(|r| r.get::<String, _>(0))
    .collect();
    assert_eq!(log_statuses, vec!["ok".to_string(), "already_exists".to_string()]);

    eprintln!("[sync_push_e2e] ✅ ідемпотентність: 2× push → 1 запис на сервері");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Вимкнений сервер → outbox росте; після запуску — вивантажується FIFO
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn server_down_outbox_grows_then_flushes_fifo() {
    let pool = api_pool().await;
    let product = ensure_seed(&pool).await;

    // Сервер НЕ запускаємо: push має падати з мережевою помилкою.
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let db_path = dir.path().join("cash.db");
    let mut conn = open_connection(&db_path).expect("каса БД");

    // Чеки створюються офлайн (outbox росте), коли сервер вимкнений.
    let mut uuids = Vec::new();
    for n in 1..=3i64 {
        let receipt = json!({
            "receipt_type": "sale",
            "receipt_number": n,
            "items": [{"product_id": product.to_string(), "quantity": 1, "price": "100.00"}],
            "total_amount": "100.00",
            "payment_method": "cash",
            "cash_amount": "100.00",
        })
        .to_string();
        let out = enqueue_receipt(&mut conn, &receipt, Some(STORE1)).expect("enqueue чек");
        uuids.push(out.client_uuid);
    }
    assert_eq!(pending_count(&conn).expect("count"), 3, "outbox росте офлайн");
    drop(conn);
    let client = reqwest::Client::new();

    // Push з вимкненим сервером: мережева помилка → статуси pending БЕЗ змін
    // (дизайн 4.3 «немає мережі» — не backoff).
    let cfg0 = PushConfig {
        base_url: base.clone(),
        token: "неважливо".to_string(),
        store_id: STORE1.to_string(),
        db_path: db_path.clone(),
        interval_secs: 30,
    };
    let err = push_pending_batch(&db_path, &client, &cfg0).await;
    assert!(err.is_err(), "вимкнений сервер → мережева помилка");
    let conn = open_connection(&db_path).expect("БД");
    assert_eq!(pending_count(&conn).expect("count"), 3, "pending без змін (сервер вимкнений)");
    drop(conn);

    // Запускаємо сервер — outbox вивантажується ОДНИМ пакетом FIFO (3 чеки).
    let _h = run_facade(&addr);
    let token = login(&base).await;

    let cfg = push_cfg(&db_path, &base, &token);
    let s = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push після відновлення сервера");
    assert_eq!(s.done, 3, "КРИТЕРІЙ: усі 3 чеки вивантажені FIFO після відновлення");
    let conn = open_connection(&db_path).expect("БД");
    let done_n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'done'",
            [],
            |r| r.get(0),
        )
        .expect("done count");
    assert_eq!(done_n, 3, "outbox: усі done");
    drop(conn);

    // На сервері — рівно 3 записи (по одному на client_uuid, без дублікатів).
    for uuid in &uuids {
        assert_eq!(
            server_receipt_count(&pool, uuid).await,
            1,
            "запис {uuid} на сервері рівно 1"
        );
    }
    eprintln!("[sync_push_e2e] ✅ вимкнений сервер → outbox росте → FIFO-вивантаження після відновлення");
}
