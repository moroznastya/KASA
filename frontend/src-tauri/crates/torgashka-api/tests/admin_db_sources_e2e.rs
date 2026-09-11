//! E2E: «Джерело даних» (Етап 3 адмін-панелі, ТЗ 2.4/5.8).
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL + pg_dump/pg_restore:
//!   GET/POST/PUT/DELETE /api/v1/admin/db-sources
//!   POST /api/v1/admin/db-sources/:id/test
//!   POST /api/v1/admin/db-sources/:id/activate
//!   POST /api/v1/admin/db-sources/export-dump
//!   GET  /api/v1/admin/db-sources/dumps
//!   POST /api/v1/admin/db-sources/import-dump
//!
//! Критерії прийняття Етапа 3:
//!   1. activate з НЕдосяжним джерелом → 400 з причиною, active НЕ змінюється;
//!   2. activate з досяжним → 200, active збережено у db_sources.toml,
//!      applied_immediately=false + чесне повідомлення про рестарт (stability_first);
//!   3. db_sources.toml створюється з правами 0600; пароль у файлі ЛИШЕ в
//!      password_encrypted (AES-GCM), не у plaintext; .dbkey 0600;
//!   4. експорт дампу активної БД → файл .dump, який імпортується назад у нову
//!      (порожню) БД → у ній з'являються таблиці+дані;
//!   5. CRUD: створити/редагувати/видалити НЕактивне; видалити АКТИВНЕ → 409;
//!   6. RBAC: cashier → 403 на /admin/db-sources.
//!
//! Конфіг db_sources.toml ізолюється в temp-директорії через env
//! TORGASHKA_DB_SOURCES (жодного файлу в репозиторії); ключ .dbkey
//! генерується автоматично (env TORGASHKA_DBKEY не задано — тестуємо
//! авто-створення ключового файлу).

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
        let dir = std::env::temp_dir().join(format!("torgashka_dbsrc_e2e_{}", std::process::id()));
        let cfg = dir.join("db_sources.toml");
        let _ = std::fs::remove_file(&cfg);
        let _ = std::fs::remove_file(dir.join(".dbkey"));
        let _ = std::fs::remove_dir_all(dir.join("dumps"));
        std::env::set_var("TORGASHKA_DB_SOURCES", &cfg);
        dir
    })
}

async fn seed_users(pool: &sqlx::PgPool, tag: &str) -> (String, String) {
    let owner_login = format!("dbsrc_owner_{tag}");
    let cashier_login = format!("dbsrc_cashier_{tag}");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E DbSources Owner', $2, $3, 'owner'::public.user_role, true, now(), now(), true)
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
         VALUES ($1, 'E2E DbSources Cashier', $2, $3, 'cashier'::public.user_role, true, now(), now(), true)
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

// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn db_sources_full_lifecycle_test_activate_dumps_rbac() {
    common::force_test_db();
    let dir = sources_dir();
    let cfg_path = dir.join("db_sources.toml");
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string()[..10].to_string();
    let (owner_login, cashier_login) = seed_users(&pool, &tag).await;

    // ── Тестова БД + адмін-з'єднання (для CREATE/DROP scratch-БД) ──
    let test_url = std::env::var("DATABASE_URL").expect("DATABASE_URL (force_test_db)");
    let parts = split_url(&test_url);
    let admin_pool = sqlx::PgPool::connect(&url_of(&parts, "postgres"))
        .await
        .expect("підключення до postgres (адмін)");
    let fresh_db = format!("dbsrc_e2e_{tag}");
    sqlx::query(&format!("DROP DATABASE IF EXISTS {fresh_db}"))
        .execute(&admin_pool)
        .await
        .ok();
    sqlx::query(&format!("CREATE DATABASE {fresh_db}"))
        .execute(&admin_pool)
        .await
        .expect("створити порожню scratch-БД");

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner_tok = login(&base, &owner_login).await;
    let cashier_tok = login(&base, &cashier_login).await;

    // ── RBAC: cashier → 403 ──
    let (s403, _b) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/db-sources",
        Some(&cashier_tok),
        None,
    )
    .await;
    assert_eq!(s403, 403, "cashier має отримати 403 на /admin/db-sources");

    // ── Початковий стан: порожньо ──
    let (s0, list0) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/db-sources",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(s0, 200, "список джерел: {list0}");
    assert_eq!(list0["active"], Value::Null);
    assert_eq!(list0["sources"].as_array().map(|a| a.len()), Some(0));

    // ── Створення НЕдосяжного джерела + activate має ПРОВАЛИТИСЬ ──
    let (sc, unreach) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources",
        Some(&owner_tok),
        Some(json!({
            "id": "unreach",
            "label": "Недосяжне",
            "host": "127.0.0.1",
            "port": 59999,
            "database": "nodb",
            "user": "nobody",
            "password": "SecretPlain_1",
        })),
    )
    .await;
    assert_eq!(sc, 201, "створено unreach: {unreach}");
    let (sa, bad_activate) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/unreach/activate",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sa, 400, "activate недосяжного має впасти: {bad_activate}");
    assert!(
        bad_activate["detail"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("недосяжне"),
        "detail має містити причину: {bad_activate}"
    );

    // ── Створення досяжного джерела (головна тестова БД) ──
    let (sc2, prim) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources",
        Some(&owner_tok),
        Some(json!({
            "id": "primary",
            "label": "Основна (e2e)",
            "host": parts.host,
            "port": parts.port,
            "database": parts.database,
            "user": parts.user,
            "password": parts.password,
        })),
    )
    .await;
    assert_eq!(sc2, 201, "створено primary: {prim}");
    assert_eq!(prim["has_password"], json!(true));
    assert_eq!(prim["is_active"], json!(false));
    assert!(
        prim.get("password_encrypted").is_none(),
        "пароль не віддається"
    );
    assert!(prim.get("password").is_none(), "пароль не віддається");

    // ── test-з'єднання досяжного джерела ──
    let (st, tested) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/primary/test",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(st, 200, "test primary: {tested}");
    assert_eq!(tested["ok"], json!(true));

    // ── activate досяжного → active збережено, applied_immediately=false ──
    let (sac, act) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/primary/activate",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sac, 200, "activate primary: {act}");
    assert_eq!(act["active"], json!("primary"));
    assert_eq!(act["applied_immediately"], json!(false));
    assert!(
        act["message"]
            .as_str()
            .unwrap_or("")
            .contains("ПЕРЕЗАПУСКУ"),
        "чесна відповідь про рестарт: {act}"
    );

    // ── Файл: 0600, пароль не у plaintext, .dbkey 0600 ──
    let raw = std::fs::read_to_string(&cfg_path).expect("db_sources.toml створено");
    assert!(raw.contains("active = \"primary\""), "{raw}");
    assert!(
        !raw.contains("SecretPlain_1") && !raw.contains(&parts.password),
        "пароль не має бути у plaintext: {raw}"
    );
    assert!(raw.contains("password_encrypted"), "{raw}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(&cfg_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, 0o600, "db_sources.toml 0600, отримано {m:o}");
        let key = dir.join(".dbkey");
        assert!(key.is_file(), ".dbkey створено автоматично");
        let km = std::fs::metadata(&key).unwrap().permissions().mode() & 0o777;
        assert_eq!(km, 0o600, ".dbkey 0600, отримано {km:o}");
    }

    // ── Редагування (PUT): label/host ──
    let (su, upd) = req(
        &base,
        reqwest::Method::PUT,
        "/api/v1/admin/db-sources/primary",
        Some(&owner_tok),
        Some(json!({"label": "Основна (e2e, оновлено)"})),
    )
    .await;
    assert_eq!(su, 200, "PUT primary: {upd}");
    assert_eq!(upd["label"], json!("Основна (e2e, оновлено)"));

    // ── scratch-джерело «target» для імпорту ──
    let (sc3, tgt) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources",
        Some(&owner_tok),
        Some(json!({
            "id": "target",
            "label": "Приймач (порожня БД)",
            "host": parts.host,
            "port": parts.port,
            "database": fresh_db,
            "user": parts.user,
            "password": parts.password,
        })),
    )
    .await;
    assert_eq!(sc3, 201, "створено target: {tgt}");

    // ── Експорт дампу активної БД ──
    let (se, dump) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/export-dump",
        Some(&owner_tok),
        Some(json!({})),
    )
    .await;
    assert_eq!(se, 200, "export-dump: {dump}");
    let file = dump["file"].as_str().expect("file").to_string();
    assert!(file.ends_with(".sql"), "файл дампу: {file}");
    let size = dump["size_bytes"].as_u64().unwrap_or(0);
    assert!(size > 0, "дамп не порожній");
    let dump_path = dir.join("dumps").join(&file);
    assert!(
        dump_path.is_file(),
        "файл існує на диску: {}",
        dump_path.display()
    );

    // ── Список дампів ──
    let (sd, dumps) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/db-sources/dumps",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sd, 200, "список дампів: {dumps}");
    assert!(
        dumps
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["file"] == json!(file)),
        "дамп у списку: {dumps}"
    );

    // ── Імпорт у target (порожня БД) ──
    let (si, imp) = req(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/db-sources/import-dump",
        Some(&owner_tok),
        Some(json!({"source_id": "target", "file": file})),
    )
    .await;
    assert_eq!(si, 200, "import-dump: {imp}");
    assert_eq!(imp["ok"], json!(true));

    // ── Перевірка: у target з'явились таблиці + дані (схема імпортована) ──
    let target_url = url_of(&parts, &fresh_db);
    let tp = sqlx::PgPool::connect(&target_url)
        .await
        .expect("target pool");
    let has_users: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(&tp)
        .await
        .expect("users у target");
    assert!(has_users, "після імпорту у target є таблиця users");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE is_active = true")
        .fetch_one(&tp)
        .await
        .expect("count users");
    assert!(rows >= 2, "імпортовано дані користувачів: {rows} рядків");
    let has_roles: bool =
        sqlx::query_scalar("SELECT count(*) > 0 FROM pg_type WHERE typname = 'user_role'")
            .fetch_one(&tp)
            .await
            .expect("enum user_role у target");
    assert!(has_roles, "enum user_role імпортовано");
    tp.close().await;

    // ── Видалення: активне → 409; неактивне → 200; неіснуюче → 404 ──
    let (sd1, del_active) = req(
        &base,
        reqwest::Method::DELETE,
        "/api/v1/admin/db-sources/primary",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sd1, 409, "активне не видаляється: {del_active}");
    let (sd2, del_unreach) = req(
        &base,
        reqwest::Method::DELETE,
        "/api/v1/admin/db-sources/unreach",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sd2, 200, "unreach видалено: {del_unreach}");
    assert_eq!(del_unreach["removed"], json!("unreach"));
    let (sd3, del_missing) = req(
        &base,
        reqwest::Method::DELETE,
        "/api/v1/admin/db-sources/no_such",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sd3, 404, "неіснуюче → 404: {del_missing}");
    // Фінальний список: primary (активне) + target.
    let (sf, final_list) = req(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/db-sources",
        Some(&owner_tok),
        None,
    )
    .await;
    assert_eq!(sf, 200);
    let ids: Vec<&str> = final_list["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec!["primary", "target"],
        "фінальний список: {final_list}"
    );
    assert_eq!(final_list["active"], json!("primary"));

    // ── Очищення scratch-БД ──
    sqlx::query(&format!("DROP DATABASE IF EXISTS {fresh_db}"))
        .execute(&admin_pool)
        .await
        .ok();
    admin_pool.close().await;
}
