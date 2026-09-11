//! E2E: ПОВЕРНЕННЯ ПОСТАЧАЛЬНИКУ на standby — адаптер `OutboxReturnInvoices`
//! (ADR-0007 §10, §11.1, §11.7.9.7; Фаза 3.3b).
//!
//! До Фази 3.3b сутність була **АНОМАЛІЄЮ**: `state.return_invoices` =
//! `SqlxReturnInvoices(репліка)`, тому `POST /api/v1/return-invoices` падав з
//! PG-помилкою `cannot execute INSERT in a read-only transaction` → сирий 500,
//! і навіть типу черги для повернення постачальнику не існувало
//! (`TYPE_RETURN_RECEIPT` — це чек повернення ПОКУПЦЯ → `receipts`).
//! Тепер: тип `return_invoice` + локальний агрегат `return_invoices`
//! (offline-міграція 0012) + SQLite-ефект **−qty** + приймач на primary
//! (сервісний, як `invoice`) + partial UNIQUE `uq_return_invoices_client_uuid`
//! (Alembic 0018).
//!
//! ТЕСТ 1 `standby_return_invoice_queues_locally_and_never_writes_replica`:
//!   репліка фізично read-only → `POST /api/v1/return-invoices` → **201** зі
//!   `status:"queued"`; локальний stock 10000 → **7000** (−3.000); агрегат
//!   `return_invoices` (synced=1) + outbox `return_invoice/pending`; 0 рядків у
//!   PG `return_invoices`; негативний контроль зі старою обв'язкою → 500 без
//!   сирого тексту PG (§D).
//!
//! ТЕСТ 2 `standby_return_invoice_payload_accepted_by_primary_receiver`:
//!   документ створено РЕАЛЬНИМ HTTP-роутом із адаптером → outbox каси → push
//!   на піднятий primary (`TORGASHKA_RUST_RETURN_INVOICES=1`) → рівно 1 рядок
//!   `return_invoices` з `client_uuid` каси, `store_id`, `supplier_id`,
//!   `number`, статус `confirmed` + серверний stock 10.000 → **7.000**;
//!   повторний push → `already_exists` без другого stock-ефекту.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use torgashka_api::auth::create_access_token;
use torgashka_api::{router_v1, AppState};
use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use torgashka_infrastructure::repositories::outbox_return_invoices::OutboxReturnInvoices;
use torgashka_infrastructure::repositories::return_invoices::SqlxReturnInvoices;
use torgashka_infrastructure::offline::sync_push::{
    open_connection, pending_count, push_pending_batch, PushConfig,
};
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

const SECRET: &str = "return_invoice-standby-e2e-secret";
const RO_ROLE: &str = "torgashka_return_invoicee2e_ro";
const RO_PASS: &str = "torgashka_return_invoicee2e_ro_pwd";

/// Тести бінаря ділять один SQLite каси (XDG): серіалізуємо, щоб push одного
/// не забирав чергу іншого.
static SEQ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ─────────────────────────────────────────────────────────────────────────────
// Інфраструктура тесту (той самий контур, що cash_operation_standby_e2e)
// ─────────────────────────────────────────────────────────────────────────────

fn isolate_sqlite() -> &'static std::path::Path {
    static ONCE: std::sync::Once = std::sync::Once::new();
    static mut DIR: Option<&'static std::path::Path> = None;
    ONCE.call_once(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let leaked: &'static std::path::Path = Box::leak(dir.keep().into_boxed_path());
        std::env::set_var("XDG_DATA_HOME", leaked);
        unsafe { DIR = Some(leaked) };
    });
    unsafe { DIR.expect("XDG_DATA_HOME") }
}

fn offline_db_path() -> PathBuf {
    torgashka_infrastructure::offline::db::OfflineDatabase::default_db_path().expect("шлях SQLite")
}

fn seed_sqlite_store_id(store: Uuid) {
    let path = offline_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("каталог даних каси");
    }
    let conn = torgashka_infrastructure::offline::sync_push::open_connection(&path)
        .expect("SQLite каси + міграції");
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('store_id', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [store.to_string()],
    )
    .expect("settings.store_id");
}

fn swap_credentials(url: &str, user: &str, pass: &str) -> String {
    let scheme_end = url.find("://").map(|i| i + 3).expect("схема URL");
    let after = &url[scheme_end..];
    let host_start = after.find('@').map(|i| i + 1).unwrap_or(0);
    format!("{}{}:{}@{}", &url[..scheme_end], user, pass, &after[host_start..])
}

fn db_name_from_url(url: &str) -> String {
    let before = url.split('?').next().unwrap_or(url);
    before[before.rfind('/').expect("слеш") + 1..].to_string()
}

static ROLE_SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn ensure_readonly_role(pool: &sqlx::PgPool, db: &str) {
    let _guard = ROLE_SETUP.lock().await;
    sqlx::query(&format!(
        "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{RO_ROLE}') \
         THEN CREATE ROLE {RO_ROLE} LOGIN PASSWORD '{RO_PASS}'; END IF; END $$;"
    ))
    .execute(pool)
    .await
    .expect("CREATE ROLE (read-only роль)");
    sqlx::query(&format!(
        "ALTER ROLE {RO_ROLE} SET default_transaction_read_only = on"
    ))
    .execute(pool)
    .await
    .expect("ALTER ROLE read_only");
    sqlx::query(&format!("GRANT CONNECT ON DATABASE \"{db}\" TO {RO_ROLE}"))
        .execute(pool)
        .await
        .expect("GRANT CONNECT");
    sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {RO_ROLE}"))
        .execute(pool)
        .await
        .expect("GRANT USAGE");
    sqlx::query(&format!(
        "GRANT SELECT ON ALL TABLES IN SCHEMA public TO {RO_ROLE}"
    ))
    .execute(pool)
    .await
    .expect("GRANT SELECT");
}

/// Standby-фасад із переданою гілкою повернень постачальнику (адаптер або
/// стара обв'язка на репліці).
fn standby_state(
    return_invoices: Option<Arc<dyn torgashka_domain::return_invoices::ReturnInvoicesService + Send + Sync>>,
    pool: sqlx::PgPool,
) -> AppState {
    AppState {
        jwt_secret: Arc::new(SECRET.to_string()),
        readdirs: None,
        write: None,
        write_pool: None,
        pos: None,
        ledger: None,
        auth: None,
        prro: None,
        debtors: None,
        documents: None,
        documents_pool: None,
        invoices_v1: None,
        invoices_v2: None,
        invoices_pool: None,
        return_invoices,
        return_invoices_pool: Some(pool.clone()),
        purchase_orders: None,
        purchase_orders_pool: None,
        print_templates: None,
        print_pool: None,
        products_v2: None,
        products_v2_pool: None,
        ocr: None,
        ocr_pool: None,
        uploads_dir: std::path::PathBuf::from("uploads"),
        store_pool: Some(StorePool::new(pool)),
        stores: None,
        setup: None,
        node_config: NodeConfig {
            mode: NodeMode::Standby,
            ..NodeConfig::default()
        },
        local: None,
    }
}

/// Нова обв'язка: `OutboxReturnInvoices` поверх репліки (§11.7.9.7).
fn outbox_state(pool: sqlx::PgPool) -> AppState {
    standby_state(
        Some(Arc::new(OutboxReturnInvoices::new(Arc::new(
            SqlxReturnInvoices::new(StorePool::new(pool.clone())),
        )))),
        pool,
    )
}

/// Стара обв'язка (негативний контроль): сервіс пише в read-only репліку.
fn legacy_state(pool: sqlx::PgPool) -> AppState {
    standby_state(
        Some(Arc::new(SqlxReturnInvoices::new(StorePool::new(pool.clone())))),
        pool,
    )
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    store: Uuid,
    body: Value,
) -> (StatusCode, Value, String) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-store-id", store.to_string())
        .body(Body::from(body.to_string()))
        .expect("запит");
    let resp = app.clone().oneshot(req).await.expect("відповідь");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("тіло");
    let raw = String::from_utf8_lossy(&bytes).to_string();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, raw)
}

/// Скільки outbox-записів цього типу й `client_uuid` у черзі.
fn outbox_count_for(kind: &str, client_uuid: &str) -> i64 {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        "SELECT COUNT(*) FROM outbox WHERE type = ?1 AND payload LIKE ?2",
        rusqlite::params![kind, format!("%{client_uuid}%")],
        |r| r.get::<_, i64>(0),
    )
    .expect("COUNT outbox")
}

/// Агрегат локальної черги: `(synced, data)` за `client_uuid`.
fn local_aggregate_row(table: &str, client_uuid: &str) -> Option<(i64, String)> {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        &format!("SELECT synced, data FROM {table} WHERE client_uuid = ?1"),
        [client_uuid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

/// Локальний stock каси (мілі-одиниці) — прямий SQL до SQLite.
fn local_stock_milli(product: Uuid) -> i64 {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        "SELECT quantity FROM stock WHERE product_id = ?1",
        [product.to_string()],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
}

/// Очищення локальної черги/агрегатів перед push-тестом (тестова ізоляція
/// у спільному XDG-файлі каси).
fn reset_local_queue() {
    let path = offline_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("каталог даних каси");
    }
    // `open_connection` створює файл і доганяє міграції (як у адаптерів).
    let conn = open_connection(&path).expect("SQLite каси + міграції");
    for t in [
        "outbox",
        "invoices",
        "invoice_items",
        "stock",
        "sync_log",
    ] {
        let _ = conn.execute(&format!("DELETE FROM {t}"), []);
    }
}

/// Локальний залишок каси: записати рівень (мілі-одиниці) — щоб ефект
/// повернення (−qty) було видно як ЗМЕНШЕННЯ, а не як від'ємний старт.
fn seed_local_stock(product: Uuid, milli: i64) {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    let store: String = conn
        .query_row("SELECT value FROM settings WHERE key = 'store_id'", [], |r| r.get(0))
        .expect("settings.store_id");
    conn.execute(
        "INSERT INTO stock (store_id, product_id, quantity) VALUES (?1, ?2, ?3) \
         ON CONFLICT (store_id, product_id) DO UPDATE SET quantity = excluded.quantity",
        rusqlite::params![store, product.to_string(), milli],
    )
    .expect("seed local stock");
}

async fn pg_return_invoice_rows(pool: &sqlx::PgPool, store: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM return_invoices WHERE store_id = $1")
        .bind(store)
        .fetch_one(pool)
        .await
        .expect("COUNT invoices")
}

/// Серверний stock PG по точці й товару (numeric → text).
async fn pg_stock(pool: &sqlx::PgPool, store: Uuid, product: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2")
        .bind(store)
        .bind(product)
        .fetch_optional(pool)
        .await
        .expect("SELECT stock")
}

async fn api_pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_readonly_pool(3)
        .await
        .expect("writable-пул тестової БД")
}

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

fn db_name_from_url_pub(url: &str) -> String {
    db_name_from_url(url)
}

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ 1: standby — 201/queued, локальна черга, 0 рядків у PG
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn standby_return_invoice_queues_locally_and_never_writes_replica() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let admin_pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url_pub(&db_url);
    ensure_readonly_role(&admin_pool, &db_name).await;

    let store = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Return Точка') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Return Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind(format!("ret_e2e_{}", &user_id.to_string()[..8]))
    .execute(&admin_pool)
    .await
    .expect("INSERT users");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at) \
         VALUES ($1, $2, 'admin', '{}'::jsonb, true, now()) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(store)
    .execute(&admin_pool)
    .await
    .expect("INSERT user_stores");

    // ── 1. Доказ: репліка справді read-only ────────────────────────────────
    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&swap_credentials(&db_url, RO_ROLE, RO_PASS))
        .await
        .expect("read-only пул репліки");
    let ro_err = sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'RO-заборонено')")
        .bind(Uuid::new_v4())
        .execute(&ro_pool)
        .await
        .expect_err("репліка мусить бути read-only");
    assert!(
        ro_err.to_string().contains("read-only transaction"),
        "роль репліки не read-only: {ro_err}"
    );
    eprintln!("[return e2e] репліка справді read-only: {ro_err}");

    // ── 2. Standby-фасад з `OutboxReturnInvoices` ──────────────────────────
    reset_local_queue();
    seed_sqlite_store_id(store);
    let app = router_v1::build_router(outbox_state(ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");
    let product = Uuid::new_v4();
    let supplier = Uuid::new_v4();
    // Каса має 10 одиниць товару — повернення постачальнику спише 3.
    seed_local_stock(product, 10_000);

    // ── 3. Повернення постачальнику: 3 × 100 = 300 → 201/queued ────────────
    let (status, dto, raw) = call(
        &app,
        "POST",
        "/api/v1/return-invoices",
        &token,
        store,
        json!({
            "number": "RV-Q-1",
            "supplier_id": supplier.to_string(),
            "return_date": "2026-09-02T10:00:00",
            "return_action": "deduct_from_debt",
            "is_fiscal": false,
            "notes": "standby return invoice e2e",
            "total_amount": "300.00",
            "items": [{
                "product_id": product.to_string(),
                "quantity": "3.000",
                "price": "100.00",
                "total": "300.00"
            }]
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "повернення на standby має створитися локально (201/queued), маємо {status}: {raw}"
    );
    assert_eq!(dto["status"], "queued", "бізнес-статус черги: {dto}");
    assert_eq!(dto["number"], "RV-Q-1", "{dto}");
    let cu = dto["id"].as_str().expect("client_uuid").to_string();
    let (synced, data) = local_aggregate_row("return_invoices", &cu).expect("агрегат return_invoices");
    assert_eq!(synced, 1, "агрегат каси — push-кандидат");
    assert!(data.contains("RV-Q-1"), "data = payload як є: {data}");
    assert_eq!(
        outbox_count_for("return_invoice", &cu),
        1,
        "рівно один outbox-запис типу return_invoice"
    );
    assert_eq!(
        local_stock_milli(product),
        7_000,
        "локальний stock 10000 → 7000 (списання −3.000 за поверненням)"
    );

    // ── 4. У PG — НУЛЬ рядків ──────────────────────────────────────────────
    assert_eq!(
        pg_return_invoice_rows(&admin_pool, store).await,
        0,
        "на standby повернення НЕ пише в репліку"
    );

    // ── 5. НЕГАТИВНИЙ КОНТРОЛЬ: read-only БД + СТАРА обв'язка (§11.7.9.7) ──
    let old_app = router_v1::build_router(legacy_state(ro_pool.clone()));
    let (old_status, _, old_raw) = call(
        &old_app,
        "POST",
        "/api/v1/return-invoices",
        &token,
        store,
        json!({
            "number": "RV-Q-OLD",
            "supplier_id": supplier.to_string(),
            "return_date": "2026-09-02T11:00:00",
            "return_action": "deduct_from_debt",
            "is_fiscal": false,
            "total_amount": "100.00",
            "items": [{
                "product_id": product.to_string(),
                "quantity": "1.000",
                "price": "100.00",
                "total": "100.00"
            }]
        }),
    )
    .await;
    assert_eq!(
        old_status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "стара обв'язка на read-only репліці мусить дати 500, маємо {old_status}: {old_raw}"
    );
    for banned in ["read-only transaction", "INSERT INTO", "sqlx"] {
        assert!(
            !old_raw.contains(banned),
            "у тілі 500 немає сирого тексту PG/драйвера ('{banned}'): {old_raw}"
        );
    }
    eprintln!("[return e2e] негативний контроль: {old_status} {old_raw}");
    assert_eq!(
        outbox_count_for("return_invoice", "RV-Q-OLD"),
        0,
        "негативний контроль не додав outbox-запису"
    );
    assert_eq!(
        local_stock_milli(product),
        7_000,
        "локальний stock не змінився негативним контролем"
    );
    eprintln!("[return_invoice_standby_outbox_e2e] ✅ ТЕСТ 1: 201/queued, 1 pending, 0 рядків PG");
}

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ 2: payload АДАПТЕРА приймається приймачем primary (ідемпотентно)
// ─────────────────────────────────────────────────────────────────────────────

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

#[tokio::test]
async fn standby_return_invoice_payload_accepted_by_primary_receiver() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url_pub(&db_url);
    ensure_readonly_role(&pool, &db_name).await;

    // ── Каталог на «primary»: точка, адмін, постачальник, товар + залишок ──
    let store = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Return Push Точка') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&pool)
        .await
        .expect("INSERT stores");
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
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, $1, 'owner', '{}'::jsonb, true, now() FROM users u WHERE u.login = 'admin'
         ON CONFLICT DO NOTHING",
    )
    .bind(store)
    .execute(&pool)
    .await
    .expect("seed user_stores");
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'admin'")
        .fetch_one(&pool)
        .await
        .expect("admin id");
    let supplier = Uuid::new_v4();
    sqlx::query("INSERT INTO suppliers (id, name) VALUES ($1, 'E2E Return Push Постачальник')")
        .bind(supplier)
        .execute(&pool)
        .await
        .expect("seed supplier");
    let product = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E Return Push Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(&pool)
    .await
    .expect("seed product");
    // Серверний залишок 10 одиниць — повернення спише 3 (confirm перевіряє достатність).
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, updated_at) VALUES ($1, $2, 10.000, now()) \
         ON CONFLICT (store_id, product_id) DO UPDATE SET quantity = 10.000",
    )
    .bind(store)
    .bind(product)
    .execute(&pool)
    .await
    .expect("seed stock");

    // ── Каса (standby): документ створюється ЧЕРЕЗ HTTP-роут із адаптером ──
    reset_local_queue();
    seed_sqlite_store_id(store);
    seed_local_stock(product, 10_000);
    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&swap_credentials(&db_url, RO_ROLE, RO_PASS))
        .await
        .expect("read-only пул репліки");
    let app = router_v1::build_router(outbox_state(ro_pool.clone()));
    let token = create_access_token(&admin_id.to_string(), "admin", &[], SECRET).expect("JWT");
    let (status, dto, raw) = call(
        &app,
        "POST",
        "/api/v1/return-invoices",
        &token,
        store,
        json!({
            "number": "RV-PUSH-1",
            "supplier_id": supplier.to_string(),
            "return_date": "2026-09-02T10:00:00",
            "return_action": "deduct_from_debt",
            "is_fiscal": false,
            "notes": "офлайн-повернення з адаптера (e2e)",
            "total_amount": "300.00",
            "items": [{
                "product_id": product.to_string(),
                "quantity": "3.000",
                "price": "100.00",
                "total": "300.00"
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "каса: {status} {raw}");
    let cu = Uuid::parse_str(dto["id"].as_str().expect("client_uuid")).expect("uuid");
    let db_path = offline_db_path();
    {
        let conn = open_connection(&db_path).expect("SQLite каси");
        assert_eq!(pending_count(&conn).expect("pending"), 1, "1 документ у черзі");
    }

    // ── Сервер піднято (Rust-гілка повернень УВІМКНЕНА) → push каси ───────
    std::env::set_var(torgashka_api::RUST_RETURN_INVOICES_ENV, "1");
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = torgashka_api::run_facade(&addr);
    let login_token = login(&base).await;
    let client = reqwest::Client::new();
    let cfg = PushConfig {
        base_url: base.clone(),
        token: login_token.clone(),
        store_id: Some(store.to_string()),
        db_path: db_path.clone(),
        interval_secs: 30,
    };
    let s1 = push_pending_batch(&db_path, &client, &cfg).await.expect("push");
    eprintln!("[return e2e] перший push: {s1:?}");
    assert_eq!(s1.done, 1, "перший push → created (payload адаптера прийнято)");
    assert_eq!(s1.failed, 0, "помилок немає: {s1:?}");
    assert_eq!(s1.already_exists, 0, "перший push — не дублікат");

    // ── Серверний стан: 1 повернення з client_uuid каси, stock −3 ─────────
    let (srv_store, srv_supplier, srv_number, srv_status): (Uuid, Uuid, String, String) =
        sqlx::query_as(
            "SELECT store_id, supplier_id, number, status::text FROM return_invoices WHERE client_uuid = $1",
        )
        .bind(cu)
        .fetch_one(&pool)
        .await
        .expect("рядок return_invoices на primary");
    assert_eq!(srv_store, store);
    assert_eq!(srv_supplier, supplier);
    assert_eq!(srv_number, "RV-PUSH-1");
    assert_eq!(srv_status, "confirmed", "приймач проводить повернення одразу");
    assert_eq!(pg_return_invoice_rows(&pool, store).await, 1, "рівно 1 повернення");
    assert_eq!(
        pg_stock(&pool, store, product).await.as_deref(),
        Some("7.000"),
        "серверний stock 10.000 → 7.000 (один ефект)"
    );

    // ── Повторний push (done→pending) → already_exists, дубля немає ────────
    {
        let conn = open_connection(&db_path).expect("БД");
        conn.execute(
            "UPDATE outbox SET status = 'pending', next_attempt_at = datetime('now') \
             WHERE status = 'done'",
            [],
        )
        .expect("reset done→pending");
    }
    let s2 = push_pending_batch(&db_path, &client, &cfg).await.expect("push");
    eprintln!("[return e2e] повторний push: {s2:?}");
    assert_eq!(s2.already_exists, 1, "повторний push → already_exists");
    assert_eq!(s2.done, 0, "нового created немає");
    assert_eq!(s2.failed, 0, "помилок немає");
    assert_eq!(pg_return_invoice_rows(&pool, store).await, 1, "дублів немає (UNIQUE 0018)");
    assert_eq!(
        pg_stock(&pool, store, product).await.as_deref(),
        Some("7.000"),
        "повторний push не подвоїв stock-ефект"
    );
    eprintln!("[return_invoice_standby_outbox_e2e] ✅ ТЕСТ 2: created → already_exists, 1 рядок, 0 дублів");
}
