//! E2E: ПРИБУТКОВА НАКЛАДНА на standby — адаптери `OutboxInvoicesV1/V2`
//! (ADR-0007 §10, §11.1, §11.7.9.7; Фаза 3.3a).
//!
//! До виправлення: на standby `state.invoices_v1/v2` = `SqlxInvoices(репліка)`,
//! тому `POST /api/v1/invoices` / `POST /api/v2/invoices` падали з PG-помилкою
//! `cannot execute INSERT in a read-only transaction` → сирий **500**.
//!
//! ТЕСТ 1 `standby_invoice_queues_locally_and_never_writes_replica`:
//!   репліка фізично read-only → `POST /api/v1/invoices` (і `/api/v2/invoices`)
//!   → **201** з бізнес-статусом `queued`; локальний stock +3.000/+2.000;
//!   0 рядків у PG `invoices`; агрегат `invoices` (synced=1) + outbox `pending`;
//!   негативний контроль зі старою обв'язкою → 500 без сирого тексту PG (§D).
//!
//! ТЕСТ 2 `standby_invoice_payload_accepted_by_primary_receiver`:
//!   накладна створюється РЕАЛЬНИМ HTTP-роутом із адаптером → outbox каси →
//!   push-цикл на піднятий primary (Rust-гілка інвойсів увімкнена) → рівно
//!   1 рядок `invoices` з `client_uuid` каси, `store_id`, `supplier_id`,
//!   `number`, `confirmed` + stock +3.000; повторний push → `already_exists`
//!   без другого stock-ефекту (partial UNIQUE 0016).

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
use torgashka_infrastructure::repositories::invoices::SqlxInvoices;
use torgashka_infrastructure::repositories::outbox_invoices::{OutboxInvoicesV1, OutboxInvoicesV2};
use torgashka_infrastructure::offline::sync_push::{
    open_connection, pending_count, push_pending_batch, PushConfig, PushSummary,
};
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

const SECRET: &str = "invoice-standby-e2e-secret";
const RO_ROLE: &str = "torgashka_invce2e_ro";
const RO_PASS: &str = "torgashka_invce2e_ro_pwd";

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

/// Standby-фасад із переданими гілками інвойсів (нова або стара обв'язка).
fn standby_state(
    v1: Arc<dyn torgashka_domain::InvoicesV1Service + Send + Sync>,
    v2: Arc<dyn torgashka_domain::InvoicesV2Service + Send + Sync>,
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
        invoices_v1: Some(v1),
        invoices_v2: Some(v2),
        invoices_pool: Some(pool.clone()),
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

/// Стара обв'язка (для негативного контролю): обидві гілки на репліці.
fn legacy_state(pool: sqlx::PgPool) -> AppState {
    standby_state(
        Arc::new(SqlxInvoices::new(StorePool::new(pool.clone()))),
        Arc::new(SqlxInvoices::new(StorePool::new(pool.clone()))),
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

/// Рядки outbox: `(type, status, count)`.
fn outbox_rows() -> Vec<(String, String, i64)> {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    let mut stmt = conn
        .prepare("SELECT type, status, COUNT(*) FROM outbox GROUP BY type, status ORDER BY type")
        .expect("prepare");
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    rows
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

async fn pg_invoice_rows(pool: &sqlx::PgPool, store: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE store_id = $1")
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
// ТЕСТ 1: standby — 201/queued (v1 + v2), локальна черга, 0 рядків у PG
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn standby_invoice_queues_locally_and_never_writes_replica() {
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
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Invoice Точка') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Invoice Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind(format!("inv_e2e_{}", &user_id.to_string()[..8]))
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
    eprintln!("[invoice e2e] репліка справді read-only: {ro_err}");

    // ── 2. Standby-фасад із OutboxInvoicesV1/V2 (нова обв'язка) ────────────
    // Чиста база: тести бінаря ділять один XDG-файл каси (див. SEQ).
    reset_local_queue();
    seed_sqlite_store_id(store);
    let app = router_v1::build_router(standby_state(
        Arc::new(OutboxInvoicesV1::new(Arc::new(SqlxInvoices::new(
            StorePool::new(ro_pool.clone()),
        )))),
        Arc::new(OutboxInvoicesV2::new(Arc::new(SqlxInvoices::new(
            StorePool::new(ro_pool.clone()),
        )))),
        ro_pool.clone(),
    ));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");
    let product = Uuid::new_v4();
    let supplier = Uuid::new_v4();

    // ── 3. v1-накладна: 3 шт × 100 = 300 → 201/queued ──────────────────────
    let (status, dto, raw) = call(
        &app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        json!({
            "number": "INV-Q-V1",
            "supplier_id": supplier.to_string(),
            "invoice_date": "2026-09-01T10:00:00",
            "payment_method": null,
            "is_fiscal": false,
            "notes": "standby invoice v1 e2e",
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
        "v1-накладна на standby має створитися локально (201/queued), маємо {status}: {raw}"
    );
    assert_eq!(dto["status"], "queued", "бізнес-статус черги: {dto}");
    assert_eq!(dto["number"], "INV-Q-V1", "{dto}");
    let cu_v1 = dto["id"].as_str().expect("client_uuid v1").to_string();
    let (synced, data) = local_aggregate_row("invoices", &cu_v1).expect("агрегат invoices");
    assert_eq!(synced, 1, "агрегат каси — push-кандидат");
    assert!(data.contains("INV-Q-V1"), "data = payload як є: {data}");
    assert_eq!(
        outbox_count_for("invoice", &cu_v1),
        1,
        "рівно один outbox-запис типу invoice для цього client_uuid"
    );
    assert_eq!(
        local_stock_milli(product),
        3_000,
        "локальний stock = +3.000 (прихід за накладною) у тій самій транзакції"
    );

    // ── 4. v2-накладна (інший DTO) → теж у чергу ───────────────────────────
    let (status2, dto2, raw2) = call(
        &app,
        "POST",
        "/api/v2/invoices",
        &token,
        store,
        json!({
            "number": "INV-Q-V2",
            "supplier_id": supplier.to_string(),
            "notes": "standby invoice v2 e2e",
            "items": [{
                "product_id": product.to_string(),
                "quantity": 2.0,
                "price": 50.0,
                "tax_rate": 20,
                "name": "Товар v2"
            }]
        }),
    )
    .await;
    assert_eq!(
        status2,
        StatusCode::CREATED,
        "v2-накладна на standby має створитися локально (201/queued), маємо {status2}: {raw2}"
    );
    assert_eq!(dto2["status"], "queued", "бізнес-статус черги v2: {dto2}");
    let cu_v2 = dto2["id"].as_str().expect("client_uuid v2").to_string();
    assert_eq!(outbox_count_for("invoice", &cu_v2), 1, "v2 → один outbox-запис");
    assert_eq!(
        local_stock_milli(product),
        5_000,
        "локальний stock = +2.000 від v2-накладної (3.000 + 2.000)"
    );

    // ── 5. У PG — НУЛЬ рядків ──────────────────────────────────────────────
    assert_eq!(
        pg_invoice_rows(&admin_pool, store).await,
        0,
        "на standby накладна НЕ пише в репліку"
    );

    // ── 6. НЕГАТИВНИЙ КОНТРОЛЬ: та сама read-only БД + СТАРА обв'язка ──────
    let old_app = router_v1::build_router(legacy_state(ro_pool.clone()));
    let (old_status, _, old_raw) = call(
        &old_app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        json!({
            "number": "INV-Q-OLD",
            "supplier_id": supplier.to_string(),
            "invoice_date": "2026-09-01T11:00:00",
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
    eprintln!("[invoice e2e] негативний контроль: {old_status} {old_raw}");
    assert_eq!(
        pg_invoice_rows(&admin_pool, store).await,
        0,
        "негативний контроль нічого не записав у PG"
    );
    assert_eq!(
        outbox_count_for("invoice", "INV-Q-OLD"),
        0,
        "негативний контроль не додав жодного outbox-запису"
    );
    assert_eq!(
        outbox_rows()
            .into_iter()
            .filter(|(ty, st, _)| ty == "invoice" && st == "pending")
            .map(|(_, _, c)| c)
            .sum::<i64>(),
        2,
        "у черзі рівно 2 pending-накладні каси (v1 + v2): {:?}",
        outbox_rows()
    );
    assert_eq!(
        local_stock_milli(product),
        5_000,
        "локальний stock не змінився негативним контролем"
    );
    eprintln!("[invoice_standby_outbox_e2e] ✅ ТЕСТ 1: v1+v2 → 201/queued, 2 pending, 0 рядків PG");
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

async fn push_once(db: &std::path::Path, client: &reqwest::Client, cfg: &PushConfig) -> PushSummary {
    push_pending_batch(db, client, cfg).await.expect("push")
}

#[tokio::test]
async fn standby_invoice_payload_accepted_by_primary_receiver() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url_pub(&db_url);
    ensure_readonly_role(&pool, &db_name).await;

    // ── Каталог на «primary»: точка, адмін (login/password), постачальник,
    //    товар — приймач накладної робить pre-flight по каталогу (§9).
    let store = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Invoice Push Точка') ON CONFLICT (id) DO NOTHING")
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
    sqlx::query("INSERT INTO suppliers (id, name) VALUES ($1, 'E2E Invoice Push Постачальник')")
        .bind(supplier)
        .execute(&pool)
        .await
        .expect("seed supplier");
    let product = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E Invoice Push Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(&pool)
    .await
    .expect("seed product");

    // ── Каса (standby): накладна створюється ЧЕРЕЗ HTTP-роут із адаптером ──
    reset_local_queue();
    seed_sqlite_store_id(store);
    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&swap_credentials(&db_url, RO_ROLE, RO_PASS))
        .await
        .expect("read-only пул репліки");
    let app = router_v1::build_router(standby_state(
        Arc::new(OutboxInvoicesV1::new(Arc::new(SqlxInvoices::new(
            StorePool::new(ro_pool.clone()),
        )))),
        Arc::new(OutboxInvoicesV2::new(Arc::new(SqlxInvoices::new(
            StorePool::new(ro_pool.clone()),
        )))),
        ro_pool.clone(),
    ));
    let token = create_access_token(&admin_id.to_string(), "admin", &[], SECRET).expect("JWT");
    let (status, dto, raw) = call(
        &app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        json!({
            "number": "INV-PUSH-1",
            "supplier_id": supplier.to_string(),
            "invoice_date": "2026-09-01T10:00:00",
            "is_fiscal": false,
            "notes": "офлайн-накладна з адаптера (e2e)",
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

    // ── Сервер піднято (Rust-гілка інвойсів УВІМКНЕНА) → push каси ─────────
    std::env::set_var(torgashka_api::RUST_INVOICES_ENV, "1");
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
    let s1 = push_once(&db_path, &client, &cfg).await;
    eprintln!("[invoice e2e] перший push: {s1:?}");
    assert_eq!(s1.done, 1, "перший push → created (payload адаптера прийнято)");
    assert_eq!(s1.failed, 0, "помилок немає: {s1:?}");
    assert_eq!(s1.already_exists, 0, "перший push — не дублікат");

    // ── Серверний стан: 1 накладна з client_uuid каси, stock +3 ────────────
    let (srv_store, srv_supplier, srv_number, srv_status): (Uuid, Uuid, String, String) =
        sqlx::query_as(
            "SELECT store_id, supplier_id, number, status::text FROM invoices WHERE client_uuid = $1",
        )
        .bind(cu)
        .fetch_one(&pool)
        .await
        .expect("рядок invoices на primary");
    assert_eq!(srv_store, store);
    assert_eq!(srv_supplier, supplier);
    assert_eq!(srv_number, "INV-PUSH-1");
    assert_eq!(srv_status, "confirmed", "приймач проводить накладну одразу");
    assert_eq!(pg_invoice_rows(&pool, store).await, 1, "рівно 1 накладна");
    assert_eq!(
        pg_stock(&pool, store, product).await.as_deref(),
        Some("3.000"),
        "серверний stock = +3.000 (один ефект)"
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
    let s2 = push_once(&db_path, &client, &cfg).await;
    eprintln!("[invoice e2e] повторний push: {s2:?}");
    assert_eq!(s2.already_exists, 1, "повторний push → already_exists");
    assert_eq!(s2.done, 0, "нового created немає");
    assert_eq!(s2.failed, 0, "помилок немає");
    assert_eq!(pg_invoice_rows(&pool, store).await, 1, "дублів немає (UNIQUE 0016)");
    assert_eq!(
        pg_stock(&pool, store, product).await.as_deref(),
        Some("3.000"),
        "повторний push не подвоїв stock-ефект"
    );
    eprintln!("[invoice_standby_outbox_e2e] ✅ ТЕСТ 2: created → already_exists, 1 рядок, 0 дублів");
}
