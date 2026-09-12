//! E2E: POS-документи каси на **standby-вузлі** (ADR-0007 §10, §11.1, §11.4 п.3).
//!
//! Перевіряється весь ланцюг: HTTP-фасад (standby) → `OutboxPos` → SQLite-черга
//! (`LocalOutbox`) при **реально read-only** репліці PostgreSQL. До виправлення
//! `state.pos` на standby був `SqlxPos(репліка)` → кожен чек падав з
//! `cannot execute INSERT in a read-only transaction` (500).
//!
//! Що доводиться (кожен пункт — реальний assert, не переказ):
//!   1. репліка ФІЗИЧНО read-only: окрема роль
//!      `ALTER ROLE ... SET default_transaction_read_only = on`, прямий INSERT
//!      через неї → помилка `read-only transaction`;
//!   2. `POST /api/v2/receipts/sale` (та списання/переміщення) на standby →
//!      **202 Accepted** з ознакою `queued` (НЕ 500);
//!   3. у PG `receipts`/`write_offs`/`transfers` — **0 рядків**;
//!   4. у SQLite `outbox` — рівно **1** рядок `pending` потрібного типу на
//!      документ; локальний агрегат має той самий `client_uuid` (synced=1);
//!   5. локальний stock-ефект виконано в тій самій транзакції (дизайн 4.4):
//!      продаж qty=2 → локальний залишок −2000 (мілі-одиниці);
//!   6. **негативний контроль** — та сама read-only БД + СТАРА обв'язка
//!      (`state.pos = SqlxPos(read-only)`) → 500, і тіло без сирого тексту PG
//!      (§D: технічний текст іде в `torgashka.log`, користувачу — стабільне).
//!
//! SQLite ізольовано через `XDG_DATA_HOME` (temp): `db.rs:83` бере
//! `dirs_next::data_dir()/torgashka/offline.db`, лог — `…/torgashka.log`.

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use torgashka_api::auth::create_access_token;
use torgashka_api::{router_v1, AppState};
use torgashka_domain::PosService;
use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use torgashka_infrastructure::repositories::outbox_pos::{OutboxPos, QUEUED_STATUS};
use torgashka_infrastructure::repositories::pos::SqlxPos;
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "pos-standby-outbox-e2e-secret";
const RO_ROLE: &str = "torgashka_e2e_ro";
const RO_PASS: &str = "torgashka_e2e_ro_pwd";

// ─────────────────────────────────────────────────────────────────────────────
// Хелпери інфраструктури тесту
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

fn offline_db_path() -> std::path::PathBuf {
    torgashka_infrastructure::offline::db::OfflineDatabase::default_db_path().expect("шлях SQLite")
}

/// Шлях журналу — беремо в самого модуля (не вгадуємо):
/// `data_dir_default()` = `$XDG_DATA_HOME/Torgashka/pgdata`, журнал — поруч.
fn torgashka_log_path() -> std::path::PathBuf {
    torgashka_infrastructure::embedded_pg::log_file_path()
}

/// Записує `store_id` у SQLite-налаштування каси (як активація каси:
/// `settings.store_id`) — основне джерело `store_id` для `OutboxPos`.
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

/// Записує в SQLite каси КАНАЛ ДОСТАВКИ черги (`server_url` + `device_token`) —
/// як після активації точки на хабі (реальна каса без нього не буває).
///
/// Навіщо явно: `POST` документа дає **202 Accepted лише якщо черга доставна**
/// (інваріант з `receipt_silent_failure_e2e`: 202 — обіцянка доставки, а не
/// «щось колись станеться»). Тут хаб НЕДОСТУПНИЙ (порт 1) — це і є легітимний
/// offline-first: документ у черзі, касир бачить 202, доставка станеться, коли
/// мережа повернеться. Без каналу той самий запит мусить дати 503.
fn seed_sqlite_delivery_channel() {
    let path = offline_db_path();
    let conn =
        torgashka_infrastructure::offline::sync_push::open_connection(&path).expect("SQLite каси");
    for (key, value) in [
        ("server_url", "http://127.0.0.1:1/"),
        ("device_token", "e2e-device-token"),
    ] {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )
        .expect("settings каналу доставки");
    }
    // Прекондиція легітимного офлайну: канал НАЛАШТОВАНИЙ, але хаб недоступний.
    assert!(
        torgashka_infrastructure::offline::sync_push::queue_has_delivery_channel(),
        "канал доставки мусить бути сконфігурований"
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", 1u16)).is_err(),
        "прекондиція легітимного офлайну: хаб (127.0.0.1:1) мусить бути недоступний"
    );
    eprintln!(
        "[e2e][evidence] легітимний офлайн: канал сконфігуровано (server_url=http://127.0.0.1:1/, \
         хаб НЕДОСТУПНИЙ), queue_has_delivery_channel={}",
        torgashka_infrastructure::offline::sync_push::queue_has_delivery_channel()
    );
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

/// Серіалізація DDL по ролі: два тести бінаря не можуть одночасно робити
/// `ALTER ROLE`/`GRANT` (PG: `tuple concurrently updated`).
static ROLE_SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Роль з `default_transaction_read_only = on` + SELECT на схему `public`.
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

/// Стан фасаду standby-вузла: `pos` — або `OutboxPos` (нова обв'язка), або
/// голий `SqlxPos` (негативний контроль — СТАРА обв'язка).
fn standby_state(
    pos: Arc<dyn torgashka_domain::PosService + Send + Sync>,
    pool: sqlx::PgPool,
) -> AppState {
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

/// Чи є локальний агрегат із таким client_uuid (таблиця міграції 0006/0001).
fn local_aggregate_exists(table: &str, client_uuid: &str) -> bool {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    let n: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE client_uuid = ?1"),
            [client_uuid],
            |r| r.get(0),
        )
        .expect("count агрегата");
    n == 1
}

/// Локальний залишок товару в касі (мілі-одиниці, як `stock::get_stock_level`).
fn local_stock_milli(store: Uuid, product: Uuid) -> i64 {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    torgashka_infrastructure::offline::stock::get_stock_level(
        &conn,
        &store.to_string(),
        &product.to_string(),
    )
    .expect("локальний залишок")
}

// ─────────────────────────────────────────────────────────────────────────────
// Основний e2e
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn standby_pos_documents_go_to_local_outbox_not_to_replica() {
    common::force_test_db();
    let sqlite_dir = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    // ── 0. Підготовка: writable-пул (setup) + read-only роль (репліка) ──────
    let admin_pool = torgashka_infrastructure::db::connect_readonly_pool(3)
        .await
        .expect("writable-пул тестової БД");
    let db_name = db_name_from_url(&db_url);
    ensure_readonly_role(&admin_pool, &db_name).await;

    let store = Uuid::new_v4();
    let product = Uuid::new_v4();
    let other_store = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    // Своя точка + категорія/товар, щоб «0 рядків у PG» було точним.
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Standby Outbox') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(&admin_pool)
        .await
        .expect("INSERT stores");
    // Товар у PG потрібен лише НЕГАТИВНОМУ КОНТРОЛЮ: стара обв'язка
    // (`SqlxPos` на репліці) спершу читає ціну товару, і лише потім падає на
    // INSERT у read-only репліку. Нова обв'язка в PG не ходить узагалі.
    let category = Uuid::new_v4();
    sqlx::query("INSERT INTO categories (id, name) VALUES ($1, 'E2E Standby Cat') ON CONFLICT (id) DO NOTHING")
        .bind(category)
        .execute(&admin_pool)
        .await
        .expect("INSERT categories");
    sqlx::query(
        "INSERT INTO products (id, title, category_id, price, cost_price, stock) \
         VALUES ($1, 'E2E Standby Товар', $2, 10.50, 5.00, 100) ON CONFLICT (id) DO NOTHING",
    )
    .bind(product)
    .bind(category)
    .execute(&admin_pool)
    .await
    .expect("INSERT products");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Standby Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind(format!("standby_e2e_{}", &user_id.to_string()[..8]))
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

    // ── 1. Доказ read-only: той самий INSERT через роль репліки падає ──────
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
    let ro_err_text = ro_err.to_string();
    assert!(
        ro_err_text.contains("read-only transaction"),
        "роль репліки не read-only: {ro_err_text}"
    );
    eprintln!("[e2e] репліка справді read-only: {ro_err_text}");

    // ── 2. Standby-фасад із OutboxPos (нова обв'язка) ───────────────────────
    // Точка каси в SQLite-налаштуваннях: без неї `OutboxPos` віддає
    // 422 «точку продажу не налаштовано» (Validation, НЕ 500).
    seed_sqlite_store_id(store);
    // Легітимний offline-first: канал є, хаб недоступний (див. хелпер).
    seed_sqlite_delivery_channel();
    let ro_store_pool = StorePool::new(ro_pool.clone());
    let pos: Arc<dyn torgashka_domain::PosService + Send + Sync> =
        Arc::new(OutboxPos::new(Arc::new(SqlxPos::new(ro_store_pool))));
    let app = router_v1::build_router(standby_state(pos, ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    // ── 3. Чек продажу → 202 Accepted + queued (НЕ 500 read-only) ───────────
    let (status, receipt) = call(
        &app,
        "POST",
        "/api/v2/receipts/sale",
        &token,
        store,
        json!({
            "items": [{"product_id": product, "name": "E2E Товар", "quantity": 2, "price": 10.5, "tax_rate": 20}],
            "payment_method": "cash",
            "cash_amount": 21.0,
            "is_fiscal": true,
            "notes": "standby outbox e2e"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "чек на standby має бути 202/queued, маємо {status}: {receipt}"
    );
    assert_eq!(receipt["fiscal_status"], QUEUED_STATUS, "{receipt}");
    assert_eq!(receipt["total"], 21.0, "{receipt}");
    let receipt_uuid = receipt["id"].as_str().expect("client_uuid").to_string();

    // ── 4. Списання → 202 Accepted + queued ────────────────────────────────
    let (status, wo) = call(
        &app,
        "POST",
        "/api/v1/write-offs",
        &token,
        store,
        json!({
            "reason": "E2E псування",
            "write_off_date": "2026-09-11T12:00:00",
            "notes": "standby outbox e2e",
            "items": [{"product_id": product, "quantity": 1, "cost_price": 5.0, "price": 10.5}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "списання: {status} {wo}");
    assert_eq!(wo["status"], QUEUED_STATUS, "{wo}");
    let wo_uuid = wo["id"].as_str().expect("client_uuid").to_string();

    // ── 5. Переміщення → 202 Accepted + queued ─────────────────────────────
    let (status, tr) = call(
        &app,
        "POST",
        "/api/v1/transfers",
        &token,
        store,
        json!({
            "from_location": store.to_string(),
            "to_location": other_store.to_string(),
            "transfer_date": "2026-09-11T12:00:00",
            "items": [{"product_id": product, "quantity": 3, "price": 10.5}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "переміщення: {status} {tr}");
    assert_eq!(tr["status"], QUEUED_STATUS, "{tr}");
    let tr_uuid = tr["id"].as_str().expect("client_uuid").to_string();

    // ── 6. У PG — НУЛЬ документів (репліка не ціль запису) ─────────────────
    for table in ["receipts", "write_offs", "transfers"] {
        let n: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE store_id = $1"))
                .bind(store)
                .fetch_one(&admin_pool)
                .await
                .expect("count PG");
        assert_eq!(n, 0, "{table}: на standby в PG не має бути жодного рядка");
    }

    // ── 7. SQLite: рівно по 1 pending у черзі + локальні агрегати ──────────
    let rows = outbox_rows();
    assert_eq!(
        rows,
        vec![
            ("receipt".to_string(), "pending".to_string(), 1),
            ("transfer".to_string(), "pending".to_string(), 1),
            ("write_off".to_string(), "pending".to_string(), 1),
        ],
        "черга SQLite: {rows:?}"
    );
    assert!(local_aggregate_exists("receipts", &receipt_uuid));
    assert!(local_aggregate_exists("write_offs", &wo_uuid));
    assert!(local_aggregate_exists("transfers", &tr_uuid));

    // ── 8. Локальний stock-ефект у тій самій транзакції (дизайн 4.4) ───────
    // -2 (чек) -1 (списання) -3 (переміщення, каса = from) = -6 одиниць.
    assert_eq!(
        local_stock_milli(store, product),
        -6000,
        "локальний залишок каси (мілі-одиниці)"
    );

    // ── 9. НЕГАТИВНИЙ КОНТРОЛЬ: та сама read-only БД + СТАРА обв'язка ──────
    let legacy: Arc<dyn torgashka_domain::PosService + Send + Sync> =
        Arc::new(SqlxPos::new(StorePool::new(ro_pool.clone())));
    let app_legacy = router_v1::build_router(standby_state(legacy, ro_pool.clone()));
    let (status, body) = call(
        &app_legacy,
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
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "стара обв'язка мусить віддати 500: {body}"
    );
    let detail = body["detail"].as_str().unwrap_or_default();
    // §D: сирий текст PG у відповідь НЕ потрапляє (лише стабільне повідомлення).
    assert!(
        !detail.contains("read-only") && !detail.contains("cannot execute"),
        "тіло відповіді не має містити сирий текст PG: {detail}"
    );
    assert_eq!(
        detail, "Не вдалося зберегти зміну, спробуйте ще раз",
        "{body}"
    );
    // …а технічний текст лягає в torgashka.log (санація, а не втрата).
    let log = std::fs::read_to_string(torgashka_log_path()).unwrap_or_default();
    assert!(
        log.contains("read-only transaction"),
        "технічний текст мусить бути в torgashka.log ({}): {}",
        torgashka_log_path().display(),
        &log[log.len().saturating_sub(500)..]
    );
    // Нова обв'язка після негативного контролю не додала PG-рядків.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE store_id = $1")
        .bind(store)
        .fetch_one(&admin_pool)
        .await
        .expect("count PG receipts");
    assert_eq!(n, 0);
    assert_eq!(outbox_rows().iter().map(|r| r.2).sum::<i64>(), 3);

    // Прибирання тестових даних (категорія/товар/точка/користувач).
    let _ = sqlx::query("DELETE FROM user_stores WHERE user_id = $1")
        .bind(user_id)
        .execute(&admin_pool)
        .await;
    let _ = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&admin_pool)
        .await;
    let _ = sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(product)
        .execute(&admin_pool)
        .await;
    let _ = sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(category)
        .execute(&admin_pool)
        .await;
    let _ = sqlx::query("DELETE FROM stores WHERE id = $1")
        .bind(store)
        .execute(&admin_pool)
        .await;
    drop(sqlite_dir);
}

/// Операції без представлення в черзі → людська помилка (не паніка, не PG).
///
/// Залишкові відмови (ADR-0007 §11.6): довідник причин списання (гейт:
/// `ProxyToPrimary`, адаптер — друга лінія), update/delete/confirm документів
/// (у черзі немає типу дії над наявним документом), v1-чек із боргом.
#[tokio::test]
async fn unavailable_ops_refuse_with_human_message() {
    common::force_test_db();
    isolate_sqlite();
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let admin_pool = torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("пул");
    let db_name = db_name_from_url(&db_url);
    ensure_readonly_role(&admin_pool, &db_name).await;
    let ro_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&swap_credentials(&db_url, RO_ROLE, RO_PASS))
        .await
        .expect("read-only пул");

    let pos = OutboxPos::new(Arc::new(SqlxPos::new(StorePool::new(ro_pool.clone()))));
    let user = Uuid::new_v4();

    // Сутності без рядка §11.1 → відмова з людським текстом (§11.1 не вигадуємо).
    let err = pos
        .create_write_off_reason("E2E причина")
        .await
        .expect_err("причина списання без §11.1 → відмова");
    assert!(err.to_string().contains("причини списання"), "{err}");
    assert!(
        err.to_string()
            .contains("операція недоступна на цьому вузлі"),
        "{err}"
    );

    // КАСОВА ОПЕРАЦІЯ тут БІЛЬШЕ НЕ ЗАПЕРЕЧУЄТЬСЯ: з 2026-09-11 вона має
    // власний рядок §11.6 (`cash_operation` → LocalOutbox) і власний касовий
    // ефект (`offline/cash.rs`), тому йде в локальну чергу — покриття
    // позитивного шляху: `tests/cash_operation_standby_e2e.rs`.
    // (У цьому тесті `settings.store_id` у SQLite НЕ засіяний, тож прямий
    // виклик адаптера впав би на відсутній точці, а не на «недоступно» —
    // тому тут касовий шлях не перевіряємо.)

    // Оновлення/проведення наявних документів: у черзі немає типу update/confirm.
    let err = pos
        .confirm_write_off(Uuid::new_v4())
        .await
        .expect_err("confirm недоступний");
    assert!(
        err.to_string()
            .contains("операція недоступна на цьому вузлі"),
        "{err}"
    );
    let err = pos
        .delete_transfer(Uuid::new_v4())
        .await
        .expect_err("delete недоступний");
    assert!(
        err.to_string()
            .contains("операція недоступна на цьому вузлі"),
        "{err}"
    );

    // v1-чек із боргом: боргову частину черга не відтворює → явна відмова,
    // а не тиха втрата боргу.
    let err = pos
        .create_receipt_v1(&torgashka_domain::ReceiptV1CreateInput {
            receipt_number: None,
            receipt_type: "sale".to_string(),
            cashier_id: Some(user),
            total_amount: "10".to_string(),
            paid_amount: Some("0".to_string()),
            debtor_id: Some(Uuid::new_v4()),
            is_return: false,
            notes: None,
            original_receipt_id: None,
            return_reason: None,
            items: vec![torgashka_domain::ReceiptV1ItemInput {
                product_id: Uuid::new_v4(),
                quantity: "1".to_string(),
                price: "10".to_string(),
                total: None,
            }],
            debt_payment: None,
            payment_method: Some("cash".to_string()),
        })
        .await
        .expect_err("борговий v1-чек недоступний на standby");
    assert!(err.to_string().contains("борговою семантикою"), "{err}");
}
