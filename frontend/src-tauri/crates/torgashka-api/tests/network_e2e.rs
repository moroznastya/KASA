//! E2E: мережевий рівень власника (Частина 3) — активація та керування касами.
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL:
//!   POST   /api/v1/devices/activate                     (ПУБЛІЧНИЙ)
//!   POST   /api/v1/admin/stores/:id/activation-code     (owner|admin)
//!   GET    /api/v1/admin/devices[?store_id=]            (owner|admin)
//!   POST   /api/v1/admin/devices/:id/block|/unblock     (owner|admin)
//!   DELETE /api/v1/admin/devices/:id (архівація)        (owner|admin)
//!
//! Сценарії:
//!   1. activate з валідним кодом → 200 + device_token (48 hex) ОДИН раз;
//!      у БД — лише SHA-256-хеш токена, status='active';
//!   2. activate з невалідним кодом → 404; після 5 невдалих спроб з одного
//!      IP (X-Forwarded-For) → 429 (rate limit);
//!   3. регенерація коду точки → новий code; старий код анульовано (404);
//!   4. admin-список/фільтр/block/unblock/архівація (status=deleted, рядок
//!      у БД зберігається);
//!   5. cashier → 403 на /admin/*; без JWT → 401;
//!   6. activate без обов'язкових полів → 400.
//!
//! БД: TEST_DATABASE_URL або робочий URL + _test (tests/common/mod.rs) —
//! мережеві таблиці створюються ensure_schema при старті фасаду (NETWORK_DDL).

use std::time::Duration;

use serde_json::{json, Value};
use sqlx::Row;
use torgashka_api::run_facade;
use uuid::Uuid;

mod common;

/// Застосувати схему БД (users/stores + owner-таблиці Частини 3) ОДИН раз на
/// процес. ensure_schema ідемпотентний: порожня БД отримує повну схему
/// (SCHEMA_SQL), мігрована — лише owner-DDL додатки. Без цього seed_store_and_users
/// падає на порожній тестовій БД («relation "stores" does not exist»), бо
/// ensure_schema виконується фасадом лише при старті (run_facade).
/// Один виклик на процес (OnceCell) — паралельні тести не конкурують за
/// CREATE TABLE на fresh-БД.
static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn apply_schema() {
    SCHEMA_ONCE
        .get_or_init(|| async {
            let p = torgashka_infrastructure::db::connect_test_pool(5)
                .await
                .expect("тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test");
            torgashka_infrastructure::db::ensure_schema(&p)
                .await
                .expect("ensure_schema на тестовій БД");
            p.close().await;
        })
        .await;
}

/// Унікальний XFF-ключ rate limit на кожен тест-сценарій (ізоляція).
const XFF_MAIN: &str = "203.0.113.10";
const XFF_INVALID: &str = "203.0.113.77";
const XFF_REGEN: &str = "203.0.113.20";
const XFF_REGEN2: &str = "203.0.113.21";
const XFF_BETA1: &str = "203.0.113.30";
const XFF_BETA2: &str = "203.0.113.31";

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

/// Seed: точка + власник (role owner) + касир (role cashier).
async fn seed_store_and_users(pool: &sqlx::PgPool) -> Uuid {
    let store = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Network Точка') ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .execute(pool)
        .await
        .expect("seed store");

    // bcrypt('admin123') — спільний seed-пароль усіх e2e torgashka-api.
    let pwd = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e";
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'Network Owner', 'network_owner', $2, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(pwd)
    .execute(pool)
    .await
    .expect("seed owner");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'Network Cashier', 'network_cashier', $2, 'cashier'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(pwd)
    .execute(pool)
    .await
    .expect("seed cashier");
    // Прив'язка до точки (логін-контур user_stores).
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, s.id, u.role::text, '{}'::jsonb, true, now()
         FROM users u, stores s
         WHERE u.login IN ('network_owner','network_cashier') AND s.id = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(store)
    .execute(pool)
    .await
    .expect("seed user_stores");
    store
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
                return v["access_token"].as_str().expect("access_token").to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("login '{login_name}': сервер не піднявся");
}

/// Чекаємо, поки фасад піднявся (ensure_schema виконано до serve).
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

/// POST /api/v1/devices/activate (публічний) → (status, тіло).
async fn activate(base: &str, code: &str, fingerprint: &str, xff: &str) -> (u16, Value) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/devices/activate"))
        .header("x-forwarded-for", xff)
        .json(&json!({"code": code, "device_fingerprint": fingerprint}))
        .send()
        .await
        .expect("activate запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// POST /api/v1/admin/stores/:id/activation-code → (status, code|detail).
async fn gen_code(base: &str, token: &str, store: Uuid) -> (u16, Value) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/admin/stores/{store}/activation-code"))
        .bearer_auth(token)
        .send()
        .await
        .expect("activation-code запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(s.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Валідна активація: токен один раз, у БД — лише SHA-256-хеш
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn activate_valid_code_returns_token_and_persists_hash() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let store = seed_store_and_users(&pool).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, "network_owner").await;

    // Код точки (admin) → активація каси (публічна, без JWT).
    let (cs, code_body) = gen_code(&base, &token, store).await;
    assert_eq!(cs, 200, "код точки створено: {code_body}");
    let code = code_body["code"].as_str().expect("code").to_string();
    assert_eq!(code.len(), 8);

    let (as_, act) = activate(&base, &code, "KASA-ALPHA-7F3A9C", XFF_MAIN).await;
    assert_eq!(as_, 200, "активація валідним кодом: {act}");

    let device_token = act["device_token"].as_str().expect("device_token").to_string();
    assert_eq!(device_token.len(), 48, "токен — 48 hex");
    assert!(
        device_token.chars().all(|c| c.is_ascii_hexdigit()),
        "токен hex: {device_token}"
    );
    let device_id = Uuid::parse_str(act["device_id"].as_str().expect("device_id"))
        .expect("device_id uuid");
    let store_id = Uuid::parse_str(act["store_id"].as_str().expect("store_id"))
        .expect("store_id uuid");
    assert_eq!(store_id, store, "каса прив'язана до точки коду");
    assert_eq!(act["store_name"], json!("E2E Network Точка"));

    // У БД: device_token_hash == sha256(токена), НЕ сам токен; status active.
    let row = sqlx::query(
        "SELECT device_token_hash, status::text, name, activated_at \
         FROM devices WHERE id = $1",
    )
    .bind(device_id)
    .fetch_one(&pool)
    .await
    .expect("device у БД");
    let hash: String = row.get("device_token_hash");
    let status: String = row.get("status");
    let name: String = row.get("name");
    let activated_at: Option<chrono::NaiveDateTime> = row.get("activated_at");
    assert_eq!(hash, sha256_hex(&device_token), "зберігається лише SHA-256");
    assert_ne!(hash, device_token, "оригінал токена не зберігається");
    assert_eq!(status, "active");
    assert!(name.starts_with("Каса "), "name каси: {name}");
    assert!(activated_at.is_some(), "activated_at заповнено");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Невалідний код → 404; 5 невдалих спроб з одного IP → 429
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn activate_invalid_code_404_then_rate_limit_429() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let store = seed_store_and_users(&pool).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let _token = login(&base, "network_owner").await;
    let _ = store;

    // Код "00000000": символи поза алфавітом генерації (без 0) →
    // гарантовано не може існувати в store_activation_codes.
    for i in 0..5 {
        let (st, body) = activate(&base, "00000000", "KASA-BRUTE", XFF_INVALID).await;
        assert_eq!(st, 404, "спроба {}: {body}", i + 1);
    }
    let (st, body) = activate(&base, "00000000", "KASA-BRUTE", XFF_INVALID).await;
    assert_eq!(st, 429, "6-та спроба → rate limit: {body}");

    // Інший IP (інший ключ) не заблокований.
    let (st, _) = activate(&base, "00000000", "KASA-BRUTE2", "203.0.113.99").await;
    assert_eq!(st, 404, "інший IP не під rate limit");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Регенерація коду: новий code, старий анульовано
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn regenerate_activation_code_changes_code() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let store = seed_store_and_users(&pool).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, "network_owner").await;

    let (_, c1) = gen_code(&base, &token, store).await;
    let code1 = c1["code"].as_str().expect("code1").to_string();

    // Перша активація старим кодом — успішна.
    let (st, _) = activate(&base, &code1, "KASA-REGEN-01", XFF_REGEN).await;
    assert_eq!(st, 200);

    // Регенерація → інший код; regenerated_at у БД заповнено.
    let (_, c2) = gen_code(&base, &token, store).await;
    let code2 = c2["code"].as_str().expect("code2").to_string();
    assert_ne!(code1, code2, "регенерація дає новий код");
    let regen: Option<chrono::NaiveDateTime> =
        sqlx::query_scalar("SELECT regenerated_at FROM store_activation_codes WHERE store_id = $1")
            .bind(store)
            .fetch_one(&pool)
            .await
            .expect("regenerated_at");
    assert!(regen.is_some(), "regenerated_at заповнено");

    // Старий код анульовано (404), новий — працює.
    let (st, body) = activate(&base, &code1, "KASA-REGEN-02", XFF_REGEN2).await;
    assert_eq!(st, 404, "старий код після регенерації: {body}");
    let (st, _) = activate(&base, &code2, "KASA-REGEN-03", XFF_REGEN2).await;
    assert_eq!(st, 200, "новий код активує касу");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Admin: список + фільтр + block/unblock + архівація (status=deleted)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn admin_devices_list_block_unblock_archive() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let store = seed_store_and_users(&pool).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, "network_owner").await;

    let (_, code_body) = gen_code(&base, &token, store).await;
    let code = code_body["code"].as_str().expect("code").to_string();

    // Дві каси на точку.
    let (s1, a1) = activate(&base, &code, "KASA-BETA-01", XFF_BETA1).await;
    assert_eq!(s1, 200);
    let (s2, a2) = activate(&base, &code, "KASA-BETA-02", XFF_BETA2).await;
    assert_eq!(s2, 200);
    let dev1 = a1["device_id"].as_str().expect("device_id 1").to_string();
    let dev2 = a2["device_id"].as_str().expect("device_id 2").to_string();

    let client = reqwest::Client::new();

    // Список без фільтра: знаходимо обидві каси, store_name заповнено.
    let resp = client
        .get(format!("{base}/api/v1/admin/devices"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list");
    assert_eq!(resp.status().as_u16(), 200);
    let all: Value = resp.json().await.expect("list json");
    let mine: Vec<&Value> = all
        .as_array()
        .expect("array")
        .iter()
        .filter(|d| d["store_id"].as_str() == Some(store.to_string().as_str()))
        .collect();
    assert_eq!(mine.len(), 2, "2 каси цієї точки у загальному списку");
    for d in &mine {
        assert_eq!(d["store_name"], json!("E2E Network Точка"), "store_name додано");
        assert_eq!(d["status"], json!("active"));
    }

    // Фільтр ?store_id=.
    let resp = client
        .get(format!("{base}/api/v1/admin/devices?store_id={store}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list filtered");
    assert_eq!(resp.status().as_u16(), 200);
    let filtered: Value = resp.json().await.expect("filtered json");
    assert_eq!(filtered.as_array().expect("array").len(), 2);

    // block → статус змінився у списку.
    let resp = client
        .post(format!("{base}/api/v1/admin/devices/{dev1}/block"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("block");
    assert_eq!(resp.status().as_u16(), 200, "block: {}", resp.text().await.unwrap_or_default());
    let resp = client
        .get(format!("{base}/api/v1/admin/devices?store_id={store}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list after block");
    let after_block: Value = resp.json().await.expect("json");
    let d1 = after_block
        .as_array()
        .expect("array")
        .iter()
        .find(|d| d["id"].as_str() == Some(dev1.as_str()))
        .expect("dev1 у списку");
    assert_eq!(d1["status"], json!("blocked"));

    // unblock → active.
    let resp = client
        .post(format!("{base}/api/v1/admin/devices/{dev1}/unblock"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("unblock");
    assert_eq!(resp.status().as_u16(), 200);
    let ub: Value = resp.json().await.expect("unblock json");
    assert_eq!(ub["status"], json!("active"));

    // Архівація (DELETE — не фізичне видалення): status='deleted', рядок є.
    let resp = client
        .delete(format!("{base}/api/v1/admin/devices/{dev2}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("delete");
    assert_eq!(resp.status().as_u16(), 200, "delete: {}", resp.text().await.unwrap_or_default());
    let deleted: Value = resp.json().await.expect("delete json");
    assert_eq!(deleted["status"], json!("deleted"));

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM devices WHERE id = $1")
            .bind(Uuid::parse_str(&dev2).unwrap())
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(count, 1, "архівація НЕ видаляє рядок");

    // block архівованого → 409.
    let resp = client
        .post(format!("{base}/api/v1/admin/devices/{dev2}/block"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("block archived");
    assert_eq!(resp.status().as_u16(), 409, "архівований пристрій не керується");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. RBAC: cashier → 403; без JWT → 401
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn admin_endpoints_require_owner_or_admin() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let store = seed_store_and_users(&pool).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;

    // Без JWT — 401 (auth_middleware на /admin/*).
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/api/v1/admin/devices"))
        .send()
        .await
        .expect("no-token list");
    assert_eq!(resp.status().as_u16(), 401);

    // Касир (роль cashier) — 403 на всі /admin/* мережі.
    let cashier_token = login(&base, "network_cashier").await;
    let resp = client
        .get(format!("{base}/api/v1/admin/devices"))
        .bearer_auth(&cashier_token)
        .send()
        .await
        .expect("cashier list");
    assert_eq!(resp.status().as_u16(), 403, "cashier → 403");

    let resp = client
        .post(format!("{base}/api/v1/admin/stores/{store}/activation-code"))
        .bearer_auth(&cashier_token)
        .send()
        .await
        .expect("cashier code");
    assert_eq!(resp.status().as_u16(), 403, "cashier не генерує код");
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Активація з неповним тілом → 400
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn activate_missing_fields_400() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let _store = seed_store_and_users(&pool).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;

    let client = reqwest::Client::new();
    // Без device_fingerprint.
    let resp = client
        .post(format!("{base}/api/v1/devices/activate"))
        .header("x-forwarded-for", "203.0.113.55")
        .json(&json!({"code": "ABCD2345"}))
        .send()
        .await
        .expect("missing fp");
    assert_eq!(resp.status().as_u16(), 422, "відсутній device_fingerprint → 422 (FastAPI-формат)");

    // Порожній код → 400 (наша валідація, не rate limit).
    let resp = client
        .post(format!("{base}/api/v1/devices/activate"))
        .header("x-forwarded-for", "203.0.113.56")
        .json(&json!({"code": "   ", "device_fingerprint": "KASA-X"}))
        .send()
        .await
        .expect("empty code");
    assert_eq!(resp.status().as_u16(), 400, "порожній code → 400");
}
