//! Executable-частина приймальних тестів ADR-0007 §5 (AT-1…AT-15).
//!
//! ФАЗА 3.4: більшість AT були чек-листом, а не кодом. Тут зібрані ті AT, які
//! можна виконати НАЯВНИМ харнесом (без 2-вузлового середовища з фізичною
//! реплікацією + аналізом `postgres.log` репліки):
//!
//! | AT | тест цього файлу | що доводить |
//! |----|------------------|-------------|
//! | AT-2 | [`at_02_standby_primary_down_returns_503_contract`] | primary НАЛАШТОВАНИЙ і недосяжний → 503 §4 (не 500), маркери; live-primary → pass-through (доказ, що це справді гілка «primary down», а не «URL не задано») |
//! | AT-10 | [`at_10_standby_without_upstream_write_url_is_503_not_500`] | standby без `upstream_write_url` → 503 §4 на всіх `UPSTREAM_NOW`-поверхнях, 0 рядків у БД (немає тихого запису), тіло рівно `{detail}` |
//! | AT-14 | [`at_14_standby_invoice_atomic_and_marker`] | документ + позиції + outbox + SQLite-stock в ОДНІЙ транзакції; маркер «очікує синку»; контур атомарний (провал ефекту → ROLLBACK усього) |
//! | AT-15 | [`at_15_local_vs_authoritative_stock_delta_and_alignment`] | локальний (SQLite) і авторитетний (PG) залишки окремо + дельта; вирівнювання інвентаризацією (`set_stock_level`) |
//!
//! Уже-executable AT поза цим файлом (без нового коду):
//!   * **AT-8** — `tests/write_gate_guard.rs` (9 тестів, статичний guard);
//!   * **AT-9** — `tests/write_gate_behavior.rs::primary_mode_behavior_unchanged_f2`
//!     + unit `node_config::tests::upstream_write_url_field_does_not_affect_primary_resolution`;
//!   * **AT-7** — `tests/write_gate_behavior.rs::standby_disabled_route_sync_push_returns_503`;
//!   * **AT-11/AT-12** — `tests/sync_invoice_push_e2e.rs::invoice_push_idempotent_and_stock_once`;
//!   * **AT-13** — `tests/sync_invoice_disabled_e2e.rs::invoice_push_with_rust_invoices_disabled_is_not_silently_acked`;
//!   * **AT-14 (частина 1)** — `tests/invoice_standby_outbox_e2e.rs::standby_invoice_queues_locally_and_never_writes_replica`.
//!
//! Решта AT (1, 3, 4, 5, 6) — 🕐 потребують 2-вузлового середовища; точні
//! перешкоди зафіксовані в §5 ADR (колонка «Статус»).

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use torgashka_api::auth::create_access_token;
use torgashka_api::write_gate::{STANDBY_DETAIL, UPSTREAM_DOWN, UPSTREAM_HEADER};
use torgashka_api::{router_v1, AppState};
use torgashka_domain::{InvoicesV1Service, InvoicesV2Service};
use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use torgashka_infrastructure::offline::sync_push::{open_connection, pending_count, PushConfig};
use torgashka_infrastructure::offline::transactions;
use torgashka_infrastructure::repositories::invoices::SqlxInvoices;
use torgashka_infrastructure::repositories::outbox_invoices::{OutboxInvoicesV1, OutboxInvoicesV2};
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

const SECRET: &str = "adr0007-at-contract-secret";
const RO_ROLE: &str = "torgashka_at_e2e_ro";
const RO_PASS: &str = "torgashka_at_e2e_ro_pwd";
const NODE_MODE_HEADER: &str = "x-torgashka-node-mode";

/// Тести ділять ОДИН SQLite каси (XDG temp на бінар) → послідовно.
static SEQ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ─────────────────────────────────────────────────────────────────────────────
// Харнес (той самий набір прийомів, що `*_standby_outbox_e2e`)
// ─────────────────────────────────────────────────────────────────────────────

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

async fn api_pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_readonly_pool(3)
        .await
        .expect("writable-пул тестової БД")
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

/// Серіалізація DDL по ролі: тести бінаря не можуть одночасно робити
/// `ALTER ROLE`/`GRANT` (PG: `tuple concurrently updated`).
static ROLE_SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn swap_credentials(url: &str, user: &str, pass: &str) -> String {
    let scheme_end = url.find("://").map(|i| i + 3).expect("схема URL");
    let after = &url[scheme_end..];
    let host_start = after.find('@').map(|i| i + 1).unwrap_or(0);
    format!(
        "{}{}:{}@{}",
        &url[..scheme_end],
        user,
        pass,
        &after[host_start..]
    )
}

fn db_name_from_url(url: &str) -> String {
    let before = url.split('?').next().unwrap_or(url);
    before[before.rfind('/').expect("слеш") + 1..].to_string()
}

/// Read-only роль = «репліка»: будь-який запис у неї фізично неможливий.
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
        "stock",
        "cash_ledger",
        "inventories",
        "sync_log",
    ] {
        let _ = conn.execute(&format!("DELETE FROM {t}"), []);
    }
}

/// Стан фасаду standby-вузла: `write_pool` — РЕАЛЬНИЙ writable пул тестової БД
/// (щоб «0 рядків у БД» було доказом, а не відсутністю пулу), інвойси — черга.
fn standby_state(
    write_pool: Option<sqlx::PgPool>,
    v1: Option<Arc<dyn InvoicesV1Service + Send + Sync>>,
    v2: Option<Arc<dyn InvoicesV2Service + Send + Sync>>,
    pool: sqlx::PgPool,
) -> AppState {
    AppState {
        jwt_secret: Arc::new(SECRET.to_string()),
        readdirs: None,
        write: None,
        write_pool,
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

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    store: Uuid,
    body: Value,
) -> (StatusCode, Value, String, axum::http::HeaderMap) {
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
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("тіло");
    let raw = String::from_utf8_lossy(&bytes).to_string();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, raw, headers)
}

fn header<'a>(h: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// Контракт §4 цілком: 503 + рівно `{"detail": STANDBY_DETAIL}` + маркери.
fn assert_503_contract(where_: &str, status: StatusCode, body: &Value, headers: &axum::http::HeaderMap) {
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{where_}: має бути 503, не {status}");
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "{where_}: 500 заборонено §4");
    let obj = body.as_object().unwrap_or_else(|| panic!("{where_}: тіло не об'єкт: {body}"));
    assert_eq!(
        obj.len(),
        1,
        "{where_}: тіло мусить містити РІВНО `detail`, маємо {body}"
    );
    assert_eq!(
        obj.get("detail").and_then(Value::as_str),
        Some(STANDBY_DETAIL),
        "{where_}: текст detail мусить бути контрактним §4, маємо {body}"
    );
    assert_eq!(
        header(headers, NODE_MODE_HEADER),
        Some("standby"),
        "{where_}: X-Torgashka-Node-Mode"
    );
    assert_eq!(
        header(headers, UPSTREAM_HEADER),
        Some(UPSTREAM_DOWN),
        "{where_}: X-Torgashka-Upstream"
    );
    assert!(
        header(headers, "retry-after").is_some(),
        "{where_}: Retry-After (маркер §4)"
    );
}

async fn count(pool: &sqlx::PgPool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("COUNT {table}: {e}"))
}

fn local_stock_milli(product: Uuid) -> i64 {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    torgashka_infrastructure::offline::stock::get_stock_level(
        &conn,
        &local_store_id(),
        &product.to_string(),
    )
    .expect("локальний залишок SQLite")
}

fn local_store_id() -> String {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'store_id'",
        [],
        |r| r.get::<_, String>(0),
    )
    .expect("settings.store_id")
}

async fn pg_stock(pool: &sqlx::PgPool, store: Uuid, product: Uuid) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(store)
    .bind(product)
    .fetch_optional(pool)
    .await
    .expect("SELECT stock")
}

async fn pg_invoice_rows(pool: &sqlx::PgPool, store: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE store_id = $1")
        .bind(store)
        .fetch_one(pool)
        .await
        .expect("COUNT invoices")
}

/// Точка + адмін + доступ (RLS-контекст) + постачальник + товар.
async fn seed_catalog(pool: &sqlx::PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let store = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E AT Точка')")
        .bind(store)
        .execute(pool)
        .await
        .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E AT Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true)",
    )
    .bind(user_id)
    .bind(format!("at_e2e_{}", &user_id.to_string()[..8]))
    .execute(pool)
    .await
    .expect("INSERT users");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at) \
         VALUES ($1, $2, 'admin', '{}'::jsonb, true, now())",
    )
    .bind(user_id)
    .bind(store)
    .execute(pool)
    .await
    .expect("INSERT user_stores");
    let supplier = Uuid::new_v4();
    sqlx::query("INSERT INTO suppliers (id, name) VALUES ($1, 'E2E AT Постачальник')")
        .bind(supplier)
        .execute(pool)
        .await
        .expect("INSERT suppliers");
    let product = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E AT Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(pool)
    .await
    .expect("INSERT products");
    (store, user_id, supplier, product)
}

fn invoice_body(supplier: Uuid, product: Uuid, number: &str, qty: &str, price: &str) -> Value {
    json!({
        "number": number,
        "supplier_id": supplier.to_string(),
        "invoice_date": "2026-09-01T10:00:00",
        "payment_method": null,
        "is_fiscal": false,
        "notes": "ADR-0007 AT",
        "total_amount": price,
        "items": [{
            "product_id": product.to_string(),
            "quantity": qty,
            "price": price,
            "total": price
        }]
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// AT-2: standby + PRIMARY DOWN → 503 §4 (не 500, не тихий запис)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn at_02_standby_primary_down_returns_503_contract() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let pool = api_pool().await;
    apply_schema().await;

    let (_store, user_id, _supplier, _product) = seed_catalog(&pool).await;
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");
    let app = router_v1::build_router(standby_state(
        Some(pool.clone()),
        None,
        None,
        pool.clone(),
    ));

    // ── КРОК 1: primary ЖИВИЙ (фейковий HTTP-сервер) → pass-through ─────────
    let live = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake primary");
    let live_port = live.local_addr().expect("addr").port();
    drop(live);
    let fake = axum::Router::new().route(
        "/api/v1/admin/stores",
        axum::routing::post(|| async {
            (
                StatusCode::CREATED,
                [(("x-fake-primary"), "yes")],
                axum::Json(serde_json::json!({"fake": true})),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{live_port}"))
        .await
        .expect("fake primary listen");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, fake).await;
    });
    // `server_url` каси (той самий ключ, що читає гейт: write_gate.rs:597).
    torgashka_infrastructure::offline::commands::set_setting(
        "server_url".to_string(),
        format!("http://127.0.0.1:{live_port}"),
    )
    .expect("settings.server_url");

    let stores_before = count(&pool, "stores").await;
    let (st_up, body_up, raw_up, h_up) = call(
        &app,
        "POST",
        "/api/v1/admin/stores",
        &token,
        Uuid::nil(),
        json!({"name": "AT-2 live primary"}),
    )
    .await;
    assert_eq!(
        st_up,
        StatusCode::CREATED,
        "AT-2 (контроль): primary досяжний → pass-through, маємо {st_up} {raw_up}"
    );
    assert_eq!(
        header(&h_up, "x-fake-primary"),
        Some("yes"),
        "AT-2 (контроль): відповідь primary пройшла як є"
    );
    assert_eq!(body_up["fake"], true, "тіло primary: {body_up}");
    assert_eq!(
        header(&h_up, NODE_MODE_HEADER),
        Some("standby"),
        "AT-2: маркер режиму — у КОЖНІЙ відповіді фасаду (§4), у т.ч. на pass-through"
    );
    assert_eq!(
        count(&pool, "stores").await,
        stores_before,
        "AT-2: проксі-гілка НЕ пише в локальну БД (навіть коли пул writable)"
    );

    // ── КРОК 2: primary ВПАВ (URL налаштований, сокет недосяжний) → 503 §4 ──
    // Той самий контракт «налаштований, але недосяжний primary»: порт вільний
    // (bind+release) — жоден процес не слухає.
    server.abort();
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind dead port");
    let dead_port = dead.local_addr().expect("addr").port();
    drop(dead);
    torgashka_infrastructure::offline::commands::set_setting(
        "server_url".to_string(),
        format!("http://127.0.0.1:{dead_port}"),
    )
    .expect("settings.server_url → недосяжний primary");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (st_down, body_down, raw_down, h_down) = call(
        &app,
        "POST",
        "/api/v1/admin/stores",
        &token,
        Uuid::nil(),
        json!({"name": "AT-2 primary down"}),
    )
    .await;
    assert_503_contract(
        "AT-2 primary down: POST /api/v1/admin/stores",
        st_down,
        &body_down,
        &h_down,
    );
    assert_eq!(
        count(&pool, "stores").await,
        stores_before,
        "AT-2: тихого запису в репліку немає (рядків stores стільки ж) — {raw_down}"
    );
    eprintln!(
        "[AT-2] ✅ live primary → {st_up} pass-through; down → {st_down} §4; stores={stores_before} не змінилось"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// AT-10: standby БЕЗ `upstream_write_url` → 503 (не 500, не тихий запис)
// ─────────────────────────────────────────────────────────────────────────────

/// Репрезентативна вибірка `UPSTREAM_NOW`-поверхонь (§3.1 + §11.7.9).
const UPSTREAM_NOW_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/v1/admin/stores"),
    (
        "PUT",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111",
    ),
    (
        "DELETE",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111",
    ),
    ("POST", "/api/v1/admin/devices"),
    ("POST", "/api/v1/admin/network-nodes"),
    (
        "PUT",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111/prro-settings",
    ),
    ("POST", "/api/v1/admin/migrate/legacy"),
    ("POST", "/api/v1/products"),
    (
        "PUT",
        "/api/v1/products/11111111-1111-1111-1111-111111111111",
    ),
    ("POST", "/api/v1/categories"),
    ("POST", "/api/v1/suppliers"),
    ("POST", "/api/v1/sync/push"),
];

#[tokio::test]
async fn at_10_standby_without_upstream_write_url_is_503_not_500() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let pool = api_pool().await;
    apply_schema().await;

    let (_store, user_id, _supplier, _product) = seed_catalog(&pool).await;
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    // Стандартна конфігурація standby: `[node] upstream_write_url` НЕ задано
    // (`NodeConfig::default()` → None) і `server_url` у SQLite відсутній.
    let cfg = NodeConfig {
        mode: NodeMode::Standby,
        ..NodeConfig::default()
    };
    assert_eq!(
        cfg.resolve_upstream_write_url(),
        None,
        "AT-10: передумова — апстрім-запис не задано"
    );
    let app = router_v1::build_router(standby_state(
        Some(pool.clone()),
        None,
        None,
        pool.clone(),
    ));

    let before: Vec<(&str, i64)> = vec![
        ("stores", count(&pool, "stores").await),
        ("devices", count(&pool, "devices").await),
        ("network_nodes", count(&pool, "network_nodes").await),
        ("products", count(&pool, "products").await),
        ("categories", count(&pool, "categories").await),
        ("suppliers", count(&pool, "suppliers").await),
    ];

    for (method, path) in UPSTREAM_NOW_ROUTES {
        let (status, body, raw, headers) =
            call(&app, method, path, &token, Uuid::nil(), json!({"probe": true})).await;
        assert_ne!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "AT-10: {method} {path} → сирий 500 заборонено §4: {raw}"
        );
        assert!(
            !raw.contains("read-only transaction") && !raw.contains("sqlx"),
            "AT-10: {method} {path} → у тілі сирий текст PG/драйвера: {raw}"
        );
        if *path == "/api/v1/sync/push" {
            // Агрегатор-only приймач → той самий §4-контракт (клас DisabledOnStandby).
            assert_503_contract(&format!("AT-10 {method} {path}"), status, &body, &headers);
        } else {
            assert_503_contract(&format!("AT-10 {method} {path}"), status, &body, &headers);
        }
    }

    for (table, n) in before {
        assert_eq!(
            count(&pool, table).await,
            n,
            "AT-10: тихий запис у '{table}' (без upstream_write_url) — заборонено"
        );
    }
    eprintln!(
        "[AT-10] ✅ {} маршрутів → 503 §4 (0×500, 0 тихих записів у БД)",
        UPSTREAM_NOW_ROUTES.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// AT-14: standby — накладна: агрегат+позиції+outbox+stock в ОДНІЙ транзакції
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn at_14_standby_invoice_atomic_and_marker() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let admin_pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url(&db_url);
    ensure_readonly_role(&admin_pool, &db_name).await;

    let (store, user_id, supplier, product) = seed_catalog(&admin_pool).await;
    reset_local_queue();
    seed_sqlite_store_id(store);

    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&swap_credentials(&db_url, RO_ROLE, RO_PASS))
        .await
        .expect("read-only пул репліки");

    let v1: Arc<dyn InvoicesV1Service + Send + Sync> = Arc::new(OutboxInvoicesV1::new(Arc::new(
        SqlxInvoices::new(StorePool::new(ro_pool.clone())),
    )));
    let v2: Arc<dyn InvoicesV2Service + Send + Sync> = Arc::new(OutboxInvoicesV2::new(Arc::new(
        SqlxInvoices::new(StorePool::new(ro_pool.clone())),
    )));
    let app = router_v1::build_router(standby_state(Some(ro_pool.clone()), Some(v1), Some(v2), ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    // ── 1. Доказ: репліка ФІЗИЧНО read-only ────────────────────────────────
    let direct: Result<sqlx::postgres::PgRow, sqlx::Error> = sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'AT-14 мусить впасти')",
    )
    .bind(Uuid::new_v4())
    .fetch_one(&ro_pool)
    .await;
    let err = direct.expect_err("репліка мусить бути read-only");
    assert!(
        err.to_string().contains("read-only"),
        "очікували `read-only transaction`, маємо: {err}"
    );

    // ── 2. Накладна каси офлайн → 201/queued ───────────────────────────────
    let (status, dto, raw, _h) = call(
        &app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        invoice_body(supplier, product, "AT14-1", "3.000", "100.00"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "AT-14: каса на standby → 201/queued, маємо {status}: {raw}"
    );
    assert_eq!(dto["status"], "queued", "AT-14: бізнес-статус черги: {dto}");
    let cu = dto["id"].as_str().expect("client_uuid").to_string();

    // ── 3. АТОМАРНІСТЬ: той самий client_uuid в агрегаті, позиціях, outbox ──
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    let agg: (i64, String) = conn
        .query_row(
            "SELECT synced, data FROM invoices WHERE client_uuid = ?1",
            [&cu],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("агрегат invoices");
    assert_eq!(agg.0, 1, "AT-14: агрегат каси позначено як push-кандидат");
    assert!(agg.1.contains("AT14-1"), "data = payload як є: {}", agg.1);
    let items: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM invoice_items WHERE invoice_client_uuid = ?1 AND product_id = ?2",
            rusqlite::params![&cu, product.to_string()],
            |r| r.get(0),
        )
        .expect("позиції накладної");
    assert_eq!(items, 1, "AT-14: деталізація у ТІЙ САМІЙ транзакції");
    let ob: (String, i64) = conn
        .query_row(
            "SELECT status, COUNT(*) FROM outbox WHERE type = 'invoice' AND client_uuid = ?1",
            [&cu],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("outbox-запис накладної");
    assert_eq!(ob.0, "pending", "AT-14: маркер «очікує синку» = pending");
    assert_eq!(ob.1, 1, "AT-14: рівно один outbox-запис");
    assert_eq!(
        torgashka_infrastructure::offline::stock::get_stock_level(
            &conn,
            &store.to_string(),
            &product.to_string()
        )
        .expect("локальний stock"),
        3_000,
        "AT-14: SQLite stock +3.000 у тій самій транзакції"
    );
    assert_eq!(
        pending_count(&conn).expect("pending_count"),
        1,
        "AT-14: маркер у черзі push-циклу"
    );

    // ── 4. Репліка недоторкана ─────────────────────────────────────────────
    assert_eq!(
        pg_invoice_rows(&admin_pool, store).await,
        0,
        "AT-14: на standby документ НЕ пише в репліку"
    );
    assert_eq!(
        pg_stock(&admin_pool, store, product).await,
        None,
        "AT-14: авторитетний stock недоторканий (його рахує primary)"
    );

    // ── 5. ROLLBACK контуру: провал stock-ефекту котить усе ────────────────
    //
    // АНОМАЛІЯ (див. звіт): ADR §5 (AT-14) обіцяє rollback на «невалідний
    // product_id», але локальний контур продукт НЕ валідує (`stock` (0005) і
    // `invoice_items` (0010) без FK; `transactions.rs::enqueue_invoice`
    // перевірок product_id не має) — такий payload створив би фантомний
    // stock-рядок замість rollback. Тому атомарність доводимо РЕАЛЬНИМ
    // провалом ефекту в тому самому контурі (`enqueue_transaction` →
    // `apply_effects` → Err), а не підганяємо тест під очікування ADR.
    let outbox_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))
        .expect("outbox до");
    let cash_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM cash_ledger", [], |r| r.get(0))
        .expect("cash_ledger до");
    let mut conn_mut = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    let failed = transactions::enqueue_transaction(
        &mut conn_mut,
        transactions::TYPE_CASH_OPERATION,
        &json!({"operation_type": "НЕВАЛІДНИЙ", "cash_type": "cash", "amount": 100}).to_string(),
        &store.to_string(),
    );
    assert!(
        failed.is_err(),
        "AT-14 (rollback): невалідна касова операція мусить провалити ефект"
    );
    assert_eq!(
        conn_mut
            .query_row("SELECT COUNT(*) FROM cash_ledger", [], |r| r.get::<_, i64>(0))
            .expect("cash_ledger після"),
        cash_before,
        "AT-14 (rollback): агрегат не осів"
    );
    assert_eq!(
        conn_mut
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .expect("outbox після"),
        outbox_before,
        "AT-14 (rollback): outbox-запис не осів (атомарність контуру)"
    );
    eprintln!(
        "[AT-14] ✅ агрегат+позиції+outbox(pending)+stock=3.000 → атомарно; репліка 0 рядків; rollback контуру підтверджено"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// AT-15: звірка локальний (SQLite) ↔ авторитетний (PG) + дельта + інвентаризація
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn at_15_local_vs_authoritative_stock_delta_and_alignment() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let admin_pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url(&db_url);
    ensure_readonly_role(&admin_pool, &db_name).await;

    let (store, user_id, supplier, product) = seed_catalog(&admin_pool).await;
    reset_local_queue();
    seed_sqlite_store_id(store);

    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&swap_credentials(&db_url, RO_ROLE, RO_PASS))
        .await
        .expect("read-only пул репліки");
    let v1: Arc<dyn InvoicesV1Service + Send + Sync> = Arc::new(OutboxInvoicesV1::new(Arc::new(
        SqlxInvoices::new(StorePool::new(ro_pool.clone())),
    )));
    let v2: Arc<dyn InvoicesV2Service + Send + Sync> = Arc::new(OutboxInvoicesV2::new(Arc::new(
        SqlxInvoices::new(StorePool::new(ro_pool.clone())),
    )));
    let app = router_v1::build_router(standby_state(Some(ro_pool.clone()), Some(v1), Some(v2), ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    // ── 1. Офлайн-накладна #1 (+3): локальний оптимістичний залишок ────────
    let (st1, dto1, raw1, _) = call(
        &app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        invoice_body(supplier, product, "AT15-1", "3.000", "100.00"),
    )
    .await;
    assert_eq!(st1, StatusCode::CREATED, "AT-15: {st1} {raw1}");
    assert_eq!(dto1["status"], "queued", "AT-15: {dto1}");

    let local_before_push = local_stock_milli(product);
    let auth_before_push: f64 = pg_stock(&admin_pool, store, product)
        .await
        .map(|s| s.parse().unwrap_or(0.0))
        .unwrap_or(0.0);
    assert_eq!(local_before_push, 3_000, "AT-15: локальний (оптимістичний)");
    assert_eq!(auth_before_push, 0.0, "AT-15: авторитетний ще не бачив документа");
    let delta_before = local_before_push as f64 / 1000.0 - auth_before_push;
    assert_eq!(delta_before, 3.0, "AT-15: дельта локальний−авторитетний");

    // ── 2. Push на реальний primary → авторитетний залишок +3 ──────────────
    std::env::set_var(torgashka_api::RUST_INVOICES_ENV, "1");
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = torgashka_api::run_facade(&format!("127.0.0.1:{port}"));
    let client = reqwest::Client::new();
    let push_token = {
        // Логін каси на primary (owner/admin, як у sync_invoice_push_e2e).
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
             VALUES ($1, 'AT15 Admin', 'admin', '$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e', 'owner'::public.user_role, true, now(), now(), true) \
             ON CONFLICT (login) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .execute(&admin_pool)
        .await
        .expect("seed admin");
        sqlx::query(
            "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at) \
             SELECT u.id, $1, 'owner', '{}'::jsonb, true, now() FROM users u WHERE u.login = 'admin' \
             ON CONFLICT DO NOTHING",
        )
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("seed user_stores");
        let mut token = String::new();
        for _ in 0..60 {
            if let Ok(r) = client
                .post(format!("{base}/api/v1/auth/login"))
                .json(&json!({"login": "admin", "password": "admin123"}))
                .send()
                .await
            {
                if r.status().is_success() {
                    let v: Value = r.json().await.expect("login json");
                    token = v["access_token"].as_str().expect("token").to_string();
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert!(!token.is_empty(), "AT-15: логін каси на primary");
        token
    };
    let db_path = offline_db_path();
    let cfg = PushConfig {
        base_url: base.clone(),
        token: push_token,
        store_id: Some(store.to_string()),
        db_path: db_path.clone(),
        interval_secs: 30,
    };
    let summary = torgashka_infrastructure::offline::sync_push::push_pending_batch(
        &db_path, &client, &cfg,
    )
    .await
    .expect("push на primary");
    assert_eq!(summary.done, 1, "AT-15: push доставив накладну: {summary:?}");
    assert_eq!(summary.failed, 0, "AT-15: {summary:?}");

    let auth_after: f64 = pg_stock(&admin_pool, store, product)
        .await
        .expect("авторитетний stock після приймача")
        .parse()
        .expect("numeric");
    assert_eq!(auth_after, 3.0, "AT-15: primary застосував stock +3");
    let local_after = local_stock_milli(product);
    assert_eq!(local_after, 3_000, "AT-15: локальний не змінився push-ом");
    assert_eq!(
        local_after as f64 / 1000.0 - auth_after,
        0.0,
        "AT-15: після синку дельта = 0 (числа зійшлися)"
    );

    // ── 3. Локальна (ще не синхронізована) накладна #2 → дельта ≠ 0 ────────
    let (st2, _dto2, raw2, _) = call(
        &app,
        "POST",
        "/api/v1/invoices",
        &token,
        store,
        invoice_body(supplier, product, "AT15-2", "2.000", "100.00"),
    )
    .await;
    assert_eq!(st2, StatusCode::CREATED, "AT-15: {st2} {raw2}");
    let local2 = local_stock_milli(product);
    let delta2 = local2 as f64 / 1000.0 - auth_after;
    assert_eq!(local2, 5_000, "AT-15: локальний = 3+2");
    assert_eq!(delta2, 2.0, "AT-15: дельта локальний−авторитетний = 2.000");

    // Локальний каталог (products_v2) — як після master-pull: `stock_with_catalog`
    // показує лише товари каталогу точки, тому без рядка каталогу локальний
    // залишок у переліку не видно (це не «зникнення» залишку, а межа вибірки
    // LEFT JOIN products_v2 — фіксуємо в коментарі, бо для звірки це важливо).
    {
        let conn = rusqlite::Connection::open(&db_path).expect("SQLite каси");
        conn.execute(
            "INSERT INTO products_v2 (id, name, price, is_deleted, server_version) \
             VALUES (?1, 'AT15 Товар (pull)', 100.0, 0, 1) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            [product.to_string()],
        )
        .expect("products_v2 (локальний каталог)");
    }

    // ── 4. Вирівнювання НАЯВНИМ механізмом: інвентаризація (set_stock_level) ─
    let mut conn = rusqlite::Connection::open(&db_path).expect("SQLite каси");
    transactions::enqueue_transaction(
        &mut conn,
        transactions::TYPE_INVENTORY,
        &json!({
            "number": "AT15-INV",
            "items": [{"product_id": product.to_string(), "fact_quantity": 3.0}]
        })
        .to_string(),
        &store.to_string(),
    )
    .expect("інвентаризація (set_stock_level)");
    let local_aligned = local_stock_milli(product);
    assert_eq!(
        local_aligned, 3_000,
        "AT-15: інвентаризація вирівняла локальний залишок до авторитетного"
    );
    assert_eq!(
        local_aligned as f64 / 1000.0 - auth_after,
        0.0,
        "AT-15: дельта після вирівнювання = 0"
    );

    // ── 5. АНОМАЛІЯ: окремої поверхні «обидва числа + дельта» в коді НЕМА ──
    // ADR §10.3 п.3 обіцяє ПОКАЗ обох чисел із дельтою; у коді існують лише
    // примітиви (`stock::stock_with_catalog` / `get_stock_levels` — локальний,
    // PG-читання — авторитетний) і `set_stock_level`. Тест звіряє саме
    // ПРИМІТИВИ (числа + дельта), поверхні не вигадує — розходження ADR↔код
    // зафіксовано як АНОМАЛІЯ у звіті.
    let local_catalog = {
        let rows = torgashka_infrastructure::offline::stock::stock_with_catalog(
            &conn,
            &store.to_string(),
        )
        .expect("stock_with_catalog");
        rows.iter()
            .find(|(id, _, _)| id == &product.to_string())
            .map(|(_, _, milli)| *milli)
            .unwrap_or(0)
    };
    assert_eq!(local_catalog, local_aligned, "AT-15: каталог + залишки (локальне число)");
    eprintln!(
        "[AT-15] ✅ локальний={local_aligned}‰ авторитетний={auth_after} дельта=0; пре-push дельта=3.000; локальна #2 дала дельту 2.000 → вирівняно інвентаризацією"
    );
}
