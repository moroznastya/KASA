//! E2E: міграція існуючих інсталяцій (Етап 6, ТЗ §9).
//!
//!   POST /api/v1/admin/migrate/legacy  (owner|admin)
//!
//! Сценарії (критерій прийняття Етапа 6):
//!   1. порожня мережа (немає точок і кас — «одиночна інсталяція») →
//!      POST створює першу точку + реєструє локальну касу як device
//!      (status='active', source='legacy_migration', БЕЗ коду активації)
//!      і пише audit_log запис action='legacy_migration';
//!   2. ідемпотентність: повторний POST НЕ дублює device (той самий id,
//!      count=1) і не пише зайвий audit-запис;
//!   3. наявна точка (setup виконаний) + немає кас → POST використовує
//!      існуючу точку (другу не створює) і реєструє каса;
//!   4. RBAC: cashier → 403; без JWT → 401.
//!
//! БД: TEST_DATABASE_URL або робочий URL + _test (tests/common/mod.rs).
//! Схема — ensure_schema (NETWORK_DDL ідемпотентно додає колонку devices.source
//! та partial UNIQUE на існуючих БД).

use std::time::Duration;

use serde_json::{json, Value};
use sqlx::Row;
use torgashka_api::run_facade;
use uuid::Uuid;

mod common;

static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn apply_schema() {
    SCHEMA_ONCE
        .get_or_init(|| async {
            let p = torgashka_infrastructure::db::connect_test_pool(5)
                .await
                .expect("тестова БД недоступна");
            torgashka_infrastructure::db::ensure_schema(&p)
                .await
                .expect("ensure_schema на тестовій БД");
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

async fn api_pool() -> sqlx::PgPool {
    let _ = torgashka_infrastructure::db::resolve_database_url()
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env");
    torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool")
}

/// bcrypt('admin123') — спільний seed-пароль e2e torgashka-api.
const PWD: &str = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e";

async fn seed_owner(pool: &sqlx::PgPool, tag: &str) -> String {
    let login = format!("migrate_e2e_owner_{tag}");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Migrate Owner', $2, $3, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed owner");
    login
}

async fn seed_cashier(pool: &sqlx::PgPool, tag: &str) -> String {
    let login = format!("migrate_e2e_cashier_{tag}");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Migrate Cashier', $2, $3, 'cashier'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed cashier");
    login
}

async fn login(base: &str, login_name: &str) -> String {
    let client = reqwest::Client::new();
    for _ in 0..60 {
        if let Ok(r) = client
            .post(format!("{base}/api/v1/auth/login"))
            .json(&json!({"login": login_name, "password": "admin123"}))
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
    panic!("login '{login_name}': сервер не піднявся");
}

async fn wait_ready(base: &str) {
    let client = reqwest::Client::new();
    for _ in 0..60 {
        if let Ok(r) = client.get(format!("{base}/api/v1/health")).send().await {
            if r.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("фасад на {base} не піднявся");
}

/// HTTP-хелпер: авторизований JSON-запит → (status, body).
async fn req(
    base: &str,
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut r = client.request(method, format!("{base}{path}"));
    if let Some(t) = token {
        r = r.bearer_auth(t);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let resp = r.send().await.expect("запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn db_count(pool: &sqlx::PgPool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("count")
}

// ─────────────────────────────────────────────────────────────────────────────
// Етап 6 §9: міграція legacy-інсталяції (пуста мережа → точка + активна каса)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn legacy_migration_creates_store_device_and_is_idempotent() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;

    // Чистий стан: ТІЛЬКИ дані, які створює сам тест (інші e2e-суіти
    // використовують власні унікальні логіни; цей тест потребує порожньої
    // мережі → TRUNCATE мережевих/owner-таблиць, як у admin_audit_prro_e2e).
    sqlx::query("TRUNCATE TABLE public.stores, public.users CASCADE")
        .execute(&pool)
        .await
        .expect("truncate");

    let tag = Uuid::new_v4().simple().to_string();
    let owner_login = seed_owner(&pool, &tag).await;
    let cashier_login = seed_cashier(&pool, &tag).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner = login(&base, &owner_login).await;

    // Вихідний стан: жодної каси й жодної точки — «одиночна інсталяція».
    let (s, devices) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/devices",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{devices}");
    assert!(devices.as_array().unwrap().is_empty(), "{devices}");
    assert_eq!(db_count(&pool, "stores").await, 0);

    // ── Крок: POST /admin/migrate/legacy → точка + активна каса без коду ──
    let (s, v) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/migrate/legacy",
        Some(&owner),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["created_store"], true, "{v}");
    assert_eq!(v["created_device"], true, "{v}");
    let store_id = v["store"]["id"].as_str().unwrap().to_string();
    let device_id = v["device"]["id"].as_str().unwrap().to_string();
    assert_eq!(v["store"]["name"], "Магазин 1", "{v}");
    assert_eq!(v["device"]["status"], "active", "{v}");
    assert_eq!(v["device"]["source"], "legacy_migration", "{v}");
    assert_eq!(v["device"]["store_id"], store_id, "{v}");
    assert_eq!(db_count(&pool, "stores").await, 1);
    assert_eq!(db_count(&pool, "devices").await, 1);
    let (src, st): (String, String) =
        sqlx::query_as("SELECT source, status::text FROM devices WHERE id = $1")
            .bind(Uuid::parse_str(&device_id).unwrap())
            .fetch_one(&pool)
            .await
            .expect("device row");
    assert_eq!(src, "legacy_migration", "маркер source у БД");
    assert_eq!(st, "active", "статус у БД");

    // Аудит: рівно 1 запис legacy_migration з іменем автора.
    let (s, log) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{log}");
    assert_eq!(log["total"], 1, "{log}");
    assert_eq!(log["items"][0]["action"], "legacy_migration", "{log}");
    assert_eq!(log["items"][0]["entity_type"], "device", "{log}");
    assert_eq!(log["items"][0]["actor_name"], "E2E Migrate Owner", "{log}");

    // ── Ідемпотентність: 2-й виклик не дублює device і не пише аудит ──
    let (s, v2) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/migrate/legacy",
        Some(&owner),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 200, "{v2}");
    assert_eq!(v2["created_store"], false, "{v2}");
    assert_eq!(v2["created_device"], false, "{v2}");
    assert_eq!(v2["device"]["id"], device_id, "{v2}");
    assert_eq!(db_count(&pool, "stores").await, 1);
    assert_eq!(db_count(&pool, "devices").await, 1, "device не задвоєно");
    let (s, log2) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{log2}");
    assert_eq!(log2["total"], 1, "повторний виклик не дублює аудит: {log2}");

    // ── Наявна точка (setup виконаний), кас немає → використовуємо її ──
    sqlx::query("DELETE FROM devices")
        .execute(&pool)
        .await
        .expect("delete devices");
    let (s, v3) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/migrate/legacy",
        Some(&owner),
        Some(json!({"store_name": "Ігнорується — точка вже є"})),
    )
    .await;
    assert_eq!(s, 200, "{v3}");
    assert_eq!(
        v3["created_store"], false,
        "наявна точка не дублюється: {v3}"
    );
    assert_eq!(v3["created_device"], true, "{v3}");
    assert_eq!(v3["store"]["id"], store_id, "{v3}");
    assert_eq!(db_count(&pool, "stores").await, 1);
    assert_eq!(db_count(&pool, "devices").await, 1);

    // ── RBAC: cashier → 403, без токена → 401 ──
    let cashier = login(&base, &cashier_login).await;
    let (s, vb) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/migrate/legacy",
        Some(&cashier),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 403, "{vb}");
    let (s, vb) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/migrate/legacy",
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 401, "{vb}");
}
