//! E2E: ОПЛАТА БОРГУ ПОКУПЦЯ на standby — адаптер `OutboxDebtors`
//! (ADR-0007 §10, §11.1, §11.6.4 варіант 1, §11.7.9.7; Фаза 3.3b).
//!
//! Рішення NIKO §11.6.4: боргові сутності — клас `LocalOutbox` (не
//! `ProxyToPrimary`), тобто на standby оплата боргу СТАЄ МОЖЛИВОЮ: каса пише
//! документ у SQLite-чергу (`TYPE_DEBTOR_PAYMENT`), а не в read-only репліку.
//! До цього `POST /api/v1/debtors/{id}/pay` давав сирий 500
//! `cannot execute INSERT in a read-only transaction`.
//!
//! ТЕСТ 1 `standby_debtor_payment_queues_locally_and_never_writes_replica`:
//!   репліка фізично read-only → дві оплати (40 + 30 з боргу 100) → **200** з
//!   локально зменшеним боргом (`60.00` → `30.00`); 2 агрегати `debtors_ledger`
//!   (synced=1) + 2 outbox `debtor_payment/pending`; локальний похідний борг
//!   `debtor_balances.pending_cents` = 7000; у PG `debtor_payments` = 0 і
//!   `debtors.total_debt` = `100.00` (репліка не змінилась); перевищення суми →
//!   400; негативний контроль зі старою обв'язкою → 500 без сирого тексту PG (§D).
//!
//! ТЕСТ 2 `standby_debtor_payment_push_idempotent_on_primary`:
//!   оплату створено РЕАЛЬНИМ HTTP-роутом із адаптером → outbox каси → push на
//!   піднятий primary → рівно 1 рядок `debtor_payments` з `client_uuid` каси +
//!   `debtors.total_debt` 100.00 → **60.00**; повторний push → `already_exists`,
//!   борг не подвоєно (partial UNIQUE `uq_debtor_payments_client_uuid`, 0018).

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
use torgashka_infrastructure::repositories::debtors::SqlxDebtors;
use torgashka_infrastructure::repositories::outbox_debtors::OutboxDebtors;
use torgashka_infrastructure::offline::sync_push::{
    open_connection, pending_count, push_pending_batch, PushConfig,
};
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

const SECRET: &str = "debtor-standby-e2e-secret";
const RO_ROLE: &str = "torgashka_debtore2e_ro";
const RO_PASS: &str = "torgashka_debtore2e_ro_pwd";

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

/// Standby-фасад із переданою гілкою боржників (адаптер або стара обв'язка).
fn standby_state(
    debtors: Option<Arc<dyn torgashka_domain::DebtorService + Send + Sync>>,
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
        debtors,
        documents: None,
        documents_pool: None,
        invoices_v1: None,
        invoices_v2: None,
        invoices_pool: None,
        return_invoices: None,
        return_invoices_pool: None,
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

/// Нова обв'язка: `OutboxDebtors` поверх репліки (§11.6.4 варіант 1).
fn outbox_state(pool: sqlx::PgPool) -> AppState {
    standby_state(
        Some(Arc::new(OutboxDebtors::new(Arc::new(SqlxDebtors::new(
            StorePool::new(pool.clone()),
        ))))),
        pool,
    )
}

/// Стара обв'язка (негативний контроль): сервіс пише в read-only репліку.
fn legacy_state(pool: sqlx::PgPool) -> AppState {
    standby_state(
        Some(Arc::new(SqlxDebtors::new(StorePool::new(pool.clone())))),
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

/// Скільки outbox-записів цього типу взагалі (усі client_uuid).
fn outbox_count_kind(kind: &str) -> i64 {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        "SELECT COUNT(*) FROM outbox WHERE type = ?1",
        [kind],
        |r| r.get::<_, i64>(0),
    )
    .expect("COUNT outbox")
}

/// `client_uuid` єдиного outbox-запису типу (для push-перевірок).
fn outbox_single_client_uuid(kind: &str) -> String {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        "SELECT client_uuid FROM outbox WHERE type = ?1 ORDER BY id DESC LIMIT 1",
        [kind],
        |r| r.get::<_, String>(0),
    )
    .expect("client_uuid outbox")
}

/// Локальний похідний борг каси (копійки) — `debtor_balances`.
fn local_pending_debt_cents(debtor: Uuid) -> i64 {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        "SELECT pending_cents FROM debtor_balances WHERE debtor_id = ?1",
        [debtor.to_string()],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
}

/// Скільки рядків `debtor_payments` у PG по точці.
async fn pg_debtor_payments(pool: &sqlx::PgPool, store: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM debtor_payments WHERE store_id = $1")
        .bind(store)
        .fetch_one(pool)
        .await
        .expect("COUNT debtor_payments")
}

/// `total_debt` боржника в PG (numeric → text).
async fn pg_debtor_debt(pool: &sqlx::PgPool, debtor: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT total_debt::text FROM debtors WHERE id = $1")
        .bind(debtor)
        .fetch_optional(pool)
        .await
        .expect("SELECT debtors.total_debt")
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
        "return_invoices",
        "supplier_ledger",
        "debtors_ledger",
        "debtor_balances",
        "supplier_balances",
        "stock",
        "sync_log",
    ] {
        let _ = conn.execute(&format!("DELETE FROM {t}"), []);
    }
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
// ТЕСТ 1: standby — 200 з локально зменшеним боргом, 0 рядків у PG
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn standby_debtor_payment_queues_locally_and_never_writes_replica() {
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
    let debtor = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Debtor Точка') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Debtor Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind(format!("dbt_e2e_{}", &user_id.to_string()[..8]))
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
    // Боржник точки з боргом 100.00 (репліка — джерело для локального відліку).
    sqlx::query(
        "INSERT INTO debtors (id, name, total_debt, created_at, updated_at, store_id) \
         VALUES ($1, 'E2E Боржник', 100.00, now(), now(), $2)",
    )
    .bind(debtor)
    .bind(store)
    .execute(&admin_pool)
    .await
    .expect("INSERT debtors");

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
    eprintln!("[debtor e2e] репліка справді read-only: {ro_err}");

    // ── 2. Standby-фасад з `OutboxDebtors` ─────────────────────────────────
    reset_local_queue();
    seed_sqlite_store_id(store);
    let app = router_v1::build_router(outbox_state(ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    // ── 3. Оплата 40.00 з боргу 100.00 → 200, борг 60.00 ───────────────────
    let (status, dto, raw) = call(
        &app,
        "POST",
        &format!("/api/v1/debtors/{debtor}/pay"),
        &token,
        store,
        json!({"amount": "40.00", "payment_method": "cash"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "оплата на standby: {status} {raw}");
    assert_eq!(
        dto["total_debt"], "60.00",
        "борг у відповіді = 100.00 − 40.00 (оцінка каси): {dto}"
    );

    // ── 4. Друга оплата 30.00 → 200, борг 30.00 ────────────────────────────
    let (status2, dto2, raw2) = call(
        &app,
        "POST",
        &format!("/api/v1/debtors/{debtor}/pay"),
        &token,
        store,
        json!({"amount": "30.00", "payment_method": "card"}),
    )
    .await;
    assert_eq!(status2, StatusCode::OK, "друга оплата: {status2} {raw2}");
    assert_eq!(dto2["total_debt"], "30.00", "борг 60.00 − 30.00: {dto2}");

    // ── 5. Локальна черга: 2 агрегати + 2 outbox + похідний борг ───────────
    assert_eq!(
        outbox_count_kind("debtor_payment"),
        2,
        "дві оплати → два outbox-записи типу debtor_payment"
    );
    assert_eq!(
        local_pending_debt_cents(debtor),
        7_000,
        "локальний похідний борг каси = 70.00 (40 + 30)"
    );
    let (agg_synced, agg_data): (i64, String) = {
        let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
        conn.query_row(
            "SELECT synced, data FROM debtors_ledger ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("агрегат debtors_ledger")
    };
    assert_eq!(agg_synced, 1, "агрегат каси — push-кандидат");
    assert!(agg_data.contains("30.00"), "data = payload як є: {agg_data}");

    // ── 6. У PG — НУЛЬ оплат, борг не змінився ─────────────────────────────
    assert_eq!(
        pg_debtor_payments(&admin_pool, store).await,
        0,
        "на standby оплата НЕ пише в репліку"
    );
    assert_eq!(
        pg_debtor_debt(&admin_pool, debtor).await.as_deref(),
        Some("100.00"),
        "борг у репліці не змінився"
    );

    // ── 7. Перевищення суми → 400 (людський текст) ─────────────────────────
    let (over_status, over_dto, over_raw) = call(
        &app,
        "POST",
        &format!("/api/v1/debtors/{debtor}/pay"),
        &token,
        store,
        json!({"amount": "999.00", "payment_method": "cash"}),
    )
    .await;
    assert_eq!(
        over_status,
        StatusCode::BAD_REQUEST,
        "сума більша за локальний борг мусить дати 400: {over_status} {over_raw}"
    );
    assert!(
        over_dto["detail"].as_str().unwrap_or_default().contains("перевищує поточний борг"),
        "людський текст відмови: {over_dto}"
    );
    assert_eq!(outbox_count_kind("debtor_payment"), 2, "відмова не створила документ");

    // ── 8. НЕГАТИВНИЙ КОНТРОЛЬ: read-only БД + СТАРА обв'язка (§11.6.4) ────
    let old_app = router_v1::build_router(legacy_state(ro_pool.clone()));
    let (old_status, _, old_raw) = call(
        &old_app,
        "POST",
        &format!("/api/v1/debtors/{debtor}/pay"),
        &token,
        store,
        json!({"amount": "10.00", "payment_method": "cash"}),
    )
    .await;
    assert_eq!(
        old_status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "стара обв'язка на read-only репліці мусить дати 500, маємо {old_status}: {old_raw}"
    );
    for banned in ["read-only transaction", "UPDATE", "INSERT INTO", "sqlx"] {
        assert!(
            !old_raw.contains(banned),
            "у тілі 500 немає сирого тексту PG/драйвера ('{banned}'): {old_raw}"
        );
    }
    eprintln!("[debtor e2e] негативний контроль: {old_status} {old_raw}");
    assert_eq!(
        local_pending_debt_cents(debtor),
        7_000,
        "локальний борг не змінився негативним контролем"
    );
    eprintln!("[debtor_standby_outbox_e2e] ✅ ТЕСТ 1: 2 оплати → 200, 2 pending, 0 рядків PG");
}

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ 2: push на primary — ідемпотентно (борг не подвоюється)
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
async fn standby_debtor_payment_push_idempotent_on_primary() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url_pub(&db_url);
    ensure_readonly_role(&pool, &db_name).await;

    // ── Каталог на «primary»: точка, адмін, боржник 100.00 ────────────────
    let store = Uuid::new_v4();
    let debtor = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Debtor Push Точка') ON CONFLICT (id) DO NOTHING")
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
    sqlx::query(
        "INSERT INTO debtors (id, name, total_debt, created_at, updated_at, store_id) \
         VALUES ($1, 'E2E Push Боржник', 100.00, now(), now(), $2)",
    )
    .bind(debtor)
    .bind(store)
    .execute(&pool)
    .await
    .expect("seed debtor");

    // ── Каса (standby): оплата створюється ЧЕРЕЗ HTTP-роут із адаптером ────
    reset_local_queue();
    seed_sqlite_store_id(store);
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
        &format!("/api/v1/debtors/{debtor}/pay"),
        &token,
        store,
        json!({"amount": "40.00", "payment_method": "cash"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "каса: {status} {raw}");
    assert_eq!(dto["total_debt"], "60.00", "борг каси: {dto}");
    let cu = outbox_single_client_uuid("debtor_payment");
    let db_path = offline_db_path();
    {
        let conn = open_connection(&db_path).expect("SQLite каси");
        assert_eq!(pending_count(&conn).expect("pending"), 1, "1 документ у черзі");
    }

    // ── Сервер піднято → push каси ────────────────────────────────────────
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
    eprintln!("[debtor e2e] перший push: {s1:?}");
    assert_eq!(s1.done, 1, "перший push → created (payload адаптера прийнято)");
    assert_eq!(s1.failed, 0, "помилок немає: {s1:?}");
    assert_eq!(s1.already_exists, 0, "перший push — не дублікат");

    // ── Серверний стан: 1 оплата з client_uuid каси, борг 60.00 ───────────
    let (srv_store, srv_amount, srv_method): (Uuid, String, Option<String>) = sqlx::query_as(
        "SELECT store_id, amount::text, payment_method FROM debtor_payments WHERE client_uuid = $1",
    )
    .bind(Uuid::parse_str(&cu).expect("uuid"))
    .fetch_one(&pool)
    .await
    .expect("рядок debtor_payments на primary");
    assert_eq!(srv_store, store);
    assert_eq!(srv_amount, "40.00");
    assert_eq!(srv_method.as_deref(), Some("cash"));
    assert_eq!(pg_debtor_payments(&pool, store).await, 1, "рівно 1 оплата");
    assert_eq!(
        pg_debtor_debt(&pool, debtor).await.as_deref(),
        Some("60.00"),
        "борг на primary 100.00 → 60.00 (один ефект)"
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
    eprintln!("[debtor e2e] повторний push: {s2:?}");
    assert_eq!(s2.already_exists, 1, "повторний push → already_exists");
    assert_eq!(s2.done, 0, "нового created немає");
    assert_eq!(s2.failed, 0, "помилок немає");
    assert_eq!(
        pg_debtor_payments(&pool, store).await,
        1,
        "дублів немає (partial UNIQUE uq_debtor_payments_client_uuid, 0018)"
    );
    assert_eq!(
        pg_debtor_debt(&pool, debtor).await.as_deref(),
        Some("60.00"),
        "повторний push не подвоїв борг-ефект"
    );
    eprintln!("[debtor_standby_outbox_e2e] ✅ ТЕСТ 2: created → already_exists, 1 рядок, борг 60.00");
}
