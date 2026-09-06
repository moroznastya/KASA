//! E2E: фізичне видалення «порожньої» точки (Етап 2-backend, DELETE store).
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL:
//!   POST /api/v1/admin/stores/:store_id/delete  → ФІЗИЧНЕ видалення ЛИШЕ
//!        порожньої точки (owner-only).
//!
//! ВАЖЛИВО про маршрут: `DELETE /api/v1/admin/stores/:id` зайнятий АРХІВАЦІЄЮ
//! (is_active=false, дані зберігаються) — це зафіксований публічний контракт
//! UI (frontend/src/services/adminService.ts archiveStore). Фізичне видалення
//! — окремий POST-екшен у стилі решти станів (activation-code, block/unblock):
//!   POST /api/v1/admin/stores/:store_id/delete
//!
//! Сценарії (критерій прийняття):
//!   1. owner створює чисту точку → delete → 204, точки більше немає
//!      (ні в /api/v1/stores user-scope, ні в /api/v1/admin/stores);
//!   2. точка з даними: stock → 409 «є дані у stock»; devices → 409 «devices»;
//!   3. admin / cashier → 403 (owner-only);
//!   4. неіснуючий store_id (валідний uuid) → 404;
//!   5. невалідний uuid у path → 400;
//!   6. аудит: з'явився запис action="store_deleted".
//!
//! БД: TEST_DATABASE_URL або робочий URL + _test (tests/common/mod.rs).

use std::time::Duration;

use serde_json::{json, Value};
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

/// Seed: owner (role owner) + admin (role admin) + cashier (role cashier).
async fn seed_users(pool: &sqlx::PgPool, tag: &str) -> (String, String, String) {
    let owner_login = format!("sdel_owner_{tag}");
    let admin_login = format!("sdel_admin_{tag}");
    let cashier_login = format!("sdel_cashier_{tag}");
    for (login, role) in [
        (owner_login.clone(), "owner"),
        (admin_login.clone(), "admin"),
        (cashier_login.clone(), "cashier"),
    ] {
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
             VALUES ($1, 'E2E StoreDel', $2, $3, $4::public.user_role, true, now(), now(), true)
             ON CONFLICT (login) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(&login)
        .bind(PWD)
        .bind(role)
        .execute(pool)
        .await
        .expect("seed user");
    }
    (owner_login, admin_login, cashier_login)
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

/// Створити точку (owner) → (status, body).
async fn create_store(base: &str, token: &str, name: &str) -> (u16, Value) {
    req(
        base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        Some(token),
        Some(json!({ "name": name, "address": "м. Київ, вул. Делет, 1" })),
    )
    .await
}

/// POST /admin/stores/:id/delete → (status, body).
async fn delete_store(base: &str, token: &str, store_id: &str) -> (u16, Value) {
    req(
        base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/stores/{store_id}/delete"),
        Some(token),
        None,
    )
    .await
}

/// Прив'язати рядок stock (потрібен валідний product через FK).
async fn seed_stock(pool: &sqlx::PgPool, store: Uuid) {
    let product_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, title, price, created_at, updated_at)
         VALUES ($1, 'E2E Product Delete', 10.00, now(), now())",
    )
    .bind(product_id)
    .execute(pool)
    .await
    .expect("seed product");
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, price, updated_at)
         VALUES ($1, $2, 5, 10.00, now())",
    )
    .bind(store)
    .bind(product_id)
    .execute(pool)
    .await
    .expect("seed stock");
}

/// Прив'язати пристрій (device) до точки.
async fn seed_device(pool: &sqlx::PgPool, store: Uuid, tag: &str) {
    let id = Uuid::new_v4();
    let hash = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO devices (id, store_id, name, device_token_hash, status, app_version, activated_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'active', '1.0.0-test', now(), now(), now())",
    )
    .bind(id)
    .bind(store)
    .bind(format!("Каса {tag}"))
    .bind(hash)
    .execute(pool)
    .await
    .expect("seed device");
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

/// Повний сценарій: чиста точка → 204; з даними → 409; RBAC; 404; 400.
#[tokio::test]
async fn store_delete_empty_lifecycle() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, admin_login, cashier_login) = seed_users(&pool, &tag).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner_token = login(&base, &owner_login).await;
    let admin_token = login(&base, &admin_login).await;
    let cashier_token = login(&base, &cashier_login).await;

    // ── 1. Чиста точка → 204; зникає зі списків /api/v1/stores та /admin/stores
    let (cs, created) = create_store(&base, &owner_token, "Чиста точка").await;
    assert_eq!(cs, 201, "створення чистої точки: {created}");
    let store_id = created["id"].as_str().expect("id").to_string();
    let store_uuid = Uuid::parse_str(&store_id).expect("uuid");

    let (ds, body) = delete_store(&base, &owner_token, &store_id).await;
    assert_eq!(ds, 204, "чисту точку видалено: {body}");

    // user-scope /api/v1/stores більше не містить точку.
    let (us, user_stores) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/stores",
        Some(&owner_token),
        None,
    )
    .await;
    assert_eq!(us, 200, "список user-точок: {user_stores}");
    let absent = user_stores
        .as_array()
        .expect("масив")
        .iter()
        .all(|s| s["id"] != json!(store_id));
    assert!(absent, "видалена точка не у /api/v1/stores: {user_stores}");

    // admin-список теж не містить точку.
    let (as_, admin_stores) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/stores",
        Some(&owner_token),
        None,
    )
    .await;
    assert_eq!(as_, 200, "admin-список: {admin_stores}");
    assert!(
        admin_stores
            .as_array()
            .expect("масив")
            .iter()
            .all(|s| s["id"] != json!(store_id)),
        "видалена точка не у /admin/stores: {admin_stores}"
    );

    // Рядок у БД зник (разом із каскадною user_stores-прив'язкою власника).
    let row_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stores WHERE id = $1)")
        .bind(store_uuid)
        .fetch_one(&pool)
        .await
        .expect("exists");
    assert!(!row_exists, "рядок stores фізично видалено");
    let owner_binding: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_stores WHERE store_id = $1)")
            .bind(store_uuid)
            .fetch_one(&pool)
            .await
            .expect("exists");
    assert!(!owner_binding, "каскадна прив'язка user_stores прибрана");

    // ── 2a. Точка зі stock → 409 «stock»
    let (cs2, with_stock) = create_store(&base, &owner_token, "Точка зі складом").await;
    assert_eq!(cs2, 201, "створення: {with_stock}");
    let stock_store_id = with_stock["id"].as_str().expect("id").to_string();
    let stock_store_uuid = Uuid::parse_str(&stock_store_id).expect("uuid");
    seed_stock(&pool, stock_store_uuid).await;

    let (ds2, conflict) = delete_store(&base, &owner_token, &stock_store_id).await;
    assert_eq!(ds2, 409, "stock блокує видалення: {conflict}");
    let detail = conflict["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("stock"),
        "повідомлення згадує таблицю stock: {detail}"
    );
    let still_there: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stores WHERE id = $1)")
            .bind(stock_store_uuid)
            .fetch_one(&pool)
            .await
            .expect("exists");
    assert!(still_there, "точка з даними не видалена (409)");

    // ── 2b. Точка з devices → 409 «devices»
    let (cs3, with_dev) = create_store(&base, &owner_token, "Точка з касою").await;
    assert_eq!(cs3, 201, "створення: {with_dev}");
    let dev_store_id = with_dev["id"].as_str().expect("id").to_string();
    let dev_store_uuid = Uuid::parse_str(&dev_store_id).expect("uuid");
    seed_device(&pool, dev_store_uuid, &tag).await;

    let (ds3, conflict3) = delete_store(&base, &owner_token, &dev_store_id).await;
    assert_eq!(ds3, 409, "devices блокує видалення: {conflict3}");
    let detail3 = conflict3["detail"].as_str().unwrap_or_default();
    assert!(
        detail3.contains("devices"),
        "повідомлення згадує таблицю devices: {detail3}"
    );

    // ── 3. RBAC: admin та cashier → 403 (owner-only), точка не видалена
    let (cs4, rbac_store) = create_store(&base, &owner_token, "Точка RBAC").await;
    assert_eq!(cs4, 201, "створення: {rbac_store}");
    let rbac_id = rbac_store["id"].as_str().expect("id").to_string();
    let rbac_uuid = Uuid::parse_str(&rbac_id).expect("uuid");

    let (da, body_a) = delete_store(&base, &admin_token, &rbac_id).await;
    assert_eq!(da, 403, "admin → 403: {body_a}");
    let (dc, body_c) = delete_store(&base, &cashier_token, &rbac_id).await;
    assert_eq!(dc, 403, "cashier → 403: {body_c}");
    let still_rbac: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stores WHERE id = $1)")
        .bind(rbac_uuid)
        .fetch_one(&pool)
        .await
        .expect("exists");
    assert!(still_rbac, "точка збережена після 403");

    // ── 4. Неіснуючий (але валідний uuid) → 404
    let (dn, body_n) = delete_store(&base, &owner_token, &Uuid::new_v4().to_string()).await;
    assert_eq!(dn, 404, "неіснуюча точка → 404: {body_n}");

    // ── 5. Невалідний uuid → 400
    let (di, body_i) = delete_store(&base, &owner_token, "not-a-uuid").await;
    assert_eq!(di, 400, "невалідний uuid → 400: {body_i}");

    // ── 6. Аудит: запис store_deleted існує (для першої видаленої точки)
    let audit_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'store_deleted'
         AND entity_id = $1 AND store_id IS NULL",
    )
    .bind(store_uuid)
    .fetch_one(&pool)
    .await
    .expect("audit count");
    assert_eq!(audit_rows, 1, "аудит store_deleted записано");
}
