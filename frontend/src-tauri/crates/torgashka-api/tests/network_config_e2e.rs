//! E2E: «Конфіг-файл мережі» (Етап 3-backend, ОБОВ'ЯЗКОВА вимога Творця;
//! безпека — Етап 5).
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL (реальний PG16
//! localhost:5432, backend/.env):
//!   POST /api/v1/admin/network-config/export → 200 { filename, content }
//!   POST /api/v1/admin/network-config/import → 200 { ok, network_id, store, server_url }
//!
//! Критерії прийняття:
//!   1. export: filename/content валідні; activation_code 8 символів A-Z0-9
//!      (алфавіт без 0/O/1/I); content містить network_id=owner users.id
//!      (канонічний вибір ORDER BY created_at), store.id/name, server_url;
//!   2. include_db_password=false → db.password_encrypted відсутній;
//!      true → присутній (base64, AES-256-GCM) і розшифровується назад;
//!      plaintext пароля ніколи не потрапляє у content;
//!   3. import того самого content → ok:true з тим самим network_id/store/server_url;
//!   4. import з підробленим code для нашої точки → 400; неіснуюча точка → 404;
//!      невалідний JSON → 400; schema_version != 1 → 400;
//!   5. RBAC: cashier/admin → 403 на export і import (owner-only, Етап 5);
//!   6. server_url: явний body > env TORGASHKA_FACADE_ADDR > дефолт.
//!
//! Конфіг db_sources.toml ізолюється у temp через env TORGASHKA_DB_SOURCES
//! (як admin_db_sources_e2e); активне джерело створюється/активується через
//! реальні адмін-ендпоінти /admin/db-sources. Жоден файл не пишеться в репозиторій.

use std::path::PathBuf;
use std::sync::OnceLock;
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

/// Алфавіт коду активації (network.rs gen_code: A-Z0-9 БЕЗ 0/O/1/I).
fn code_alphabet_ok(code: &str) -> bool {
    code.len() == 8
        && code.chars().all(|c| {
            (c.is_ascii_uppercase() && matches!(c, 'A'..='H' | 'J'..='N' | 'P'..='Z'))
                || (c.is_ascii_digit() && matches!(c, '2'..='9'))
        })
}

/// Розбір postgresql://user:pass@host:port/db на компоненти.
#[derive(Debug, Clone)]
struct PgParts {
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

fn split_url(url: &str) -> PgParts {
    let rest = url
        .strip_prefix("postgresql://")
        .or_else(|| url.strip_prefix("postgres://"))
        .expect("postgresql:// URL");
    let (userinfo, host_db) = rest.rsplit_once('@').expect("@");
    let (user, password) = match userinfo.rsplit_once(':') {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => (userinfo.to_string(), String::new()),
    };
    let (hostport, database) = host_db.rsplit_once('/').expect("db");
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().expect("port")),
        None => (hostport.to_string(), 5432),
    };
    PgParts {
        host,
        port,
        user,
        password,
        database: database.to_string(),
    }
}

/// temp-директорія db_sources.toml для всього тест-бінарника (env глобальний).
fn sources_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("torgashka_netcfg_e2e_{}", std::process::id()));
        let cfg = dir.join("db_sources.toml");
        let _ = std::fs::remove_file(&cfg);
        let _ = std::fs::remove_file(dir.join(".dbkey"));
        let _ = std::fs::remove_dir_all(dir.join("dumps"));
        std::env::set_var("TORGASHKA_DB_SOURCES", &cfg);
        dir
    })
}

async fn seed_users(pool: &sqlx::PgPool, tag: &str) -> (String, String, String) {
    let owner_login = format!("ncfg_owner_{tag}");
    let admin_login = format!("ncfg_admin_{tag}");
    let cashier_login = format!("ncfg_cashier_{tag}");
    for (login, role) in [
        (owner_login.clone(), "owner"),
        (admin_login.clone(), "admin"),
        (cashier_login.clone(), "cashier"),
    ] {
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
             VALUES ($1, $2, $3, $4, $5::public.user_role, true, now(), now(), true)
             ON CONFLICT (login) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(format!("E2E NetCfg {role}"))
        .bind(&login)
        .bind(PWD)
        .bind(role)
        .execute(pool)
        .await
        .expect("seed user");
    }
    (owner_login, admin_login, cashier_login)
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

/// Канонічний network_id (той самий запит, що в export-хендлері).
async fn canonical_owner_id(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM users WHERE role = 'owner'::public.user_role \
         ORDER BY created_at, id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("owner id")
}

// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn network_config_export_import_lifecycle() {
    common::force_test_db();
    let dir = sources_dir();
    let cfg_path = dir.join("db_sources.toml");
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string()[..10].to_string();
    let (owner_login, admin_login, cashier_login) = seed_users(&pool, &tag).await;

    let test_url = std::env::var("DATABASE_URL").expect("DATABASE_URL (force_test_db)");
    let parts = split_url(&test_url);
    assert!(
        !parts.password.is_empty(),
        "потрібен пароль БД у backend/.env для перевірки шифрування"
    );

    // ── Підняти фасад + логіни ──
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner_tok = login(&base, &owner_login).await;
    let admin_tok = login(&base, &admin_login).await;
    let cashier_tok = login(&base, &cashier_login).await;

    // ── Активне джерело (реальні ендпоінти /admin/db-sources) ──
    let (sc, prim) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources",
        Some(&owner_tok),
        Some(json!({
            "id": "primary",
            "label": "Основна (netcfg e2e)",
            "host": parts.host,
            "port": parts.port,
            "database": parts.database,
            "user": parts.user,
            "password": parts.password,
        })),
    )
    .await;
    assert_eq!(sc, 201, "створити джерело: {prim}");
    let (sa, act) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/primary/activate",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sa, 200, "активувати джерело: {act}");
    assert_eq!(act["active"], json!("primary"));
    let raw = std::fs::read_to_string(&cfg_path).expect("db_sources.toml");
    assert!(raw.contains("password_encrypted"), "{raw}");

    // ── Точка ──
    let (ss, store) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        Some(&owner_tok),
        Some(json!({"name": "NC E2E Store 1"})),
    )
    .await;
    assert_eq!(ss, 201, "створити точку: {store}");
    let store_id = store["id"].as_str().expect("store.id").to_string();
    let store_name = store["name"].as_str().expect("store.name").to_string();

    // ── RBAC (Етап 5): cashier і admin → 403 ──
    for tok in [&cashier_tok, &admin_tok] {
        let (s1, _b) = req(
            &base,
            reqwest::Method::POST,
            "/api/v1/admin/network-config/export",
            Some(tok),
            Some(json!({"store_id": store_id})),
        )
        .await;
        assert_eq!(s1, 403, "export: не-owner має отримати 403");
        let (s2, _b2) = req(
            &base,
            reqwest::Method::POST,
            "/api/v1/admin/network-config/import",
            Some(tok),
            Some(json!({"content": "{}"})),
        )
        .await;
        assert_eq!(s2, 403, "import: не-owner має отримати 403");
    }

    // ── Export #1 (без env TORGASHKA_FACADE_ADDR) → дефолтний server_url ──
    std::env::remove_var("TORGASHKA_FACADE_ADDR");
    let (se, exp) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/export",
        Some(&owner_tok),
        Some(json!({"store_id": store_id})),
    )
    .await;
    assert_eq!(se, 200, "export: {exp}");
    let filename = exp["filename"].as_str().expect("filename").to_string();
    assert!(
        filename.starts_with("torgashka-network-nc-e2e-store-1-") && filename.ends_with(".json"),
        "filename: {filename}"
    );
    let date_part = filename
        .strip_prefix("torgashka-network-nc-e2e-store-1-")
        .and_then(|s| s.strip_suffix(".json"))
        .expect("date part");
    assert_eq!(date_part.len(), 8, "дата YYYYmmdd: {date_part}");
    assert!(
        date_part.chars().all(|c| c.is_ascii_digit()),
        "дата: {date_part}"
    );

    let content = exp["content"].as_str().expect("content").to_string();
    let cfg1: Value = serde_json::from_str(&content).expect("content — валідний JSON");
    assert_eq!(cfg1["schema_version"], json!(1));
    assert_eq!(cfg1["server_url"], json!("http://127.0.0.1:8000"));

    let owner_id = canonical_owner_id(&pool).await;
    assert_eq!(
        cfg1["network_id"].as_str().unwrap_or(""),
        owner_id.to_string(),
        "network_id = users.id власника"
    );
    assert_eq!(cfg1["store"]["id"].as_str().unwrap_or(""), store_id);
    assert_eq!(cfg1["store"]["name"].as_str().unwrap_or(""), store_name);
    let code1 = cfg1["store"]["activation_code"]
        .as_str()
        .expect("code")
        .to_string();
    assert!(code_alphabet_ok(&code1), "код активації: {code1}");
    assert!(
        cfg1["exported_at"].as_str().unwrap_or("").contains('T'),
        "exported_at RFC3339: {cfg1}"
    );

    // db-блок присутній (активне джерело є); без include_db_password →
    // password_encrypted ВІДСУТНІЙ.
    assert_eq!(cfg1["db"]["host"], json!(parts.host));
    assert_eq!(cfg1["db"]["port"], json!(parts.port));
    assert_eq!(cfg1["db"]["database"], json!(parts.database));
    assert_eq!(cfg1["db"]["user"], json!(parts.user));
    assert!(
        cfg1["db"].get("password_encrypted").is_none(),
        "без include_db_password пароль не включається: {cfg1}"
    );
    assert!(
        !content.contains(&parts.password),
        "plaintext пароль не потрапляє у content"
    );

    // ── Export #2: include_db_password=true → password_encrypted присутній ──
    let (se2, exp2) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/export",
        Some(&owner_tok),
        Some(json!({
            "store_id": store_id,
            "include_db_password": true,
        })),
    )
    .await;
    assert_eq!(se2, 200, "export #2: {exp2}");
    let cfg2: Value =
        serde_json::from_str(exp2["content"].as_str().expect("content2")).expect("json2");
    let enc = cfg2["db"]["password_encrypted"]
        .as_str()
        .expect("password_encrypted присутній");
    assert!(!enc.is_empty(), "password_encrypted не порожній");
    assert!(
        enc.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')),
        "base64: {enc}"
    );
    assert!(
        !exp2["content"]
            .as_str()
            .unwrap_or("")
            .contains(&parts.password),
        "content не містить plaintext пароля"
    );
    // Розшифровка назад дає оригінал (AES-256-GCM, ключ .dbkey у temp-дирі).
    let decrypted =
        torgashka_infrastructure::db_sources::decrypt_password(&cfg_path, enc).expect("decrypt");
    assert_eq!(decrypted, parts.password, "розшифрований пароль = оригінал");

    // Код активації пере-генеровано на кожному export (#2 ≠ #1).
    let code2 = cfg2["store"]["activation_code"]
        .as_str()
        .expect("code2")
        .to_string();
    assert!(code_alphabet_ok(&code2), "код #2: {code2}");
    assert_ne!(code1, code2, "код має пере-генеруватись на кожному export");

    // ── server_url: env > дефолт (request-time читання env) ──
    std::env::set_var("TORGASHKA_FACADE_ADDR", "100.64.0.9:8000");
    let (se3, exp3) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/export",
        Some(&owner_tok),
        Some(json!({"store_id": store_id, "server_url": "  "})),
    )
    .await;
    assert_eq!(se3, 200);
    let cfg3: Value =
        serde_json::from_str(exp3["content"].as_str().expect("content3")).expect("json3");
    assert_eq!(
        cfg3["server_url"],
        json!("http://100.64.0.9:8000"),
        "env TORGASHKA_FACADE_ADDR має виграти у дефолту"
    );
    // Явний body.server_url > env.
    let (se4, exp4) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/export",
        Some(&owner_tok),
        Some(json!({
            "store_id": store_id,
            "server_url": "https://vpn.example:8443",
        })),
    )
    .await;
    assert_eq!(se4, 200);
    let cfg4: Value =
        serde_json::from_str(exp4["content"].as_str().expect("content4")).expect("json4");
    assert_eq!(cfg4["server_url"], json!("https://vpn.example:8443"));
    std::env::remove_var("TORGASHKA_FACADE_ADDR");

    // ── Import поточного content (#4) → ok:true ──
    let (si, imp) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/import",
        Some(&owner_tok),
        Some(json!({"content": cfg4.to_string()})),
    )
    .await;
    assert_eq!(si, 200, "import: {imp}");
    assert_eq!(imp["ok"], json!(true));
    assert_eq!(
        imp["network_id"].as_str().unwrap_or(""),
        cfg4["network_id"].as_str().unwrap_or("")
    );
    assert_eq!(imp["store"]["id"].as_str().unwrap_or(""), store_id);
    assert_eq!(imp["store"]["name"].as_str().unwrap_or(""), store_name);
    assert_eq!(
        imp["server_url"].as_str().unwrap_or(""),
        "https://vpn.example:8443"
    );

    // ── Import СТАРОГО content (#1, з попереднім кодом) → 400 ──
    let (si2, imp2) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/import",
        Some(&owner_tok),
        Some(json!({"content": content.clone()})),
    )
    .await;
    assert_eq!(
        si2, 400,
        "застарілий код (після пере-генерації) має бути відхилено: {imp2}"
    );

    // ── Import: підроблений code для нашої точки → 400 ──
    let mut forged = cfg4.clone();
    forged["store"]["activation_code"] = json!("ZZZZZZZZ");
    let (sf, impf) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/import",
        Some(&owner_tok),
        Some(json!({"content": forged.to_string()})),
    )
    .await;
    assert_eq!(sf, 400, "чужий/підроблений код → 400: {impf}");

    // ── Import: неіснуюча точка → 404 ──
    let mut ghost = cfg4.clone();
    ghost["store"]["id"] = json!(Uuid::new_v4().to_string());
    let (sg, impg) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/import",
        Some(&owner_tok),
        Some(json!({"content": ghost.to_string()})),
    )
    .await;
    assert_eq!(sg, 404, "неіснуюча точка → 404: {impg}");

    // ── Import: невалідний JSON / schema_version != 1 / битий формат → 400 ──
    let (sb1, b1) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/import",
        Some(&owner_tok),
        Some(json!({"content": "{not json"})),
    )
    .await;
    assert_eq!(sb1, 400, "невалідний JSON: {b1}");
    let (sb2, b2) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/import",
        Some(&owner_tok),
        Some(json!({"content": "{\"schema_version\": 2}"})),
    )
    .await;
    assert_eq!(sb2, 400, "schema_version != 1: {b2}");

    // ── Export неіснуючої точки → 404 ──
    let (sx, x) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/export",
        Some(&owner_tok),
        Some(json!({"store_id": Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(sx, 404, "export неіснуючої точки: {x}");

    // ── Export з невалідним store_id → 400 ──
    let (sx2, x2) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/network-config/export",
        Some(&owner_tok),
        Some(json!({"store_id": "не-uuid"})),
    )
    .await;
    assert_eq!(sx2, 400, "невалідний store_id: {x2}");
}
