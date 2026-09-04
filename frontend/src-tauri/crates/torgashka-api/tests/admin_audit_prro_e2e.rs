//! E2E: Аудит-лог + ПРРО per-store (Етап 5, ТЗ 5.9 / 5.7; «один магазин —
//! один ПРРО» — закриття аномалії глобального реєстру).
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL:
//!   GET/PUT /api/v1/admin/stores/:store_id/prro-settings  (owner|admin)
//!   GET  /api/v1/admin/audit-log
//!   GET  /api/v2/prro/settings  (каса точки: X-Store-Id, без require_admin)
//!
//! Сценарій (критерій прийняття):
//!   1. audit-log: owner виконує реальні адмін-дії → записи + імена авторів,
//!      фільтри/пагінація (без змін відносно Етапа 5);
//!   2. RBAC: cashier → 403 на audit-log і на prro-settings (GET і PUT);
//!   3. ПРРО per-store:
//!      - PUT налаштувань точки А зберігає конфіг А (FN/ТН/ЗН/mode/url);
//!      - GET А бачить А; GET Б НЕ бачить дані А (configured=false);
//!      - PUT Б не затирає А (ключі (store_id, key_name));
//!      - каса А через /api/v2/prro/settings (X-Store-Id: А) бачить конфіг А,
//!        каса Б — конфіг Б;
//!      - PUT завантажує ключ КЕП per-store, GET НЕ повертає plaintext;
//!      - неіснуюча точка → 404;
//!   4. Міграція глобальних рядків → перший магазин покривається окремим
//!      інфра-тестом (crates/torgashka-infrastructure/tests/prro_repository.rs).
//!
//! Гігієна: TRUNCATE на старті (як інші e2e адмін-етапів).

use std::time::Duration;

use serde_json::{json, Value};
use torgashka_api::run_facade;
use uuid::Uuid;

mod common;

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

/// Запит каси з X-Store-Id (JSON-відповідь).
async fn req_json_store(
    base: &str,
    path: &str,
    token: &str,
    store_id: &str,
) -> (u16, Value) {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .header("X-Store-Id", store_id)
        .send()
        .await
        .expect("запит каси");
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, parsed)
}

/// Multipart PUT налаштувань ПРРО точки (поля + опційний файл ключа).
/// Мультипарт збирається вручну: dev-deps reqwest без feature "multipart".
async fn prro_put(
    base: &str,
    store_id: &Uuid,
    token: &str,
    fields: &[(&str, &str)],
    key_file: Option<(&str, &[u8])>,
) -> (u16, Value) {
    let boundary = format!("----torgashka-e2e-{}", Uuid::new_v4().simple());
    let mut body: Vec<u8> = Vec::new();
    for (k, v) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                .as_bytes(),
        );
    }
    if let Some((name, content)) = key_file {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"key_file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let resp = reqwest::Client::new()
        .put(format!("{base}/api/v1/admin/stores/{store_id}/prro-settings"))
        .bearer_auth(token)
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .body(body)
        .send()
        .await
        .expect("PUT prro");
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, parsed)
}

#[tokio::test]
async fn audit_log_filters_rbac_and_prro_per_store() {
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

    // Доступ власника до обох точок (каса-ендпоінти /api/v2/prro/* під
    // store_middleware перевіряють user_stores).
    let owner_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = $1")
        .bind(&owner_login)
        .fetch_one(&pool)
        .await
        .expect("owner id");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id) VALUES ($1, $2), ($1, $3)",
    )
    .bind(owner_id)
    .bind(store_a)
    .bind(store_b)
    .execute(&pool)
    .await
    .expect("owner user_stores");

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

    // ═══ 2. RBAC: cashier 403 на обох endpoints ═════════════════════════════
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
    assert_eq!(s, 403, "prro-settings GET: cashier має отримати 403");
    // PUT теж адмін-роут: cashier → 403 (мультипарт-форма коректна).
    let (s, v) = prro_put(
        &base,
        &store_a,
        &cashier,
        &[("prro_fn", "400000123456")],
        None,
    )
    .await;
    assert_eq!(s, 403, "prro-settings PUT: cashier 403: {v}");

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

    // ═══ 4. Фільтри/пагінація (без змін) ════════════════════════════════════
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
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?from=2026-02-10&to=2026-02-01",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 400, "{v}");

    // ═══ 5. ПРРО per-store: PUT А → GET А/Б, PUT Б, каса-видимість ═════════
    // 5a. До налаштувань: configured=false, editable=true, scope="store".
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
    assert_eq!(v["scope"], "store");
    assert_eq!(v["editable"], true);
    assert_eq!(v["configured"], false, "{v}");
    assert_eq!(v["reason"], Value::Null, "аномалію закрито: reason має бути null");

    // 5b. PUT налаштувань ТОЧКИ А (з валідацією per-store).
    let (s, v) = prro_put(
        &base,
        &store_a,
        &owner,
        &[
            ("prro_fn", "400000123456"),
            ("prro_tn", "1234567890"),
            ("prro_zn", "ZN900001"),
            ("mode", "test"),
            ("url", "api.test.prro.gov.ua"),
        ],
        None,
    )
    .await;
    assert_eq!(s, 200, "PUT prro A: {v}");
    assert_eq!(v["configured"], true, "{v}");
    assert_eq!(v["settings"]["prro_fn"], "400000123456", "{v}");
    assert_eq!(v["settings"]["mode"], "test", "{v}");
    assert_eq!(v["settings"]["url"], "api.test.prro.gov.ua", "{v}");

    // 5c. GET Б НЕ бачить конфіг А (глобального запису більше немає).
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_b}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "GET Б: {v}");
    assert_eq!(v["configured"], false, "Б не має бачити конфіг А: {v}");
    assert_eq!(v["settings"]["prro_fn"], "", "{v}");
    assert_eq!(v["settings"]["url"], "", "{v}");

    // 5d. PUT Б не затирає А; Б має свій конфіг.
    let (s, v) = prro_put(
        &base,
        &store_b,
        &owner,
        &[
            ("prro_fn", "400000222222"),
            ("prro_tn", "0987654321"),
            ("prro_zn", "ZN900002"),
            ("mode", "test"),
            ("url", "api.test.prro.other"),
        ],
        None,
    )
    .await;
    assert_eq!(s, 200, "PUT prro B: {v}");
    assert_eq!(v["settings"]["prro_fn"], "400000222222", "{v}");
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_a}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "GET A після PUT B: {v}");
    assert_eq!(
        v["settings"]["prro_fn"], "400000123456",
        "PUT Б не має затерти конфіг А: {v}"
    );
    assert_eq!(
        v["settings"]["url"], "api.test.prro.gov.ua",
        "конфіг А збережено: {v}"
    );

    // 5e. Каса бачить оновлений конфіг через /api/v2/prro/settings
    //     (X-Store-Id) — критерій 4 приймання.
    let (s, v) = req_json_store(&base, "/api/v2/prro/settings", &owner, &store_a.to_string())
        .await;
    assert_eq!(s, 200, "каса А settings: {v}");
    assert_eq!(v["prro_fn"], "400000123456", "каса А бачить конфіг А: {v}");
    assert_eq!(v["mode"], "test", "{v}");
    let (s, v) = req_json_store(&base, "/api/v2/prro/settings", &owner, &store_b.to_string())
        .await;
    assert_eq!(s, 200, "каса Б settings: {v}");
    assert_eq!(v["prro_fn"], "400000222222", "каса Б бачить конфіг Б: {v}");

    // 5f. Ключ КЕП: PUT завантажує per-store; GET не повертає plaintext.
    let (s, v) = prro_put(
        &base,
        &store_a,
        &owner,
        &[("key_password", "SECRET-PASSWORD-123")],
        Some(("Key-6-test.dat", b"\x01\x02fake-key-material".as_slice())),
    )
    .await;
    assert_eq!(s, 200, "PUT key A: {v}");
    assert_eq!(v["key"]["file_configured"], true, "{v}");
    assert_eq!(v["key"]["password_configured"], true, "{v}");
    assert_eq!(v["key"]["source"], "keystore", "{v}");
    let raw = v.to_string();
    assert!(
        !raw.contains("SECRET-PASSWORD-123") && !raw.contains("fake-key-material"),
        "PUT-відповідь не має містити секрети: {v}"
    );
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_a}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "GET після ключа: {v}");
    assert_eq!(v["key"]["file_configured"], true, "{v}");
    assert_eq!(v["key"]["file_name"], "Key-6-test.dat", "{v}");
    let raw = v.to_string();
    assert!(
        !raw.contains("SECRET-PASSWORD-123") && !raw.contains("fake-key-material"),
        "GET НІКОЛИ не повертає ключ/пароль у plaintext: {v}"
    );
    assert!(!raw.contains("key_file_content"), "{v}");
    // Без ключа (keystore Б окремий) — файл А не «протікає» в Б.
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_b}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "GET Б після ключа А: {v}");
    assert_eq!(v["key"]["file_configured"], false, "ключ А не видно в Б: {v}");
    assert_eq!(v["key"]["password_configured"], false, "{v}");

    // 5g. Остання зміна точки А (signer — серійний № сертифіката).
    sqlx::query(
        "INSERT INTO prro_shifts (id, store_id, shift_number, opened_at, signer_serial, signer_name,
                                  status, receipt_count, total_amount, last_local_number)
         VALUES ($1, $2, 7, now(), 'SERIAL-ABC-0001', 'ФОП Тест', 'open'::public.prro_shift_status,
                 12, 0::numeric, 0)",
    )
    .bind(Uuid::new_v4())
    .bind(store_a)
    .execute(&pool)
    .await
    .expect("seed prro_shift А");
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_a}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "GET А зі зміною: {v}");
    assert_eq!(v["key"]["signer_serial"], "SERIAL-ABC-0001", "{v}");
    let shift = v["last_shift"].as_object().expect("last_shift є");
    assert_eq!(shift["shift_number"], 7);
    assert_eq!(shift["status"], "open");
    assert_eq!(shift["receipt_count"], 12);
    assert_eq!(v["settings_updated_at"].is_null(), false);

    // 5h. Audit-запис після PUT prro (2 PUT: конфіг А + ключ А + конфіг Б —
    //     тільки успішні PUT А(конфіг), А(ключ), Б(конфіг) = 3 записи).
    let (s, v) = req_json(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/audit-log?action=prro_settings_updated",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 3, "3 PUT prro: {v}");
    for it in v["items"].as_array().expect("items") {
        assert_eq!(it["action"], "prro_settings_updated", "{v}");
        assert!(it["store_id"] == json!(store_a.to_string()) || it["store_id"] == json!(store_b.to_string()));
        assert_eq!(it["actor_name"], "E2E Audit Owner", "{v}");
    }

    // 5i. Неіснуюча точка → 404 (GET та PUT).
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
    let (s, v) = prro_put(&base, &ghost, &owner, &[("prro_fn", "400000000001")], None).await;
    assert_eq!(s, 404, "PUT неіснуючої точки: {v}");

    // 5j. Валідація форматів: невірний ФН → 400, конфіг А не змінюється.
    let (s, v) = prro_put(
        &base,
        &store_a,
        &owner,
        &[("prro_fn", "abc-not-digits")],
        None,
    )
    .await;
    assert_eq!(s, 400, "невірний prro_fn: {v}");
    let (_s, v) = req_json(
        &base,
        reqwest::Method::GET,
        &format!("/api/v1/admin/stores/{store_a}/prro-settings"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(v["settings"]["prro_fn"], "400000123456", "конфіг не змінився: {v}");

    // Гігієна: прибрати per-store файли КЕП (keystore/master/файл ключа), які
    // тест створює у CWD (crates/torgashka-api/), щоб робоче дерево лишалось чистим.
    let tag = store_a.simple().to_string();
    let _ = std::fs::remove_file(format!(".prro_keystore_{tag}.json"));
    let _ = std::fs::remove_file(format!(".prro_master_{tag}.key"));
    let _ = std::fs::remove_dir_all(format!("certs/prro-test/{tag}"));

    pool.close().await;
}
