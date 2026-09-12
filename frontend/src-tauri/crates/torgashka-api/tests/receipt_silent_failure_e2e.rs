//! E2E: **НЕВДАЛИЙ ЗАПИС ЧЕКА НЕ СМІЄ ВИГЛЯДАТИ ЯК УСПІХ** (QA-аудит класу
//! «2xx при невдачі / проковтнута помилка / no-op-успіх»).
//!
//! Гілка: `feat/pg-replication`, HEAD `e98ba05` (⊃ `ffba5a0`, ⊃ `e8c185f`).
//!
//! Клас дефекту. На standby-вузлі `state.pos = OutboxPos` (`lib.rs:296`):
//! будь-який POS-документ іде в SQLite-чергу і повертає **202 Accepted** з
//! `queued` — незалежно від того, чи існує взагалі канал доставки
//! (primary/upstream). На одновузловій машині (embedded PostgreSQL = read-only
//! репліка, primary немає) це дає «no-op-успіх»:
//!   * `POST /api/v2/receipts/sale` → 202, жодної помилки для касира;
//!   * у PG-джерелі істини — 0 чеків (запис НЕ відбувся);
//!   * у PG `stock` не змінився (товар не списано);
//!   * `GET /api/v2/receipts` (читає PG-репліку) чек НЕ показує.
//!
//! Інваріант, який перевіряє тест:
//!   2xx на `POST` чека допустиме ЛИШЕ якщо (а) чек реально є в PG-джерелі
//!   істини, АБО (б) черга має КОНФІГУРОВАНИЙ канал доставки
//!   (`NodeConfig::has_configured_upstream()` / налаштований `server_url`).
//!   На HEAD не виконується ні (а), ні (б) → тест падає (FAILED) ДО фіксу.
//!
//! Чому не мок: PG — реальний (тестова БД через `common::force_test_db`),
//! репліка — реальна read-only роль (`default_transaction_read_only = on`,
//! прямий INSERT через неї дає `cannot execute INSERT in a read-only
//! transaction`), черга — реальний SQLite (`XDG_DATA_HOME` → temp).

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use torgashka_api::auth::create_access_token;
use torgashka_api::{router_v1, AppState};
use torgashka_domain::PosService;
use torgashka_infrastructure::node_config::NodeConfig;
use torgashka_infrastructure::repositories::outbox_pos::{OutboxPos, QUEUED_STATUS};
use torgashka_infrastructure::repositories::pos::SqlxPos;
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "receipt-silent-failure-e2e-secret";
const RO_ROLE: &str = "torgashka_e2e_ro_receipt";
const RO_PASS: &str = "torgashka_e2e_ro_receipt_pwd";

// ─────────────────────────────────────────────────────────────────────────────
// Ізоляція середовища
// ─────────────────────────────────────────────────────────────────────────────

/// Ізольований SQLite каси: `XDG_DATA_HOME` → temp (один раз на бінар).
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

/// `db_sources.toml` БЕЗ секції `[node]` і БЕЗ активного джерела → жодного
/// «випадкового» апстріму з диска машини (детермінованість прекондиції).
fn isolate_sources() -> &'static std::path::Path {
    static CFG: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    CFG.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("torgashka_receipt_silent_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp-каталог db_sources");
        let cfg = dir.join("db_sources.toml");
        std::fs::write(
            &cfg,
            "# e2e receipt_silent_failure: без [node], без active\n",
        )
        .expect("db_sources.toml");
        std::env::set_var("TORGASHKA_DB_SOURCES", &cfg);
        cfg
    })
}

fn offline_db_path() -> std::path::PathBuf {
    torgashka_infrastructure::offline::db::OfflineDatabase::default_db_path().expect("шлях SQLite")
}

/// Записує `store_id` у SQLite-налаштування каси (як активація каси).
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

/// Чи є в SQLite каси налаштований сервер доставки черги (`server_url`).
fn sqlite_has_server_url() -> bool {
    let Ok(conn) = rusqlite::Connection::open(offline_db_path()) else {
        return false;
    };
    conn.query_row(
        "SELECT value FROM settings WHERE key IN ('server_url','sync_server_url') LIMIT 1",
        [],
        |r| r.get::<_, String>(0),
    )
    .map(|v| !v.trim().is_empty())
    .unwrap_or(false)
}

/// `postgresql://user:pass@host:port/db` → URL із підміненими credentials.
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

/// Обидва тести бінаря ділять ОДИН SQLite каси (`isolate_sqlite`) і одну
/// read-only роль у PG, тому паралельний прогін (як робить `cargo test
/// --workspace`) дає гонки ПІДГОТОВКИ, а не помилки продукту:
///   * PG-каталог: `XX000 tuple concurrently updated` на `ALTER ROLE`;
///   * SQLite: гонка міграцій `duplicate column name: client_uuid`.
///
/// Серіалізуємо самі тести (кожен ~0.5 s). Жодного твердження це не торкається.
/// Мьютекс — async-сумісний (`tokio::sync`), бо гвард свідомо тримається через
/// await-точки тіла тесту: він і має блокувати паралельний тест до кінця.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Підготовка read-only ролі в PG-каталозі (`pg_authid`) — СПІЛЬНИЙ ресурс:
/// обидва тести цього бінаря виконуються паралельно і б'ють у той самий рядок
/// каталогу, звідси `XX000 "tuple concurrently updated"` (heapam.c:4312) на
/// `ALTER ROLE`. Повторюємо каталог-операцію (ідемпотентна підготовка), щоб
/// збій ПІДГОТОВКИ не маскував результат ПЕРЕВІРКИ. Жодного твердження тесту
/// тут немає: у разі реальної помилки — той самий panic, що й раніше.
async fn pg_prep(pool: &sqlx::PgPool, what: &str, sql: &str) {
    const ATTEMPTS: u32 = 6;
    for attempt in 1..=ATTEMPTS {
        match sqlx::query(sql).execute(pool).await {
            Ok(_) => return,
            Err(e) => {
                let concurrent = e.to_string().contains("tuple concurrently updated");
                if concurrent && attempt < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(120 * attempt as u64));
                    continue;
                }
                panic!("{what}: {e}");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Стан фасаду (як на реальній одновузловій машині: standby + read-only репліка)
// ─────────────────────────────────────────────────────────────────────────────

fn standby_state(pos: Arc<dyn PosService + Send + Sync>, pool: sqlx::PgPool) -> AppState {
    AppState {
        jwt_secret: Arc::new(SECRET.to_string()),
        readdirs: None,
        write: None,
        write_pool: Some(pool.clone()),
        pos: Some(pos),
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
        // ОДНОВУЗЛОВА машина: апстрім НЕ заданий (жодного поля).
        node_config: NodeConfig::default(),
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
) -> (StatusCode, Value) {
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
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

// ─────────────────────────────────────────────────────────────────────────────
// Тест
// ─────────────────────────────────────────────────────────────────────────────

/// **Головний тест аудиту.** На HEAD `e98ba05` — FAILED (202 + 0 рядків у PG).
#[tokio::test]
async fn failed_receipt_write_must_not_look_like_success() {
    let _serial = SERIAL.lock().await; // гонки підготовки: див. SERIAL
    common::force_test_db();
    isolate_sqlite();
    isolate_sources();

    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    // ── 0. Реальна PG + реальний seed ───────────────────────────────────────
    let admin_pool = torgashka_infrastructure::db::connect_readonly_pool(3)
        .await
        .expect("writable-пул тестової БД");
    let db_name = db_name_from_url(&db_url);

    // Read-only роль (репліка): та сама підготовка, що в prod-standby.
    {
        let role = RO_ROLE;
        pg_prep(&admin_pool, "CREATE ROLE (read-only роль)", &format!("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{role}') THEN CREATE ROLE {role} LOGIN PASSWORD '{RO_PASS}'; END IF; END $$;")).await;
        pg_prep(
            &admin_pool,
            "ALTER ROLE read_only",
            &format!("ALTER ROLE {role} SET default_transaction_read_only = on"),
        )
        .await;
        pg_prep(
            &admin_pool,
            "GRANT CONNECT",
            &format!("GRANT CONNECT ON DATABASE \"{db_name}\" TO {role}"),
        )
        .await;
        pg_prep(
            &admin_pool,
            "GRANT USAGE",
            &format!("GRANT USAGE ON SCHEMA public TO {role}"),
        )
        .await;
        pg_prep(
            &admin_pool,
            "GRANT SELECT",
            &format!("GRANT SELECT ON ALL TABLES IN SCHEMA public TO {role}"),
        )
        .await;
    }

    let store = Uuid::new_v4();
    let product = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let category = Uuid::new_v4();

    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Silent Failure') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("INSERT stores");
    sqlx::query("INSERT INTO categories (id, name) VALUES ($1, 'E2E Silent Cat') ON CONFLICT (id) DO NOTHING")
        .bind(category)
        .execute(&admin_pool)
        .await
        .expect("INSERT categories");
    sqlx::query(
        "INSERT INTO products (id, title, category_id, price, cost_price, stock) \
         VALUES ($1, 'E2E Silent Товар', $2, 10.50, 5.00, 100) ON CONFLICT (id) DO NOTHING",
    )
    .bind(product)
    .bind(category)
    .execute(&admin_pool)
    .await
    .expect("INSERT products");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Silent Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind(format!("silent_e2e_{}", &user_id.to_string()[..8]))
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

    // Залишок товару в PG ДО чека (джерело істини для «списано/не списано»).
    let pg_stock_before: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity),0)::bigint FROM stock WHERE product_id = $1",
    )
    .bind(product)
    .fetch_one(&admin_pool)
    .await
    .unwrap_or(0);

    // ── 1. Доказ: локальний PG — РЕАЛЬНО read-only (як на Windows-касі) ────
    let ro_url = swap_credentials(&db_url, RO_ROLE, RO_PASS);
    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&ro_url)
        .await
        .expect("read-only пул репліки");
    let ro_err = sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, updated_at) VALUES ($1, $2, 1, now())",
    )
    .bind(store)
    .bind(product)
    .execute(&ro_pool)
    .await
    .expect_err("репліка мусить бути read-only");
    assert!(
        ro_err.to_string().contains("read-only transaction"),
        "роль репліки не read-only: {ro_err}"
    );
    eprintln!("[e2e] репліка справді read-only: {ro_err}");

    // ── 2. Фасад standby-вузла з OboutPos (нова обв'язка, lib.rs:296) ───────
    seed_sqlite_store_id(store);
    let node = NodeConfig::default();
    // Прекондиція «каналу доставки НЕМАЄ» — інакше тест неінформативний.
    assert!(
        !node.has_configured_upstream(),
        "прекондиція: апстрім не заданий"
    );
    // Прекондиція «ніщо не блокує відправку» більше не має об'єкта: ADR-0008 (E7)
    // видалив і режими вузла, і ґейт блокування push — HTTP-шлях касової черги
    // вільний за побудовою. Сам assert знято ТІЛЬКИ через зникнення його
    // предмета; усі змістовні асерти цього тесту не змінювались.
    assert!(
        !sqlite_has_server_url(),
        "прекондиція: у SQLite каси не задано server_url"
    );

    let ro_store_pool = StorePool::new(ro_pool.clone());
    let pos: Arc<dyn PosService + Send + Sync> =
        Arc::new(OutboxPos::new(Arc::new(SqlxPos::new(ro_store_pool))));
    let app = router_v1::build_router(standby_state(pos, ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    // ── 3. POST чека (продаж 2 шт) ──────────────────────────────────────────
    let (status, receipt) = call(
        &app,
        "POST",
        "/api/v2/receipts/sale",
        &token,
        store,
        json!({
            "items": [{"product_id": product, "name": "E2E Silent Товар", "quantity": 2,
                       "price": 10.5, "tax_rate": 20}],
            "payment_method": "cash",
            "cash_amount": 21.0,
            "is_fiscal": true,
            "notes": "receipt_silent_failure_e2e"
        }),
    )
    .await;
    let receipt_id = receipt["id"].as_str().unwrap_or_default().to_string();

    // ── 4. Незалежні докази стану (поза фасадом) ────────────────────────────
    let in_pg: i64 = if receipt_id.is_empty() {
        0
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE id = $1::uuid")
            .bind(&receipt_id)
            .fetch_one(&admin_pool)
            .await
            .expect("count receipts у PG")
    };
    let in_pg_sqlite: i64 = {
        let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
        conn.query_row(
            "SELECT COUNT(*) FROM receipts WHERE client_uuid = ?1",
            [&receipt_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let pending_outbox: i64 = {
        let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
        conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let pg_stock_after: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity),0)::bigint FROM stock WHERE product_id = $1",
    )
    .bind(product)
    .fetch_one(&admin_pool)
    .await
    .unwrap_or(0);

    // Чи БАЧИТЬ касир свій чек у списку (UI читає PG-репліку).
    let (list_status, list) = call(
        &app,
        "GET",
        "/api/v2/receipts?page=1&size=50",
        &token,
        store,
        Value::Null,
    )
    .await;
    let listed = list["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .any(|i| i["id"].as_str() == Some(receipt_id.as_str()))
        })
        .unwrap_or(false);

    let deliverable = node.has_configured_upstream() || sqlite_has_server_url();
    let durable = in_pg > 0;

    eprintln!("[e2e][evidence] POST /api/v2/receipts/sale → HTTP {status}");
    eprintln!("[e2e][evidence] body: {receipt}");
    eprintln!(
        "[e2e][evidence] fiscal_status = {}",
        receipt["fiscal_status"]
    );
    eprintln!("[e2e][evidence] чек у PG (receipts): {in_pg}");
    eprintln!("[e2e][evidence] чек у SQLite (receipts, client_uuid): {in_pg_sqlite}");
    eprintln!("[e2e][evidence] outbox pending: {pending_outbox}");
    eprintln!("[e2e][evidence] PG stock: {pg_stock_before} → {pg_stock_after}");
    eprintln!("[e2e][evidence] GET /api/v2/receipts → HTTP {list_status}, чек у списку: {listed}");
    eprintln!(
        "[e2e][evidence] доставність черги: has_configured_upstream={}, server_url={}",
        node.has_configured_upstream(),
        sqlite_has_server_url()
    );

    // ── 5. ІНВАРІАНТ ────────────────────────────────────────────────────────
    assert!(
        !(status.is_success() && !durable && !deliverable),
        "ПРОВАЛЕНИЙ ЗАПИС ЧЕКА ВИГЛЯДАЄ ЯК УСПІХ: POST → HTTP {status} \
         (fiscal_status={}), чеків у PG-джерелі істини = {in_pg}, \
         черга доставна = {deliverable}, PG stock {pg_stock_before}→{pg_stock_after}, \
         чек у списку GET /api/v2/receipts = {listed}. \
         Касир бачить «успіх», БД не змінена, черга не має куди піти.",
        receipt["fiscal_status"]
    );

    assert!(
        !status.is_success(),
        "невдалий запис чека мусить давати НЕ-2xx (маємо {status}: {receipt})"
    );
}

/// Негативний контроль (НЕ є предметом дефекту, фіксує межу класу):
/// та сама read-only БД + СТАРА обв'язка (`SqlxPos` напряму) → 500, тобто
/// помилка ВИДИМА; «тече» лише текст (санітизовано §D).
#[tokio::test]
async fn legacy_sqlx_pos_on_readonly_replica_is_visible_500() {
    let _serial = SERIAL.lock().await; // гонки підготовки: див. SERIAL
    common::force_test_db();
    isolate_sqlite();
    isolate_sources();
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let admin_pool = torgashka_infrastructure::db::connect_readonly_pool(3)
        .await
        .expect("пул");
    let db_name = db_name_from_url(&db_url);
    let role = RO_ROLE;
    let _ = db_name;

    // Роль уже створена першим тестом у цьому ж бінарі; якщо ні — створимо.
    pg_prep(&admin_pool, "CREATE ROLE", &format!("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{role}') THEN CREATE ROLE {role} LOGIN PASSWORD '{RO_PASS}'; END IF; END $$;")).await;
    pg_prep(
        &admin_pool,
        "ALTER ROLE",
        &format!("ALTER ROLE {role} SET default_transaction_read_only = on"),
    )
    .await;
    pg_prep(
        &admin_pool,
        "GRANT CONNECT",
        &format!("GRANT CONNECT ON DATABASE \"{db_name}\" TO {role}"),
    )
    .await;
    pg_prep(
        &admin_pool,
        "GRANT USAGE",
        &format!("GRANT USAGE ON SCHEMA public TO {role}"),
    )
    .await;
    pg_prep(
        &admin_pool,
        "GRANT SELECT",
        &format!("GRANT SELECT ON ALL TABLES IN SCHEMA public TO {role}"),
    )
    .await;

    let store = Uuid::new_v4();
    let product = Uuid::new_v4();
    let category = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Legacy Silent') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("stores");
    sqlx::query("INSERT INTO categories (id, name) VALUES ($1, 'E2E Legacy Cat') ON CONFLICT (id) DO NOTHING")
        .bind(category)
        .execute(&admin_pool)
        .await
        .expect("categories");
    sqlx::query(
        "INSERT INTO products (id, title, category_id, price, cost_price, stock) \
         VALUES ($1, 'E2E Legacy Товар', $2, 10.50, 5.00, 100) ON CONFLICT (id) DO NOTHING",
    )
    .bind(product)
    .bind(category)
    .execute(&admin_pool)
    .await
    .expect("products");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Legacy Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind(format!("legacy_e2e_{}", &user_id.to_string()[..8]))
    .execute(&admin_pool)
    .await
    .expect("users");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at) \
         VALUES ($1, $2, 'admin', '{}'::jsonb, true, now()) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(store)
    .execute(&admin_pool)
    .await
    .expect("user_stores");

    seed_sqlite_store_id(store);
    let ro_url = swap_credentials(&db_url, RO_ROLE, RO_PASS);
    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&ro_url)
        .await
        .expect("ro pool");
    let legacy: Arc<dyn PosService + Send + Sync> =
        Arc::new(SqlxPos::new(StorePool::new(ro_pool.clone())));
    let app = router_v1::build_router(standby_state(legacy, ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    let (status, body) = call(
        &app,
        "POST",
        "/api/v2/receipts/sale",
        &token,
        store,
        json!({
            "items": [{"product_id": product, "quantity": 1, "price": 10.5, "tax_rate": 20}],
            "payment_method": "cash"
        }),
    )
    .await;
    assert!(
        !status.is_success(),
        "стара обв'язка на read-only репліці мусить бути ВИДИМОЮ помилкою, маємо {status}: {body}"
    );
    eprintln!("[e2e] legacy/read-only → HTTP {status}: {body}");
    let _ = QUEUED_STATUS;
}
