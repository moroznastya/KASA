//! E2E: POST /api/v1/admin/db-sources/provision — бекенд provisioning БД
//! (Етап 1 sync-offline). Реальний PostgreSQL + реальний Rust-фасад.
//!
//! Сценарії:
//!   1. owner provision НОВОЇ БД на локальному PG16 (суперкористувач postgres
//!      з backend/.env) → 201; на цільовому сервері з'явилась БД з ПОВНОЮ
//!      схемою (users/stores існують, stores порожній); джерело записане у
//!      db_sources.toml (temp через TORGASHKA_DB_SOURCES) зі
//!      status=provisioned_pending_activation; пароль лише зашифрований;
//!   2. POST /:id/test на створене джерело (user=torgashka_app + згенерований
//!      пароль) → ok=true;
//!   3. повторний provision того самого database → 409;
//!   4. cashier / admin (ролі ≠ owner) → 403;
//!   5. суперкредити відсутні у відповіді та у файлі (grep db_sources.toml).
//!
//! Патерн — той самий, що в admin_db_sources_e2e.rs: common::force_test_db(),
//! apply_schema(), run_facade на free_port, реальний PostgreSQL.

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

fn url_of(parts: &PgParts, database: &str) -> String {
    format!(
        "postgresql://{}:{}@{}:{}/{}",
        parts.user, parts.password, parts.host, parts.port, database
    )
}

/// temp-директорія db_sources.toml для всього тест-бінарника (env глобальний).
fn sources_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("torgashka_prov_e2e_{}", std::process::id()));
        let cfg = dir.join("db_sources.toml");
        let _ = std::fs::remove_file(&cfg);
        let _ = std::fs::remove_file(dir.join(".dbkey"));
        let _ = std::fs::remove_dir_all(dir.join("dumps"));
        std::env::set_var("TORGASHKA_DB_SOURCES", &cfg);
        dir
    })
}

async fn seed_users(pool: &sqlx::PgPool, tag: &str) -> (String, String, String) {
    let owner_login = format!("prov_owner_{tag}");
    let cashier_login = format!("prov_cashier_{tag}");
    let admin_login = format!("prov_admin_{tag}");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Provision Owner', $2, $3, 'owner'::public.user_role, true, now(), now(), true)
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
         VALUES ($1, 'E2E Provision Cashier', $2, $3, 'cashier'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&cashier_login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed cashier");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Provision Admin', $2, $3, 'admin'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&admin_login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed admin");
    (owner_login, cashier_login, admin_login)
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

async fn drop_db(admin_pool: &sqlx::PgPool, db_name: &str) {
    sqlx::query(&format!("DROP DATABASE IF EXISTS {db_name}"))
        .execute(admin_pool)
        .await
        .ok();
}

// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn provision_full_lifecycle_owner_only() {
    common::force_test_db();
    let dir = sources_dir();
    let cfg_path = dir.join("db_sources.toml");
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, cashier_login, admin_login) = seed_users(&pool, &tag[..10]).await;

    // ── Адмін-з'єднання (для CREATE/DROP scratch-БД і перевірки схеми) ──
    let test_url = std::env::var("DATABASE_URL").expect("DATABASE_URL (force_test_db)");
    let parts = split_url(&test_url);
    let admin_pool = sqlx::PgPool::connect(&url_of(&parts, "postgres"))
        .await
        .expect("підключення до postgres (адмін)");
    let new_db = format!("prov_e2e_{tag}");
    // Прибрати можливий залишок від раніше впалого запуску.
    drop_db(&admin_pool, &new_db).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner_tok = login(&base, &owner_login).await;
    let cashier_tok = login(&base, &cashier_login).await;
    let admin_tok = login(&base, &admin_login).await;

    // ── RBAC: cashier/admin (ролі ≠ owner) → 403 ──
    let provision_body = || {
        json!({
            "id": "prov_src",
            "label": "Провіжинінг (e2e)",
            "host": parts.host,
            "port": parts.port,
            "database": new_db,
            "superuser": { "user": parts.user, "password": parts.password },
        })
    };
    for (tok, role) in [(&cashier_tok, "cashier"), (&admin_tok, "admin")] {
        let (s, b) = req(
            &base,
            reqwest::Method::POST,
            "/api/v1/admin/db-sources/provision",
            Some(tok),
            Some(provision_body()),
        )
        .await;
        assert_eq!(s, 403, "{role} має отримати 403 на provision: {b}");
    }

    // ── 1. Успішний provision (owner) → 201 ──
    let (sc, resp) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/provision",
        Some(&owner_tok),
        Some(provision_body()),
    )
    .await;
    assert_eq!(sc, 201, "provision owner: {resp}");
    let src = &resp["source"];
    assert_eq!(src["id"], json!("prov_src"), "{resp}");
    assert_eq!(src["user"], json!("torgashka_app"), "{resp}");
    assert_eq!(
        src["status"], json!("provisioned_pending_activation"),
        "статус створено-не-активовано: {resp}"
    );
    assert_eq!(src["is_active"], json!(false), "не активовано автоматично: {resp}");
    assert_eq!(src["has_password"], json!(true), "{resp}");
    assert!(
        src.get("password_encrypted").is_none() && src.get("password").is_none(),
        "пароль не віддається: {resp}"
    );
    assert!(
        !resp.to_string().contains(&parts.password),
        "суперкредити НЕ у відповіді: {resp}"
    );
    assert!(
        resp["message"]
            .as_str()
            .unwrap_or("")
            .contains("НЕ активовано")
            || resp["message"]
                .as_str()
                .unwrap_or("")
                .contains("не активовано"),
        "чесне повідомлення про окрему активацію: {resp}"
    );

    // ── 2. На цільовому сервері з'явилась БД з ПОВНОЮ схемою ──
    let ndb_url = url_of(&parts, &new_db);
    let ndb_pool = sqlx::PgPool::connect(&ndb_url)
        .await
        .expect("підключення до нової БД");
    let has_users: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(&ndb_pool)
        .await
        .expect("users у новій БД");
    let has_stores: bool =
        sqlx::query_scalar("SELECT to_regclass('public.stores') IS NOT NULL")
            .fetch_one(&ndb_pool)
            .await
            .expect("stores у новій БД");
    assert!(has_users && has_stores, "повна схема (users/stores) у новій БД");
    let stores_count: i64 = sqlx::query_scalar("SELECT count(*) FROM stores")
        .fetch_one(&ndb_pool)
        .await
        .expect("count stores");
    assert_eq!(stores_count, 0, "stores у новій БД порожній");
    ndb_pool.close().await;

    // ── 3. Джерело у db_sources.toml: status + пароль лише зашифрований ──
    let raw = std::fs::read_to_string(&cfg_path).expect("db_sources.toml створено");
    assert!(raw.contains("[sources.prov_src]"), "{raw}");
    assert!(
        raw.contains("provisioned_pending_activation"),
        "status у файлі: {raw}"
    );
    assert!(raw.contains("user = \"torgashka_app\""), "{raw}");
    assert!(raw.contains("password_encrypted"), "{raw}");
    assert!(
        !raw.contains(&parts.password),
        "суперкредити НЕ у файлі: {raw}"
    );
    assert!(
        !raw.contains("superuser"),
        "суперкористувацький блок відсутній у файлі: {raw}"
    );
    assert!(
        !raw.contains("active = \"prov_src\""),
        "джерело НЕ активоване автоматично: {raw}"
    );

    // ── 4. POST /:id/test на створене джерело → ok=true ──
    let (st, tested) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/prov_src/test",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(st, 200, "test provisioned source: {tested}");
    assert_eq!(tested["ok"], json!(true), "torgashka_app підключається: {tested}");

    // ── 5. Повторний provision того самого database → 409 ──
    let dup_body = json!({
        "id": "prov_src_dup",
        "label": "Дубль (e2e)",
        "host": parts.host,
        "port": parts.port,
        "database": new_db,
        "superuser": { "user": parts.user, "password": parts.password },
    });
    let (sd, dup) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/provision",
        Some(&owner_tok),
        Some(dup_body),
    )
    .await;
    assert_eq!(sd, 409, "повторний provision того самого database → 409: {dup}");
    assert!(
        dup["detail"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("уже існує")
            || dup["detail"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("already exists"),
        "зрозумілий detail: {dup}"
    );

    // ── Очищення: scratch-БД прибрано (залишків після тесту немає) ──
    drop_db(&admin_pool, &new_db).await;
    admin_pool.close().await;
    pool.close().await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Валідація: невірне ім'я БД → 400 (без жодних змін на сервері).
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn provision_invalid_db_name_400() {
    common::force_test_db();
    sources_dir();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();
    let (owner_login, _, _) = seed_users(&pool, &tag[..10]).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner_tok = login(&base, &owner_login).await;

    // Ім'я з великої літери / дефісом — заборонене (^[a-z_][a-z0-9_]{0,62}$).
    let bad_body = json!({
        "id": "bad_db",
        "host": "localhost",
        "port": 5432,
        "database": "Bad-DB_Name",
        "superuser": { "user": "postgres", "password": "x" },
    });
    let (s, b) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/provision",
        Some(&owner_tok),
        Some(bad_body),
    )
    .await;
    assert_eq!(s, 400, "невірне ім'я БД → 400: {b}");
    assert!(
        b["detail"].as_str().unwrap_or("").contains("Невірне ім'я"),
        "detail пояснює: {b}"
    );
    pool.close().await;
}
