//! E2E: Аудит-лог + ПРРО централізовано (Етап 5, ТЗ 5.9 / 5.7).
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL:
//!   GET  /api/v1/admin/audit-log?from&to&actor&author&action&store_id&page&size
//!   GET  /api/v1/admin/stores/:store_id/prro-settings   (read-only, Етап 5)
//!
//! Сценарій (критерій прийняття Етапа 5):
//!   1. audit-log: owner виконує реальні адмін-дії (activation-code,
//!      update store, block device) → записи видно в GET з іменами авторів
//!      (LEFT JOIN users) і назвою точки; фільтри store_id / action /
//!      author / період працюють; пагінація коректна;
//!   2. RBAC: cashier → 403 на audit-log і на prro-settings;
//!   3. prro-settings: GET повертає read-only стан глобального реєстру
//!      (model: prro_settings/prro_shifts БЕЗ store_id — див. admin_prro.rs,
//!      аномалія задокументована). `editable:false`, ключ ЕЦП НЕ повертається
//!      у plaintext (немає поля з ключем/паролем взагалі — лише булеві
//!      ознаки); неіснуюча точка → 404.
//!
//! Гігієна: TRUNCATE на старті (як інші e2e адмін-етапів).

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

async fn seed_users(pool: &sqlx::PgPool, tag: &str) -> (String, String) {
    let owner_login = format!("audit_e2e_owner_{tag}");
    let cashier_login = format!("audit_e2e_cashier_{tag}");
    for (login, role) in [(&owner_login, "owner"), (&cashier_login, "cashier")] {
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
             VALUES ($1, 'E2E Audit Owner', $2, $3, $4::public.user_role, true, now(), now(), true)
             ON CONFLICT (login) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(login)
        .bind(PWD)
        .bind(role)
        .execute(pool)
        .await
        .expect("seed user");
    }
    (owner_login, cashier_login)
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

/// Авторизований HTTP-запит з JSON (GET — без body).
async fn req_json(
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
    let text = resp.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, parsed)
}

#[tokio::test]
async fn audit_log_filters_rbac_and_prro_read_only() {
    common::force_test_db();
    let pool = api_pool().await;

    // Гігієна: чиста БД (детерміновані глобальні фільтри аудиту).
    sqlx::raw_sql(
        "TRUNCATE devices, user_stores, stores, users, audit_log, \
         store_activation_codes, prro_settings, prro_shifts, prro_queue_items CASCADE",
    )
    .execute(&pool)
    .await
    .expect("truncate");

    let tag = format!("{}", Uuid::new_v4().simple().to_string()[..8].to_string());
    let (owner_login, cashier_login) = seed_users(&pool, &tag).await;

    // ─── Точки: А (активна) та Б (активна) ──────────────────────────────────
    let store_a = Uuid::new_v4();
    let store_b = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stores (id, name, is_active, created_at, updated_at)
         VALUES ($1,'Точка А',true,now(),now()), ($2,'Точка Б',true,now(),now())",
    )
    .bind(store_a)
    .bind(store_b)
    .execute(&pool)
    .await
    .expect("seed stores");

    // ─── Фасад ───────────────────────────────────────────────────────────────
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner = login(&base, &owner_login).await;
    let cashier = login(&base, &cashier_login).await;

    // ═══ 1. Реальні дії → audit_log ════════════════════════════════════════
    // 1a. activation-code точки А (audit: activation_code_generated, store).
    let (s, v) = req_json(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/stores/{store_a}/activation-code"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "activation-code: {v}");

    // 1b. update точки А (audit: store_updated).
    let (s, v) = req_json(
        &base,
        reqwest::Method::PUT,
        &format!("/api/v1/admin/stores/{store_a}"),
        Some(&owner),
        Some(json!({
            "name": "Точка А (оновлена)",
            "address": "вул. Центральна 1",
            "legal_name": "ТОВ Аудит",
            "edrpou": "12345678"
        })),
    )
    .await;
    assert_eq!(s, 200, "update store: {v}");

    // 1c. block device точки Б (audit: device_blocked, device) — потрібен device.
    let dev_b = Uuid::new_v4();
    let hash = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO devices (id, store_id, name, device_token_hash, status, app_version, activated_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'active', '1.0.0-test', now(), now(), now())",
    )
    .bind(dev_b)
    .bind(store_b)
    .bind("Каса Б")
    .bind(hash)
    .execute(&pool)
    .await
    .expect("seed device Б");
    let (s, v) = req_json(
        &base,
        reqwest::Method::POST,
        &format!("/api/v1/admin/devices/{dev_b}/block"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "block device: {v}");

    // ═══ 2. RBAC: cashier 403 на обох нових endpoints ═══════════════════════
    let (s, _) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log",
        Some(&cashier),
        None,
    )
    .await;
    assert_eq!(s, 403, "audit-log: cashier має отримати 403");
    let (s, _) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_a}/prro-settings"),
        Some(&cashier),
        None,
    )
    .await;
    assert_eq!(s, 403, "prro-settings: cashier має отримати 403");

    // ═══ 3. GET /admin/audit-log: записи + імена авторів ════════════════════
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "audit-log: {v}");
    assert_eq!(v["total"], 3, "3 дії: {v}");
    let items = v["items"].as_array().expect("items").clone();
    assert_eq!(items.len(), 3);
    // Сортування DESC (остання дія — device_blocked з точки Б).
    assert_eq!(items[0]["action"], "device_blocked", "{v}");
    // Автор — LEFT JOIN users (ім'я + логін), точка — stores.
    for it in &items {
        assert_eq!(it["actor_name"], "E2E Audit Owner", "автор: {v}");
        assert_eq!(it["actor_login"], json!(owner_login), "{v}");
        assert_eq!(it["actor_user_id"].is_null(), false);
    }
    let upd = items
        .iter()
        .find(|x| x["action"] == "store_updated")
        .expect("store_updated є");
    assert_eq!(upd["store_id"], json!(store_a.to_string()));
    assert_eq!(upd["store_name"], json!("Точка А (оновлена)"));
    assert_eq!(upd["entity_type"], "store", "{v}");
    assert!(upd["payload"]["name"] == "Точка А (оновлена)");
    let block = &items[0];
    assert_eq!(block["store_name"], json!("Точка Б"));

    // ═══ 4. Фільтри ═════════════════════════════════════════════════════════
    // store_id → лише записи точки Б (device_blocked).
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/audit-log?store_id={store_b}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 1, "лише device точки Б: {v}");
    assert_eq!(v["items"][0]["action"], "device_blocked", "{v}");

    // action (exact) → activation_code_generated точки А.
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?action=activation_code_generated",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 1, "{v}");
    assert_eq!(v["items"][0]["store_id"], json!(store_a.to_string()), "{v}");

    // author (підрядок по імені).
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?author=Audit%20Owner",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 3, "{v}");

    // actor (uuid) exact.
    let actor_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = $1")
        .bind(&owner_login)
        .fetch_one(&pool)
        .await
        .expect("owner id");
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/audit-log?actor={actor_id}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 3, "усі дії owner: {v}");

    // Період: «старіємо» activation-запис на 3 дні → from/to сьогодні його не бачить.
    sqlx::query("UPDATE audit_log SET created_at = now() - interval '3 days' WHERE action = $1")
        .bind("activation_code_generated")
        .execute(&pool)
        .await
        .expect("старіння audit-запису");
    let (today,): (String,) = sqlx::query_as("SELECT now()::date::text")
        .fetch_one(&pool)
        .await
        .expect("today");
    let (yesterday,): (String,) = sqlx::query_as("SELECT (now()::date - 1)::text")
        .fetch_one(&pool)
        .await
        .expect("yesterday");
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/audit-log?from={yesterday}&to={today}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 2, "старий запис відфільтровано періодом: {v}");

    // Пагінація: усього рядків 3 (старий НЕ видаляється — лише фільтрується
    // періодом). page=1&size=2 → 2 елементи, pages=2; page=2 → 1 елемент.
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?page=1&size=2",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 3, "усього 3 рядки (старий теж існує): {v}");
    assert_eq!(v["items"].as_array().unwrap().len(), 2, "{v}");
    assert_eq!(v["pages"], 2, "{v}");
    let (s, v2) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?page=2&size=2",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v2}");
    assert_eq!(v2["total"], 3, "{v2}");
    assert_eq!(
        v2["items"].as_array().unwrap().len(),
        1,
        "друга сторінка: {v2}"
    );
    assert_eq!(
        v2["items"][0]["action"], "activation_code_generated",
        "{v2}"
    );

    // Валідація: from>to та невірний uuid → 400.
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?from=2026-02-10&to=2026-02-01",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 400, "{v}");
    let (s, _) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?actor=not-a-uuid",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 400, "не uuid actor");

    // ═══ 5. prro-settings: read-only стан реєстру ═══════════════════════════
    // 5a. До налаштувань: configured=false, editable=false, ключів немає.
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_a}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "prro до налаштувань: {v}");
    assert_eq!(v["store_id"], json!(store_a.to_string()));
    assert_eq!(v["store_name"], "Точка А (оновлена)");
    assert_eq!(v["scope"], "global");
    assert_eq!(v["editable"], false);
    assert_eq!(v["configured"], false, "{v}");
    assert!(v["reason"].as_str().unwrap().contains("без store_id"));
    let raw = v.to_string();
    assert!(
        !raw.contains("key_password") && !raw.contains("key_file_content"),
        "жодного поля з ключем/паролем: {v}"
    );

    // 5b. Неіснуюча точка → 404.
    let ghost = Uuid::new_v4();
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{ghost}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 404, "{v}");

    // 5c. Після seed prro_settings + prro_shifts: стан реєстру видно,
    //     configured=true, КЕП-статус без секретів (файл не завантажено).
    sqlx::query(
        "INSERT INTO prro_settings (key_name, value, updated_at) VALUES
         ('prro_fn', '400000123456', now()), ('prro_tn', '12345678', now()),
         ('prro_zn', 'ZN900001', now()), ('mode', 'test', now()),
         ('url', 'api.test.prro.gov.ua', now())",
    )
    .execute(&pool)
    .await
    .expect("seed prro_settings");
    sqlx::query(
        "INSERT INTO prro_shifts (id, shift_number, opened_at, signer_serial, signer_name, status,
                                  receipt_count, total_amount, last_local_number)
         VALUES ($1, 7, now(), 'SERIAL-ABC-0001', 'ФОП Тест', 'open'::public.prro_shift_status,
                 12, 0::numeric, 0)",
    )
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("seed prro_shifts");

    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_a}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "prro після налаштувань: {v}");
    assert_eq!(v["configured"], true, "{v}");
    assert_eq!(v["settings"]["prro_fn"], "400000123456", "{v}");
    assert_eq!(v["settings"]["mode"], "test", "{v}");
    assert_eq!(v["settings"]["prro_tn"], "12345678", "{v}");
    assert_eq!(v["key"]["source"], "none", "{v}");
    assert_eq!(v["key"]["file_configured"], false, "{v}");
    assert_eq!(v["key"]["password_configured"], false, "{v}");
    assert_eq!(v["key"]["signer_serial"], "SERIAL-ABC-0001", "{v}");
    let shift = v["last_shift"].as_object().expect("last_shift є");
    assert_eq!(shift["shift_number"], 7);
    assert_eq!(shift["status"], "open");
    assert_eq!(shift["receipt_count"], 12);
    assert_eq!(v["settings_updated_at"].is_null(), false);
    // Ключ/пароль ЕЦП НІКОЛИ не повертаються: у відповіді немає їх значень.
    let raw = v.to_string();
    assert!(
        !raw.contains("SECRET"),
        "у GET немає секретних значень: {v}"
    );
    assert!(!raw.contains("key_file"), "{v}");

    pool.close().await;
}
