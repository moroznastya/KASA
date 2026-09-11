//! E2E: адмін-панель власника мережі (Етап 1, ТЗ 5.1–5.3).
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL:
//!   GET/POST /api/v1/admin/stores, GET/PUT/DELETE /api/v1/admin/stores/:id
//!   GET/POST /api/v1/admin/stores/:id/workers
//!   POST /api/v1/admin/users/:id/deactivate|activate|reset-password|reset-pin
//!
//! Сценарії (критерій прийняття Етапа 1):
//!   1. owner створює точку (POST /admin/stores з legal_name/edrpou) →
//!      редагує (PUT) → архівує (DELETE → is_active=false);
//!   2. архівація точки з активними касами: каси архівуються разом
//!      (status='deleted', рядки у БД лишаються), відповідь містить
//!      warning + archived_devices (поведінка визначена і зафіксована);
//!   3. деактивація працівника: рядок users залишається з is_active=false;
//!      повторна активація + reset password/pin;
//!   4. cashier → 403 на /admin/*; без JWT → 401;
//!   5. старі роути не зламані: GET /api/v1/stores (user-scope) працює.
//!
//! БД: TEST_DATABASE_URL або робочий URL + _test (tests/common/mod.rs);
//! схема — ensure_schema (додає колонки legal_name/edrpou та роль
//! store_manager ідемпотентно при старті фасаду).

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

/// Seed: owner (role owner) + cashier (role cashier), унікальні логіни.
async fn seed_users(pool: &sqlx::PgPool, tag: &str) -> (String, String) {
    let owner_login = format!("admin_e2e_owner_{tag}");
    let cashier_login = format!("admin_e2e_cashier_{tag}");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Admin Owner', $2, $3, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&owner_login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed owner");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Admin Cashier', $2, $3, 'cashier'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&cashier_login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed cashier");
    (owner_login, cashier_login)
}

/// Login → access_token (повторюємо, поки фасад не піднявся).
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

/// seed активного пристрою для точки (обхід активації — прямі рядки).
async fn seed_device(pool: &sqlx::PgPool, store: Uuid, tag: &str) -> Uuid {
    let id = Uuid::new_v4();
    // 64 hex = sha256-подібний формат device_token_hash (varchar, без FK).
    let hash = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO devices (id, store_id, name, device_token_hash, status, app_version, activated_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'active', '1.0.0-test', now(), now(), now())
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(store)
    .bind(format!("Каса {tag}"))
    .bind(hash)
    .execute(pool)
    .await
    .expect("seed device");
    id
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

// ─────────────────────────────────────────────────────────────────────────────
// 1. Повний життєвий цикл точки: create → list → get → update → archive
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn store_lifecycle_create_update_archive() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, _) = seed_users(&pool, &tag).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, &owner_login).await;

    // POST /admin/stores — створення з юрособою/ЄДРПОУ.
    let (cs, created) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        Some(&token),
        Some(json!({
            "name": "Магазин Етап1",
            "address": "м. Київ, вул. Тестова, 1",
            "phone": "+380 00 000 00 00",
            "legal_name": "ФОП Тестовий",
            "edrpou": "12345678",
        })),
    )
    .await;
    assert_eq!(cs, 201, "точку створено: {created}");
    let store_id = Uuid::parse_str(created["id"].as_str().expect("id")).expect("uuid");
    assert_eq!(created["name"], json!("Магазин Етап1"));
    assert_eq!(created["legal_name"], json!("ФОП Тестовий"));
    assert_eq!(created["edrpou"], json!("12345678"));
    assert_eq!(created["is_active"], json!(true));
    assert_eq!(created["devices_count"], json!(0));

    // GET /admin/stores — точка у списку.
    let (ls, list) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/stores",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(ls, 200, "список точок: {list}");
    let found = list
        .as_array()
        .expect("масив")
        .iter()
        .any(|s| s["id"] == json!(store_id.to_string()));
    assert!(found, "створена точка у списку адміна");

    // PUT /admin/stores/:id — редагування (зміна назви + юрособи).
    let (us, updated) = req(
        &base,
        reqwest::Method::PUT,
        &format!("/api/v1/admin/stores/{store_id}"),
        Some(&token),
        Some(json!({
            "name": "Магазин Етап1 (оновлено)",
            "address": "м. Київ, вул. Нова, 7",
            "legal_name": "ТОВ «Тестова Мережа»",
            "edrpou": "87654321",
        })),
    )
    .await;
    assert_eq!(us, 200, "точку оновлено: {updated}");
    assert_eq!(updated["name"], json!("Магазин Етап1 (оновлено)"));
    assert_eq!(updated["legal_name"], json!("ТОВ «Тестова Мережа»"));
    assert_eq!(updated["edrpou"], json!("87654321"));

    // GET /admin/stores/:id — деталі.
    let (gs, got) = req(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(gs, 200, "деталі точки: {got}");
    assert_eq!(got["is_active"], json!(true));

    // DELETE /admin/stores/:id — архівація (без кас).
    let (ds, archived) = req(
        &base,
        reqwest::Method::DELETE,
        &format!("/api/v1/admin/stores/{store_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(ds, 200, "архівація: {archived}");
    assert_eq!(
        archived["store"]["is_active"],
        json!(false),
        "архів = is_active=false"
    );
    assert_eq!(archived["archived_devices"], json!(0));

    // У БД рядок існує (м'яке видалення).
    let cnt: i64 = sqlx::query_scalar("SELECT count(*) FROM stores WHERE id = $1")
        .bind(store_id)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(cnt, 1, "точка залишається в БД після архівації");

    // PUT is_active=true — відновлення.
    let (rs, restored) = req(
        &base,
        reqwest::Method::PUT,
        &format!("/api/v1/admin/stores/{store_id}"),
        Some(&token),
        Some(json!({"name": "Магазин Етап1 (оновлено)", "is_active": true})),
    )
    .await;
    assert_eq!(rs, 200, "відновлення: {restored}");
    assert_eq!(restored["is_active"], json!(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Архівація точки з АКТИВНОЮ касою: касу архівовано разом (визначена
//    поведінка), рядки не видалені, у відповіді — warning + archived_devices.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn archive_store_cascades_devices_to_deleted() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, _) = seed_users(&pool, &tag).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, &owner_login).await;

    let (cs, created) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        Some(&token),
        Some(json!({"name": "Точка з касами"})),
    )
    .await;
    assert_eq!(cs, 201, "створено: {created}");
    let store_id = Uuid::parse_str(created["id"].as_str().expect("id")).expect("uuid");

    // Активна каса + архівована каса (для перевірки, що архівується лише active).
    let active_dev = seed_device(&pool, store_id, &format!("ACT-{tag}")).await;
    let deleted_dev = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO devices (id, store_id, name, device_token_hash, status, created_at, updated_at)
         VALUES ($1, $2, 'Каса стара', 'aabbccdd', 'deleted', now(), now())
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(deleted_dev)
    .bind(store_id)
    .execute(&pool)
    .await
    .expect("seed deleted device");

    // DELETE → архівація + каскад кас.
    let (ds, archived) = req(
        &base,
        reqwest::Method::DELETE,
        &format!("/api/v1/admin/stores/{store_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(ds, 200, "архівація з касами: {archived}");
    assert_eq!(archived["store"]["is_active"], json!(false));
    assert_eq!(
        archived["archived_devices"],
        json!(1),
        "архівовано лише активну касу"
    );
    let warning = archived["warning"].as_str().expect("warning");
    assert!(
        warning.contains("1 кас"),
        "warning про прив'язані каси: {warning}"
    );

    // Каса залишається в БД зі статусом deleted (фізичного видалення немає).
    let row = sqlx::query("SELECT status::text FROM devices WHERE id = $1")
        .bind(active_dev)
        .fetch_one(&pool)
        .await
        .expect("device у БД");
    let status: String = row.get("status");
    assert_eq!(status, "deleted", "активну касу архівовано разом із точкою");
    let cnt: i64 = sqlx::query_scalar("SELECT count(*) FROM devices WHERE store_id = $1")
        .bind(store_id)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(cnt, 2, "обидва рядки кас збережені в БД");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Працівники: create worker → deactivate (рядок лишається) → activate →
//    reset password → reset pin
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn workers_create_deactivate_keep_row_activate_reset() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, _) = seed_users(&pool, &tag).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, &owner_login).await;

    let (cs, created) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        Some(&token),
        Some(json!({"name": "Точка для працівників"})),
    )
    .await;
    assert_eq!(cs, 201, "створено: {created}");
    let store_id = Uuid::parse_str(created["id"].as_str().expect("id")).expect("uuid");

    // Створення працівника: глобальна роль store_manager + роль на точці admin.
    let login_name = format!("worker_{tag}");
    let (ws, worker) = req(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/stores/{store_id}/workers"),
        Some(&token),
        Some(json!({
            "name": "Керуюча Марія",
            "login": login_name,
            "password": "admin123",
            "pin_code": "4321",
            "role": "store_manager",
            "store_role": "admin",
        })),
    )
    .await;
    assert_eq!(ws, 201, "працівника створено: {worker}");
    assert_eq!(worker["role"], json!("store_manager"), "глобальна роль");
    assert_eq!(worker["store_role"], json!("admin"), "роль на точці");
    let worker_id = Uuid::parse_str(worker["id"].as_str().expect("id")).expect("uuid");

    // GET workers → працівник у списку точки.
    let (ls, list) = req(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_id}/workers"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(ls, 200, "працівники: {list}");
    let found = list
        .as_array()
        .expect("масив")
        .iter()
        .any(|w| w["id"] == json!(worker_id.to_string()));
    assert!(found, "працівник у списку точки");

    // Логін під новим працівником (store_manager) → працює.
    let worker_token = login(&base, &login_name).await;
    assert!(!worker_token.is_empty(), "store_manager може логінитись");

    // Деактивація (POST /admin/users/:id/deactivate) — БЕЗ фізичного видалення.
    let (ds, deactivated) = req(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/users/{worker_id}/deactivate"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(ds, 200, "деактивація: {deactivated}");
    assert_eq!(deactivated["is_active"], json!(false));

    // Рядок users ЗАЛИШАЄТЬСЯ в БД (критерій: deactivate не видаляє рядок).
    let cnt: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE id = $1")
        .bind(worker_id)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(cnt, 1, "рядок users існує після деактивації");

    // Деактивований не може увійти (403/401).
    let client = reqwest::Client::new();
    let r = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({"login": login_name, "password": "admin123"}))
        .send()
        .await
        .expect("login");
    assert!(
        r.status().is_client_error(),
        "деактивований працівник не логіниться: {}",
        r.status()
    );

    // Активація.
    let (as_, active) = req(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/users/{worker_id}/activate"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(as_, 200, "активація: {active}");
    assert_eq!(active["is_active"], json!(true));

    // Reset password → вхід під новим паролем.
    let (ps, _) = req(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/users/{worker_id}/reset-password"),
        Some(&token),
        Some(json!({"password": "newpass99"})),
    )
    .await;
    assert_eq!(ps, 200, "reset password");
    let r = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({"login": login_name, "password": "newpass99"}))
        .send()
        .await
        .expect("login new pwd");
    assert_eq!(r.status().as_u16(), 200, "вхід під новим паролем");

    // Reset pin.
    let (pins, _) = req(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/users/{worker_id}/reset-pin"),
        Some(&token),
        Some(json!({"pin_code": "1111"})),
    )
    .await;
    assert_eq!(pins, 200, "reset pin");
    let r = client
        .post(format!("{base}/api/v1/auth/login-pin"))
        .json(&json!({"login": login_name, "pin_code": "1111"}))
        .send()
        .await
        .expect("login pin");
    assert_eq!(r.status().as_u16(), 200, "вхід за новим PIN");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. RBAC: cashier → 403 на /admin/*; без JWT → 401; деактивація себе → 409
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn admin_rbac_cashier_forbidden_unauthorized() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, cashier_login) = seed_users(&pool, &tag).await;
    let owner_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = $1")
        .bind(&owner_login)
        .fetch_one(&pool)
        .await
        .expect("owner id");

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner_token = login(&base, &owner_login).await;
    let cashier_token = login(&base, &cashier_login).await;

    // Без JWT → 401.
    let (s1, b1) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/stores",
        None,
        None,
    )
    .await;
    assert_eq!(s1, 401, "без JWT: {b1}");

    // cashier → 403.
    let (s2, b2) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/stores",
        Some(&cashier_token),
        None,
    )
    .await;
    assert_eq!(s2, 403, "cashier на /admin/stores: {b2}");
    let (s3, b3) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        Some(&cashier_token),
        Some(json!({"name": "Заборонена точка"})),
    )
    .await;
    assert_eq!(s3, 403, "cashier на POST /admin/stores: {b3}");

    // Деактивація самого себе → 409.
    let (s4, b4) = req(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/users/{owner_id}/deactivate"),
        Some(&owner_token),
        None,
    )
    .await;
    assert_eq!(s4, 409, "самого себе: {b4}");

    // Старий user-scope роут /api/v1/stores НЕ зламаний (owner, без кас).
    let client = reqwest::Client::new();
    let r = client
        .get(format!("{base}/api/v1/stores"))
        .bearer_auth(&owner_token)
        .send()
        .await
        .expect("GET /stores");
    assert_eq!(r.status().as_u16(), 200, "старий GET /api/v1/stores працює");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. store_manager має доступ до адмін-роутів (роль додано в enum user_role)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn store_manager_can_use_admin_panel() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, _) = seed_users(&pool, &tag).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, &owner_login).await;

    let (cs, created) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        Some(&token),
        Some(json!({"name": "Точка менеджера"})),
    )
    .await;
    assert_eq!(cs, 201, "створено: {created}");
    let store_id = Uuid::parse_str(created["id"].as_str().expect("id")).expect("uuid");

    // Створюємо store_manager (роль у enum user_role вже є — ensure_schema).
    let sm_login = format!("sm_{tag}");
    let (ws, worker) = req(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/stores/{store_id}/workers"),
        Some(&token),
        Some(json!({
            "name": "Керуючий мережею",
            "login": sm_login,
            "password": "admin123",
            "role": "store_manager",
        })),
    )
    .await;
    assert_eq!(ws, 201, "store_manager створено: {worker}");

    // Логін + доступ до адмін-панелі.
    let sm_token = login(&base, &sm_login).await;
    let (s, list) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/stores",
        Some(&sm_token),
        None,
    )
    .await;
    assert_eq!(s, 200, "store_manager бачить точки: {list}");
}
