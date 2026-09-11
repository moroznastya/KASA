//! E2E: ІНВЕНТАРИЗАЦІЯ на standby — адаптер `OutboxWrite` (ADR-0007 §11.1,
//! §11.7.9.7; Фаза 3.3a).
//!
//! До виправлення: на standby `state.write` = `SqlxWriteDirectories(репліка)`,
//! тому `POST /api/v1/inventory` падав з PG-помилкою
//! `cannot execute INSERT in a read-only transaction` → сирий **500**.
//!
//! ТЕСТ `standby_inventory_queues_locally_and_never_writes_replica`:
//!   1. репліка ФІЗИЧНО read-only (роль `default_transaction_read_only = on`);
//!   2. `POST /api/v1/inventory` на standby → **201** з бізнес-статусом
//!      `queued` (не 500);
//!   3. у PG `inventories` — **0 рядків** (репліка не ціль запису);
//!   4. SQLite: агрегат `inventories` (synced=1) + рівно **1** outbox-запис
//!      типу `inventory` = `pending`;
//!   5. локальний stock — **абсолютний рівень** факту (7.000) у тій самій
//!      транзакції (перерахунок, а не дельта);
//!   6. **негативний контроль**: та сама read-only БД + СТАРА обв'язка
//!      (`state.write = SqlxWriteDirectories(репліка)`) → 500, у тілі НЕМАЄ
//!      сирого тексту PG (санація §D), 0 рядків у PG і 0 нових outbox-записів.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use torgashka_api::auth::create_access_token;
use torgashka_api::{router_v1, AppState};
use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use torgashka_infrastructure::repositories::directories::SqlxDirectories;
use torgashka_infrastructure::repositories::outbox_write::OutboxWrite;
use torgashka_infrastructure::repositories::write::SqlxWriteDirectories;
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

const SECRET: &str = "inventory-standby-e2e-secret";
const RO_ROLE: &str = "torgashka_inv_e2e_ro";
const RO_PASS: &str = "torgashka_inv_e2e_ro_pwd";

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

/// Standby-фасад із переданою write-гілкою (нова або стара обв'язка).
fn standby_state(
    write: Arc<dyn torgashka_domain::WriteDirectories + Send + Sync>,
    pool: sqlx::PgPool,
) -> AppState {
    AppState {
        jwt_secret: Arc::new(SECRET.to_string()),
        // Роути /api/v1/inventory монтуються під readdirs (router_v1.rs:108) —
        // на реальному standby read+write ідуть однією гілкою init_readdirs.
        readdirs: Some(Arc::new(SqlxDirectories::new(StorePool::new(pool.clone())))
            as Arc<dyn torgashka_domain::ReadDirectories + Send + Sync>),
        write: Some(write),
        write_pool: Some(pool.clone()),
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
    let store = std::env::var("TORGASHKA_STORE_ID").unwrap_or_default();
    let _ = store;
    conn.query_row(
        "SELECT quantity FROM stock WHERE product_id = ?1",
        [product.to_string()],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
}

async fn pg_inventory_rows(pool: &sqlx::PgPool, store: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM inventories WHERE store_id = $1")
        .bind(store)
        .fetch_one(pool)
        .await
        .expect("COUNT inventories")
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

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ: standby — 201/queued, локальна черга, 0 рядків у PG, негативний контроль
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn standby_inventory_queues_locally_and_never_writes_replica() {
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let admin_pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url(&db_url);
    ensure_readonly_role(&admin_pool, &db_name).await;

    // Точка + адміністратор точки (потрібні для `require_admin` інвентаризації).
    let store = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Inventory Точка') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Inventory Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
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
    let ro_err = sqlx::query(
        "INSERT INTO inventories (id, number, location, inventory_date, status, created_at, updated_at, created_by_id, store_id) \
         VALUES ($1, 'RO-1', 'x', now(), 'draft', now(), now(), $2, $3)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(store)
    .execute(&ro_pool)
    .await
    .expect_err("репліка мусить бути read-only");
    assert!(
        ro_err.to_string().contains("read-only transaction"),
        "роль репліки не read-only: {ro_err}"
    );
    eprintln!("[inventory e2e] репліка справді read-only: {ro_err}");

    // ── 2. Standby-фасад із OutboxWrite (нова обв'язка) ────────────────────
    seed_sqlite_store_id(store);
    let write: Arc<dyn torgashka_domain::WriteDirectories + Send + Sync> =
        Arc::new(OutboxWrite::new(Arc::new(SqlxWriteDirectories::new(
            StorePool::new(ro_pool.clone()),
        ))));
    let app = router_v1::build_router(standby_state(write, ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");
    let product = Uuid::new_v4();

    // ── 3. Перерахунок: факт 7.000 при облікових 5.000 → 201/queued ─────────
    let (status, dto, raw) = call(
        &app,
        "POST",
        "/api/v1/inventory",
        &token,
        store,
        json!({
            "location": "Склад каси",
            "inventory_date": "2026-09-01T10:00:00",
            "notes": "standby inventory e2e",
            "items": [{
                "product_id": product.to_string(),
                "actual_quantity": "7.000",
                "accounting_quantity": "5.000",
                "difference": "2.000",
                "cost_price": "10.00",
                "price": "15.00"
            }]
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "інвентаризація на standby має створитися локально (201/queued), маємо {status}: {raw}"
    );
    assert_eq!(dto["status"], "queued", "бізнес-статус черги: {dto}");
    let client_uuid = dto["id"].as_str().expect("client_uuid документа").to_string();
    assert_eq!(
        local_stock_milli(product),
        7_000,
        "локальний stock = АБСОЛЮТНИЙ рівень факту (7.000) у тій самій транзакції"
    );

    // ── 4. У PG — НУЛЬ рядків ──────────────────────────────────────────────
    assert_eq!(
        pg_inventory_rows(&admin_pool, store).await,
        0,
        "на standby інвентаризація НЕ пише в репліку"
    );

    // ── 5. SQLite: агрегат inventories + рівно 1 pending ───────────────────
    let (synced, data) = local_aggregate_row("inventories", &client_uuid).expect("агрегат inventories");
    assert_eq!(synced, 1, "агрегат каси — push-кандидат");
    assert!(
        data.contains("\"quantity\":7.0") && data.contains("\"actual_quantity\":7.0"),
        "data = payload каси (кількості як числа; приймач scale-3 розуміє і числа): {data}"
    );
    assert_eq!(
        outbox_rows(),
        vec![("inventory".to_string(), "pending".to_string(), 1)],
        "outbox: рівно один pending типу inventory"
    );

    // ── 6. НЕГАТИВНИЙ КОНТРОЛЬ: та сама read-only БД + СТАРА обв'язка ──────
    let old_write: Arc<dyn torgashka_domain::WriteDirectories + Send + Sync> =
        Arc::new(SqlxWriteDirectories::new(StorePool::new(ro_pool.clone())));
    let old_app = router_v1::build_router(standby_state(old_write, ro_pool.clone()));
    let (old_status, _, old_raw) = call(
        &old_app,
        "POST",
        "/api/v1/inventory",
        &token,
        store,
        json!({
            "location": "Склад каси",
            "inventory_date": "2026-09-01T11:00:00",
            "items": [{
                "product_id": product.to_string(),
                "actual_quantity": "1.000",
                "accounting_quantity": "0.000",
                "difference": "1.000",
                "cost_price": "10.00",
                "price": "15.00"
            }]
        }),
    )
    .await;
    assert_eq!(
        old_status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "стара обв'язка на read-only репліці мусить дати 500, маємо {old_status}: {old_raw}"
    );
    for banned in ["read-only transaction", "INSERT INTO", "inventories", "sqlx"] {
        assert!(
            !old_raw.contains(banned),
            "у тілі 500 немає сирого тексту PG/драйвера ('{banned}'): {old_raw}"
        );
    }
    eprintln!("[inventory e2e] негативний контроль: {old_status} {old_raw}");
    assert_eq!(
        pg_inventory_rows(&admin_pool, store).await,
        0,
        "негативний контроль нічого не записав у PG"
    );
    assert_eq!(
        outbox_rows(),
        vec![("inventory".to_string(), "pending".to_string(), 1)],
        "негативний контроль не додав outbox-записів"
    );
    assert_eq!(
        local_stock_milli(product),
        7_000,
        "локальний stock не змінився негативним контролем"
    );
    eprintln!("[inventory_standby_outbox_e2e] ✅ 201/queued + 1 pending + stock 7000 + 0 рядків PG");
}
