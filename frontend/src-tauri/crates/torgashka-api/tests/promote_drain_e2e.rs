//! ФАЗА 3.8 — БЕЗПЕКА ЧЕРГИ ПРИ `promote` (E2E, реальні PG + SQLite каси).
//!
//! Дефект (знайдено NIKO): після `POST /api/v1/local/promote` вузол ставав
//! primary, але
//!   1. фоновий push далі слал чергу на СТАРИЙ `server_url` із SQLite settings
//!      (ризик split-brain) — закрито ґейтом `NodeConfig::push_blocked_reason`
//!      у `sync_push::push_pending_batch_with_node`;
//!   2. залишок SQLite-черги (продажі/накладні, зроблені офлайн) НІКУДИ не
//!      застосовувався — закрито drain-ом
//!      (`route_local::drain_local_outbox`, ядро `sync::process_push_item`)
//!      + ендпоінтом `POST /api/v1/local/outbox/drain`.
//!
//! Тести (критерії 3-4 контракту):
//!   * [`promote_drains_local_outbox_into_own_pg`] — накладна каси офлайн у
//!     черзі → `promote` → агрегат у ВЛАСНОМУ PG, `pending_outbox` порожній,
//!     HTTP-push до старого сервера НЕ робився (ґейт);
//!   * [`repeated_drain_is_idempotent_already_exists`] — повторний drain → 0
//!     дублів (`already_exists`), рівно 1 документ у PG.
//!
//! Харнес — той самий набір прийомів, що `adr0007_at_contract.rs`
//! (ізольований SQLite каси через `XDG_DATA_HOME`, тестова PG через
//! `common::force_test_db`, `db_sources.toml` — у temp, щоб promote не писав у
//! репозиторій).

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use sqlx::PgPool;
use torgashka_api::auth::create_access_token;
use torgashka_api::route_local::LocalApiState;
use torgashka_api::{router_v1, AppState};
use torgashka_domain::{
    InvoicesV1Service, InvoicesV2Service, PosService, ReadDirectories, WriteDirectories,
};
use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use torgashka_infrastructure::offline::sync_push::{
    open_connection, pending_count, push_pending_batch_with_node, PushConfig,
};
use torgashka_infrastructure::repositories::directories::SqlxDirectories;
use torgashka_infrastructure::repositories::invoices::SqlxInvoices;
use torgashka_infrastructure::repositories::outbox_invoices::{OutboxInvoicesV1, OutboxInvoicesV2};
use torgashka_infrastructure::repositories::outbox_pos::OutboxPos;
use torgashka_infrastructure::repositories::pos::SqlxPos;
use torgashka_infrastructure::repositories::write::SqlxWriteDirectories;
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

const SECRET: &str = "promote-drain-e2e-secret";

/// Тести ділять ОДИН SQLite каси (temp на бінар) → послідовно.
static SEQ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ─────────────────────────────────────────────────────────────────────────────
// Ізоляція (SQLite каси + файл db_sources.toml)
// ─────────────────────────────────────────────────────────────────────────────

/// `XDG_DATA_HOME` → tempdir: `OfflineDatabase::default_db_path()` (SQLite каси)
/// більше не вказує на реальні дані користувача.
fn isolate_sqlite() -> &'static std::path::Path {
    static ONCE: std::sync::Once = std::sync::Once::new();
    static mut DIR: Option<&'static std::path::Path> = None;
    ONCE.call_once(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let leaked: &'static std::path::Path = Box::leak(dir.keep().into_boxed_path());
        std::env::set_var("XDG_DATA_HOME", leaked);
        // SAFETY: ініціалізація один раз під Once до паралельних читачів.
        unsafe { DIR = Some(leaked) };
    });
    unsafe { DIR.expect("XDG_DATA_HOME") }
}

/// `TORGASHKA_DB_SOURCES` → temp-файл: `promote` пише `[node] mode=primary`
/// САМЕ туди (не в репозиторій і не в робочу конфігурацію вузла).
fn db_sources_path() -> &'static std::path::Path {
    static ONCE: std::sync::Once = std::sync::Once::new();
    static mut FILE: Option<&'static std::path::Path> = None;
    ONCE.call_once(|| {
        let dir = isolate_sqlite();
        let file = dir.join("db_sources.toml");
        std::env::set_var("TORGASHKA_DB_SOURCES", &file);
        let leaked: &'static std::path::Path = Box::leak(file.into_boxed_path());
        // SAFETY: ініціалізація один раз під Once до паралельних читачів.
        unsafe { FILE = Some(leaked) };
    });
    unsafe { FILE.expect("TORGASHKA_DB_SOURCES") }
}

fn offline_db_path() -> std::path::PathBuf {
    torgashka_infrastructure::offline::db::OfflineDatabase::default_db_path().expect("шлях SQLite")
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

async fn api_pool() -> PgPool {
    apply_schema().await;
    torgashka_infrastructure::db::connect_readonly_pool(3)
        .await
        .expect("writable-пул тестової БД")
}

// ─────────────────────────────────────────────────────────────────────────────
// Seed / черга каси
// ─────────────────────────────────────────────────────────────────────────────

/// Точка + власник (role=owner: `promote`/drain — owner-only) + постачальник +
/// товар. Власник — реальний `users` рядок: drain застосовує агрегати від його
/// імені (касир = sub власника; черга каси не зберігає касира — АНОМАЛІЯ звіту).
async fn seed_catalog(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let store = Uuid::new_v4();
    let owner = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Promote-Drain Точка')")
        .bind(store)
        .execute(pool)
        .await
        .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Promote Власник', $2, 'x', 'owner'::public.user_role, true, now(), now(), true)",
    )
    .bind(owner)
    .bind(format!("pd_e2e_{}", &owner.to_string()[..8]))
    .execute(pool)
    .await
    .expect("INSERT users");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at) \
         VALUES ($1, $2, 'owner', '{}'::jsonb, true, now())",
    )
    .bind(owner)
    .bind(store)
    .execute(pool)
    .await
    .expect("INSERT user_stores");
    let supplier = Uuid::new_v4();
    sqlx::query("INSERT INTO suppliers (id, name) VALUES ($1, 'E2E Promote Постачальник')")
        .bind(supplier)
        .execute(pool)
        .await
        .expect("INSERT suppliers");
    let product = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E Promote Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(pool)
    .await
    .expect("INSERT products");
    (store, owner, supplier, product)
}

fn invoice_body(supplier: Uuid, product: Uuid, number: &str, qty: &str, price: &str) -> Value {
    json!({
        "number": number,
        "supplier_id": supplier.to_string(),
        "invoice_date": "2026-09-01T10:00:00",
        "payment_method": null,
        "is_fiscal": false,
        "notes": "Фаза 3.8 drain",
        "total_amount": price,
        "items": [{
            "product_id": product.to_string(),
            "quantity": qty,
            "price": price,
            "total": price
        }]
    })
}

fn seed_sqlite_store_id(store: Uuid) {
    let path = offline_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("каталог даних каси");
    }
    let conn = open_connection(&path).expect("SQLite каси + міграції");
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('store_id', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [store.to_string()],
    )
    .expect("settings.store_id");
}

/// Локальний каталог каси (локальна валідація позицій накладної, Фаза 3.5).
fn seed_local_catalog(product: Uuid) {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.execute(
        "INSERT INTO products_v2 (id, name, price, is_deleted, server_version) \
         VALUES (?1, 'Каталог (pull)', 100.0, 0, 1) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name",
        [product.to_string()],
    )
    .expect("products_v2 (локальний каталог)");
}

fn reset_local_queue() {
    let path = offline_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("каталог даних каси");
    }
    let conn = open_connection(&path).expect("SQLite каси + міграції");
    for t in [
        "outbox",
        "invoices",
        "invoice_items",
        "receipts",
        "stock",
        "cash_ledger",
        "inventories",
        "sync_log",
    ] {
        let _ = conn.execute(&format!("DELETE FROM {t}"), []);
    }
}

fn conn_ret() -> rusqlite::Connection {
    rusqlite::Connection::open(offline_db_path()).expect("SQLite каси")
}

// ─────────────────────────────────────────────────────────────────────────────
// Стан фасаду
// ─────────────────────────────────────────────────────────────────────────────

/// Standby-вузол, чия «локальна репліка» — тестова PG (для drain вона вже
/// writable: у тесті репліка й primary фізично той самий кластер).
fn standby_state(
    pool: PgPool,
    v1: Option<Arc<dyn InvoicesV1Service + Send + Sync>>,
    v2: Option<Arc<dyn InvoicesV2Service + Send + Sync>>,
) -> AppState {
    AppState {
        jwt_secret: Arc::new(SECRET.to_string()),
        readdirs: None,
        write: None,
        write_pool: Some(pool.clone()),
        pos: None,
        ledger: None,
        auth: None,
        prro: None,
        debtors: None,
        documents: None,
        documents_pool: None,
        invoices_v1: v1,
        invoices_v2: v2,
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

/// Той самий стан ПЛЮС локальні маршрути (`state.local`): у тесті роль
/// локальної репліки виконує тестова PG (writable — як після promote).
fn with_local(mut st: AppState) -> AppState {
    let pool = st.write_pool.clone().expect("пул");
    let sp = StorePool::new(pool);
    st.local = Some(LocalApiState {
        cfg: st.node_config.clone(),
        pool: sp.clone(),
        upstream_pool: None,
        readdirs: Arc::new(SqlxDirectories::new(sp.clone()))
            as Arc<dyn ReadDirectories + Send + Sync>,
        pos: Arc::new(OutboxPos::new(Arc::new(SqlxPos::new(sp.clone()))))
            as Arc<dyn PosService + Send + Sync>,
        write: Arc::new(SqlxWriteDirectories::new(sp)) as Arc<dyn WriteDirectories + Send + Sync>,
    });
    st
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

async fn pg_invoice_rows(pool: &PgPool, store: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE store_id = $1")
        .bind(store)
        .fetch_one(pool)
        .await
        .expect("COUNT invoices")
}

async fn pg_stock(pool: &PgPool, store: Uuid, product: Uuid) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(store)
    .bind(product)
    .fetch_optional(pool)
    .await
    .expect("SELECT stock")
}

/// Клієнт на ЗАВІДОМО закритий порт: якщо ґейт не спрацював, push поверне
/// `Err` (мережева помилка) — тобто `Ok(gated)` доводить, що HTTP не робився.
fn cfg_to_closed_port(db_path: &std::path::Path) -> PushConfig {
    PushConfig {
        base_url: "http://127.0.0.1:1".to_string(),
        token: "stale-device-token".to_string(),
        store_id: None,
        db_path: db_path.to_path_buf(),
        interval_secs: 30,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// КРИТЕРІЙ 3: promote → drain → агрегат у ВЛАСНОМУ PG, черга порожня
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn promote_drains_local_outbox_into_own_pg() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite();
    let cfg_path = db_sources_path().to_path_buf();

    let pool = api_pool().await;
    let (store, owner, supplier, product) = seed_catalog(&pool).await;
    reset_local_queue();
    seed_sqlite_store_id(store);
    seed_local_catalog(product);

    let v1: Arc<dyn InvoicesV1Service + Send + Sync> = Arc::new(OutboxInvoicesV1::new(Arc::new(
        SqlxInvoices::new(StorePool::new(pool.clone())),
    )));
    let v2: Arc<dyn InvoicesV2Service + Send + Sync> = Arc::new(OutboxInvoicesV2::new(Arc::new(
        SqlxInvoices::new(StorePool::new(pool.clone())),
    )));
    let app = router_v1::build_router(with_local(standby_state(pool.clone(), Some(v1), Some(v2))));
    let token = create_access_token(&owner.to_string(), "owner", &[], SECRET).expect("JWT");

    // ── 1. Накладна каси ОФЛАЙН (канонічний шлях standby → SQLite-черга) ────
    let (status, dto, raw) = call(
        &app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        invoice_body(supplier, product, "PD-1", "3.000", "100.00"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "накладна офлайн: {status} {raw}"
    );
    assert_eq!(dto["status"], "queued", "маркер черги: {dto}");
    let client_uuid = dto["id"].as_str().expect("client_uuid").to_string();

    let conn = conn_ret();
    assert_eq!(
        pending_count(&conn).expect("pending"),
        1,
        "1 агрегат у черзі"
    );
    drop(conn);
    assert_eq!(
        pg_invoice_rows(&pool, store).await,
        0,
        "до promote документ у PG відсутній"
    );

    // Документа з таким client_uuid у PG ще немає (ідемпотентний ключ).
    let pg_by_uuid: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE client_uuid = $1::uuid")
            .bind(&client_uuid)
            .fetch_one(&pool)
            .await
            .expect("COUNT invoices by client_uuid");
    assert_eq!(pg_by_uuid, 0, "до promote документа немає");

    // ── 2. PROMOTE: вузол стає джерелом істини + drain залишку черги ────────
    let (status, body, raw) = call(
        &app,
        "POST",
        "/api/v1/local/promote",
        &token,
        store,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "promote: {status} {raw}");
    let drain = &body["outbox_drain"];
    assert_eq!(drain["drained"], 1, "drain у відповіді promote: {body}");
    assert_eq!(
        drain["created"], 1,
        "агрегат створено у власному PG: {body}"
    );
    assert_eq!(drain["errors"], 0, "жодної помилки: {body}");

    // ── 3. Агрегат У ЛОКАЛЬНОМУ PG, черга ПОРОЖНЯ ────────────────────────────
    assert_eq!(
        pg_invoice_rows(&pool, store).await,
        1,
        "накладна застосована до ВЛАСНОГО PG"
    );
    let pg_by_uuid: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE client_uuid = $1::uuid")
            .bind(&client_uuid)
            .fetch_one(&pool)
            .await
            .expect("COUNT invoices by client_uuid (після drain)");
    assert_eq!(
        pg_by_uuid, 1,
        "той самий client_uuid каси → один документ у PG"
    );
    let pg_items: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invoice_items i JOIN invoices v ON v.id = i.invoice_id \
         WHERE v.store_id = $1",
    )
    .bind(store)
    .fetch_one(&pool)
    .await
    .expect("COUNT invoice_items");
    assert_eq!(pg_items, 1, "позиції накладної теж у PG");
    assert_eq!(
        pg_stock(&pool, store, product).await.as_deref(),
        Some("3.000"),
        "авторитетний stock = +3.000 (stock-ефект застосовано)"
    );
    let conn = conn_ret();
    assert_eq!(
        pending_count(&conn).expect("pending"),
        0,
        "pending_outbox порожній після drain"
    );
    let statuses: Vec<String> = conn
        .prepare("SELECT status FROM outbox ORDER BY id")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert_eq!(statuses, vec!["done".to_string()], "агрегат знято з черги");
    drop(conn);

    // ── 4. ҐЕЙТ: на СТАРИЙ сервер більше не пушимо (mode=primary, апстріму немає)
    let disk = NodeConfig::load_explicit_from_path(&cfg_path).expect("promote записав [node]");
    assert_eq!(
        disk.mode,
        NodeMode::Primary,
        "promote → mode=primary на диску"
    );
    assert!(
        disk.push_blocked_reason().is_some(),
        "primary без апстріму → HTTP-push вимкнено"
    );
    let client = reqwest::Client::new();
    let s = push_pending_batch_with_node(
        &offline_db_path(),
        &client,
        &cfg_to_closed_port(&offline_db_path()),
        Some(&disk),
    )
    .await
    .expect("ґейт: не мережева помилка (до закритого порту не ходили)");
    assert!(s.gated, "маркер ґейта");
    assert_eq!(s.sent, 0, "0 HTTP на колишній server_url");
}

// ─────────────────────────────────────────────────────────────────────────────
// КРИТЕРІЙ 4: повторний drain — 0 дублів (`already_exists`)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn repeated_drain_is_idempotent_already_exists() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite();
    let _ = db_sources_path();

    let pool = api_pool().await;
    let (store, owner, supplier, product) = seed_catalog(&pool).await;
    reset_local_queue();
    seed_sqlite_store_id(store);
    seed_local_catalog(product);

    let v1: Arc<dyn InvoicesV1Service + Send + Sync> = Arc::new(OutboxInvoicesV1::new(Arc::new(
        SqlxInvoices::new(StorePool::new(pool.clone())),
    )));
    let v2: Arc<dyn InvoicesV2Service + Send + Sync> = Arc::new(OutboxInvoicesV2::new(Arc::new(
        SqlxInvoices::new(StorePool::new(pool.clone())),
    )));
    let app = router_v1::build_router(with_local(standby_state(pool.clone(), Some(v1), Some(v2))));
    let token = create_access_token(&owner.to_string(), "owner", &[], SECRET).expect("JWT");

    let (status, dto, raw) = call(
        &app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        invoice_body(supplier, product, "PD-2", "2.000", "100.00"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{status} {raw}");
    let client_uuid = dto["id"].as_str().expect("client_uuid").to_string();

    // Перший drain — усередині promote.
    let (status, body, raw) = call(
        &app,
        "POST",
        "/api/v1/local/promote",
        &token,
        store,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "promote: {status} {raw}");
    assert_eq!(body["outbox_drain"]["created"], 1, "{body}");
    assert_eq!(pg_invoice_rows(&pool, store).await, 1, "один документ");

    // Повторний drain БЕЗ зміни черги — черга вже порожня: жодних дублів.
    let (status, body2, raw) = call(
        &app,
        "POST",
        "/api/v1/local/outbox/drain",
        &token,
        store,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "drain: {status} {raw}");
    assert_eq!(body2["drained"], 0, "черга порожня: {body2}");
    assert_eq!(pg_invoice_rows(&pool, store).await, 1, "дублів немає");

    // «Повторна доставка»: повертаємо агрегат у pending (як після втраченої
    // відповіді на push) → drain мусить дати already_exists, а не новий документ.
    let conn = conn_ret();
    conn.execute(
        "UPDATE outbox SET status = 'pending', pushed_at = NULL WHERE client_uuid = ?1",
        [&client_uuid],
    )
    .expect("повернути агрегат у pending");
    drop(conn);

    let (status, body3, raw) = call(
        &app,
        "POST",
        "/api/v1/local/outbox/drain",
        &token,
        store,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "drain#3: {status} {raw}");
    assert_eq!(
        body3["already_exists"], 1,
        "ідемпотентність client_uuid: {body3}"
    );
    assert_eq!(body3["created"], 0, "нового документа не створено: {body3}");
    assert_eq!(body3["drained"], 1, "агрегат знято з черги: {body3}");
    assert_eq!(
        pg_invoice_rows(&pool, store).await,
        1,
        "рівно 1 документ у PG (дублів немає)"
    );
    let conn = conn_ret();
    assert_eq!(pending_count(&conn).expect("pending"), 0, "черга порожня");
}

// ─────────────────────────────────────────────────────────────────────────────
// Межа drain-у: агрегат, який застосувати НЕ вдалось, ЛИШАЄТЬСЯ `pending`
// (дані не втрачено) і видимий оператору в `errors`/`errors_detail`.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn drain_error_keeps_item_pending_and_reports_reason() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite();
    let _ = db_sources_path();

    let pool = api_pool().await;
    let (store, owner, _supplier, _product) = seed_catalog(&pool).await;
    reset_local_queue();
    seed_sqlite_store_id(store);

    let app = router_v1::build_router(with_local(standby_state(pool.clone(), None, None)));
    let token = create_access_token(&owner.to_string(), "owner", &[], SECRET).expect("JWT");

    // Агрегат типу, який приймач push не знає (`receiver_table` → None):
    // конверт валідний, застосувати неможливо.
    let conn = conn_ret();
    conn.execute(
        "INSERT INTO outbox (type, client_uuid, payload, status, attempts) \
         VALUES ('mystery', ?1, ?2, 'pending', 0)",
        rusqlite::params![
            Uuid::new_v4().to_string(),
            json!({
                "type": "mystery",
                "client_uuid": Uuid::new_v4().to_string(),
                "store_id": store.to_string(),
                "created_at": "2026-09-01T10:00:00Z",
                "payload": { "x": 1 }
            })
            .to_string()
        ],
    )
    .expect("INSERT outbox (mystery)");
    drop(conn);

    let (status, body, raw) = call(
        &app,
        "POST",
        "/api/v1/local/outbox/drain",
        &token,
        store,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "drain: {status} {raw}");
    assert_eq!(body["drained"], 0, "нічого не знято з черги: {body}");
    assert_eq!(body["errors"], 1, "одна помилка: {body}");
    assert_eq!(body["pending_left"], 1, "агрегат ЛИШАЄТЬСЯ в черзі: {body}");
    let details = body["errors_detail"].as_array().expect("errors_detail");
    assert!(
        details[0].as_str().unwrap_or_default().contains("mystery"),
        "причина видима оператору: {details:?}"
    );

    let conn = conn_ret();
    let st: String = conn
        .query_row(
            "SELECT status FROM outbox WHERE type = 'mystery'",
            [],
            |r| r.get(0),
        )
        .expect("status");
    assert_eq!(st, "pending", "дані не втрачено — агрегат у черзі");
}
