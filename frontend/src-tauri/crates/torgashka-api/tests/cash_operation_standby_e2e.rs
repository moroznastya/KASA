//! E2E: КАСОВА ОПЕРАЦІЯ (внесення/інкасація) — standby + приймач на primary
//! (ADR-0007 §11.6, клас `LocalOutbox`; Alembic 0017).
//!
//! До цього `create_cash_operation` на standby відмовляв
//! (`unavailable` — сутність була поза §11.1), а ще раніше давав 500 з сирим
//! текстом `cannot execute INSERT in a read-only transaction`.
//!
//! ТЕСТ 1 `standby_cash_operation_queues_locally_and_never_writes_replica`:
//!   1. репліка ФІЗИЧНО read-only (роль `default_transaction_read_only = on`,
//!      прямий INSERT → `read-only transaction`);
//!   2. `POST /api/v1/cash-operations` на standby → **202 Accepted** (не 500);
//!   3. у PG `cash_operations` — **0 рядків** (репліка не ціль запису);
//!   4. у SQLite: агрегат `cash_ledger` (той самий client_uuid, synced=1) +
//!      рівно **1** outbox-запис `cash_operation` = `pending`;
//!   5. локальний баланс каси — **ефект у тій самій транзакції** (15000 коп.);
//!   6. **негативний контроль**: та сама read-only БД + СТАРА обв'язка
//!      (`state.pos = SqlxPos(репліка)`) → 500, у тілі НЕМАЄ сирого тексту PG
//!      (санація §D: технічний текст іде в `torgashka.log`), 0 рядків у PG і
//!      0 нових outbox-записів.
//!
//! ТЕСТ 2 `cash_operation_push_idempotent_on_primary`:
//!   каса створила операцію офлайн → push РЕАЛЬНИМ клієнтом (`push_pending_batch`)
//!   на піднятий primary → рівно **1** рядок `cash_operations` з `client_uuid`,
//!   сумою, типом і `created_at` каси; повторний push (done→pending) →
//!   `already_exists`, дублів немає (partial UNIQUE 0017).
//!
//! SQLite ізольовано через `XDG_DATA_HOME` (temp): `db.rs:83` бере
//! `dirs_next::data_dir()/torgashka/offline.db`.

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
use torgashka_infrastructure::offline::sync_push::{
    open_connection, pending_count, push_pending_batch, PushConfig,
};
use torgashka_infrastructure::offline::transactions;
use torgashka_infrastructure::repositories::outbox_pos::OutboxPos;
use torgashka_infrastructure::repositories::pos::SqlxPos;
use torgashka_infrastructure::store_ctx::StorePool;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "common/sync_schema.rs"]
mod sync_schema;

const SECRET: &str = "cash-standby-e2e-secret";
const RO_ROLE: &str = "torgashka_cash_e2e_ro";
const RO_PASS: &str = "torgashka_cash_e2e_ro_pwd";

// ─────────────────────────────────────────────────────────────────────────────
// Інфраструктура тесту
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

fn offline_db_path() -> PathBuf {
    torgashka_infrastructure::offline::db::OfflineDatabase::default_db_path().expect("шлях SQLite")
}

/// `store_id` у SQLite-налаштуваннях каси (як після активації каси).
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

/// Серіалізація DDL по ролі: тести бінаря не можуть одночасно робити
/// `ALTER ROLE`/`GRANT` (PG: `tuple concurrently updated`).
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

/// Стан фасаду standby-вузла з переданим `pos` (нова або стара обв'язка).
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

fn local_balance_cents(store: Uuid, cash_type: &str) -> i64 {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    torgashka_infrastructure::offline::cash::get_cash_balance(&conn, &store.to_string(), cash_type)
        .expect("локальний баланс каси")
}

fn local_cash_ledger_row(client_uuid: &str) -> Option<(i64, String)> {
    let conn = rusqlite::Connection::open(offline_db_path()).expect("SQLite каси");
    conn.query_row(
        "SELECT synced, data FROM cash_ledger WHERE client_uuid = ?1",
        [client_uuid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

async fn pg_cash_rows(pool: &sqlx::PgPool, store: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM cash_operations WHERE store_id = $1")
        .bind(store)
        .fetch_one(pool)
        .await
        .expect("COUNT cash_operations")
}

// ─────────────────────────────────────────────────────────────────────────────
// Спільна підготовка схеми (для тесту 2 — як у sync_invoice_push_e2e)
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ 1: standby — 202, локальна черга, 0 рядків у PG, негативний контроль
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn standby_cash_operation_queues_locally_and_never_writes_replica() {
    common::force_test_db();
    let _ = isolate_sqlite().to_path_buf();
    let db_url = std::env::var("DATABASE_URL").expect("force_test_db виставив DATABASE_URL");

    let admin_pool = api_pool().await;
    apply_schema().await;
    let db_name = db_name_from_url(&db_url);
    ensure_readonly_role(&admin_pool, &db_name).await;

    // Точка + адміністратор точки (потрібні для `require_admin` каси).
    let store = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Cash Точка') ON CONFLICT (id) DO NOTHING",
    )
    .bind(store)
    .execute(&admin_pool)
    .await
    .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'E2E Cash Admin', $2, 'x', 'admin'::public.user_role, true, now(), now(), true) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind(format!("cash_e2e_{}", &user_id.to_string()[..8]))
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
        "INSERT INTO cash_operations (id, store_id, user_id, operation_type, cash_type, amount) \
         VALUES ($1, $2, $3, 'deposit', 'cash', 1)",
    )
    .bind(Uuid::new_v4())
    .bind(store)
    .bind(user_id)
    .execute(&ro_pool)
    .await
    .expect_err("репліка мусить бути read-only");
    let ro_err_text = ro_err.to_string();
    assert!(
        ro_err_text.contains("read-only transaction"),
        "роль репліки не read-only: {ro_err_text}"
    );
    eprintln!("[cash e2e] репліка справді read-only: {ro_err_text}");

    // ── 2. Standby-фасад із OutboxPos (нова обв'язка) ───────────────────────
    seed_sqlite_store_id(store);
    // Легітимний offline-first: канал є, хаб недоступний (див. хелпер).
    seed_sqlite_delivery_channel();
    let pos: Arc<dyn torgashka_domain::PosService + Send + Sync> = Arc::new(OutboxPos::new(
        Arc::new(SqlxPos::new(StorePool::new(ro_pool.clone()))),
    ));
    let app = router_v1::build_router(standby_state(pos, ro_pool.clone()));
    let token = create_access_token(&user_id.to_string(), "admin", &[], SECRET).expect("JWT");

    // ── 3. Внесення 150.00 → 202 Accepted (НЕ 500 read-only) ────────────────
    let (status, op, raw) = call(
        &app,
        "POST",
        "/api/v1/cash-operations",
        &token,
        store,
        json!({
            "operation_type": "deposit",
            "cash_type": "cash",
            "amount": "150.00",
            "comment": "standby cash e2e"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "касова операція на standby має бути 202/queued, маємо {status}: {raw}"
    );
    assert_eq!(op["amount"], "150.00", "{op}");
    assert_eq!(op["operation_type"], "deposit", "{op}");
    assert_eq!(op["store_id"], store.to_string(), "{op}");
    assert_eq!(op["user_id"], user_id.to_string(), "{op}");
    let client_uuid = op["id"].as_str().expect("client_uuid операції").to_string();

    // ── 4. У PG — НУЛЬ рядків ───────────────────────────────────────────────
    assert_eq!(
        pg_cash_rows(&admin_pool, store).await,
        0,
        "на standby касова операція НЕ пише в репліку"
    );

    // ── 5. SQLite: агрегат cash_ledger + рівно 1 pending + баланс ───────────
    let (synced, data) = local_cash_ledger_row(&client_uuid).expect("агрегат cash_ledger");
    assert_eq!(synced, 1, "агрегат каси — push-кандидат");
    assert!(data.contains("deposit"), "data = payload як є: {data}");
    assert_eq!(
        outbox_rows(),
        vec![("cash_operation".to_string(), "pending".to_string(), 1)],
        "outbox: рівно один pending типу cash_operation"
    );
    assert_eq!(
        local_balance_cents(store, "cash"),
        15_000,
        "локальний баланс каси = +15000 коп. (ефект у тій самій транзакції)"
    );

    // ── 6. НЕГАТИВНИЙ КОНТРОЛЬ: та сама read-only БД + СТАРА обв'язка ──────
    let old_pos: Arc<dyn torgashka_domain::PosService + Send + Sync> =
        Arc::new(SqlxPos::new(StorePool::new(ro_pool.clone())));
    let old_app = router_v1::build_router(standby_state(old_pos, ro_pool.clone()));
    let (old_status, _, old_raw) = call(
        &old_app,
        "POST",
        "/api/v1/cash-operations",
        &token,
        store,
        json!({
            "operation_type": "deposit",
            "cash_type": "cash",
            "amount": "10.00"
        }),
    )
    .await;
    assert_eq!(
        old_status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "стара обв'язка на read-only репліці мусить дати 500, маємо {old_status}: {old_raw}"
    );
    for banned in [
        "read-only transaction",
        "INSERT INTO",
        "cash_operations",
        "sqlx",
    ] {
        assert!(
            !old_raw.contains(banned),
            "у тілі 500 немає сирого тексту PG/драйвера ('{banned}'): {old_raw}"
        );
    }
    eprintln!("[cash e2e] негативний контроль: {old_status} {old_raw}");
    assert_eq!(
        pg_cash_rows(&admin_pool, store).await,
        0,
        "негативний контроль нічого не записав у PG"
    );
    assert_eq!(
        outbox_rows(),
        vec![("cash_operation".to_string(), "pending".to_string(), 1)],
        "негативний контроль не додав outbox-записів"
    );
    assert_eq!(
        local_balance_cents(store, "cash"),
        15_000,
        "локальний баланс не змінився негативним контролем"
    );
    eprintln!(
        "[cash_operation_standby_e2e] ✅ ТЕСТ 1: 202 + 1 pending + баланс 15000 + 0 рядків PG"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ 2: приймач на primary — 1 рядок, повторний push → already_exists
// ─────────────────────────────────────────────────────────────────────────────

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
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
                let v: serde_json::Value = r.json().await.expect("login json");
                return v["access_token"].as_str().expect("token").to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("login не вдався");
}

#[tokio::test]
async fn cash_operation_push_idempotent_on_primary() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;

    // Каталог: точка + користувач-адміністратор з відомим паролем (для login).
    let store = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Cash Push Точка') ON CONFLICT (id) DO NOTHING")
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

    // Каса створила операцію ОФЛАЙН (сервер ще вимкнений) — окремий файл БД.
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let db_path = dir.path().join("cash-offline.db");
    let mut conn = open_connection(&db_path).expect("каса БД");
    let payload = json!({
        "store_id": store.to_string(),
        "user_id": admin_id.to_string(),
        "operation_type": "collection",
        "cash_type": "cash",
        "amount": "42.50",
        "comment": "офлайн-інкасація (e2e)",
    })
    .to_string();
    let enq = transactions::enqueue_cash_operation(&mut conn, &payload, &store.to_string())
        .expect("enqueue_cash_operation на касі");
    assert_eq!(pending_count(&conn).expect("pending"), 1, "1 оп в outbox");
    let balance = torgashka_infrastructure::offline::cash::get_cash_balance(
        &conn,
        &store.to_string(),
        "cash",
    )
    .expect("баланс каси");
    assert_eq!(balance, -4_250, "інкасація зменшує баланс локально");
    drop(conn);

    // Сервер піднято → push реальним клієнтом каси.
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = torgashka_api::run_facade(&addr);
    let token = login(&base).await;
    let client = reqwest::Client::new();
    let cfg = PushConfig {
        base_url: base.clone(),
        token: token.clone(),
        store_id: Some(store.to_string()),
        db_path: db_path.clone(),
        interval_secs: 30,
    };
    let s1 = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push");
    eprintln!("[cash e2e] перший push: {s1:?}");
    assert_eq!(s1.done, 1, "перший push → created");
    assert_eq!(s1.failed, 0, "помилок немає");

    let cu = Uuid::parse_str(&enq.client_uuid).expect("uuid");
    let row: (String, String, String, Uuid, Uuid) = sqlx::query_as(
        "SELECT operation_type, cash_type, amount::text, store_id, user_id \
         FROM cash_operations WHERE client_uuid = $1",
    )
    .bind(cu)
    .fetch_one(&pool)
    .await
    .expect("рядок cash_operations на primary");
    assert_eq!(row.0, "collection");
    assert_eq!(row.1, "cash");
    assert_eq!(row.2, "42.50", "сума з payload каси");
    assert_eq!(row.3, store);
    assert_eq!(row.4, admin_id, "user_id = JWT sub (касир push)");
    assert_eq!(
        pg_cash_rows(&pool, store).await,
        1,
        "рівно один рядок прийнято"
    );

    // Повторний push (done→pending) → already_exists, дубля немає.
    let conn = open_connection(&db_path).expect("БД");
    conn.execute(
        "UPDATE outbox SET status = 'pending', next_attempt_at = datetime('now') \
         WHERE status = 'done'",
        [],
    )
    .expect("reset done→pending");
    drop(conn);
    let s2 = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push 2");
    eprintln!("[cash e2e] повторний push: {s2:?}");
    assert_eq!(s2.already_exists, 1, "повторний push → already_exists");
    assert_eq!(s2.done, 0, "нового created немає");
    assert_eq!(s2.failed, 0, "помилок немає");
    assert_eq!(
        pg_cash_rows(&pool, store).await,
        1,
        "дублів касових операцій немає (partial UNIQUE 0017)"
    );
    eprintln!(
        "[cash_operation_standby_e2e] ✅ ТЕСТ 2: created → already_exists, 1 рядок, 0 дублів"
    );
}
