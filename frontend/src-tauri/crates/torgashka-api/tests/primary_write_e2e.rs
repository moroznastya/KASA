//! E2E: ЗАПИС НА PRIMARY реально доходить у PostgreSQL.
//!
//! Прогалина, яку закриває тест: `write_gate_behavior::primary_mode_behavior_unchanged_f2`
//! доводить лише, що гейт НЕ ставить 503 на primary — і робить це БЕЗ жодного
//! PG-пулу (`app_state_has_no_pg_pools_in_this_test`). Доказу «запис виконався»
//! у сюїті не було (тільки «гейт не заважає»).
//!
//! Тут — реальний `run_facade` на ефемерному порту + реальний PostgreSQL
//! (тестова БД через `common::force_test_db`), режим вузла = **Primary**
//! (`db_sources.toml` без секції `[node]` → `NodeMode::default()` = Primary).
//!
//! Сценарії:
//!   1. `POST /api/v1/admin/stores` → 201, заголовок `x-torgashka-node-mode: primary`,
//!      БЕЗ `x-torgashka-upstream` (F2: primary нікуди не проксюється);
//!   2. рядок РЕАЛЬНО в PG — `SELECT name, address FROM stores WHERE id = $1`
//!      (прямий запит поза фасадом: незалежний доказ, а не само-підтвердження через API);
//!   3. читання через фасад (`GET /api/v1/admin/stores`) бачить той самий id;
//!   4. негатив: PG-помилка `22001` (name довше за `varchar(255)`) → 500,
//!      тіло БЕЗ сирого тексту PG (`value too long`, `22001`,
//!      `error returned from database`, `character varying`);
//!   5. жодного втручання рубіжного шару `readonly_net`: `funnel_hits`/`sanitized_hits`
//!      не зросли (сигнатура SQLx у тіло не потрапляє — `AdminErr` санує сам).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

mod common;

/// bcrypt('admin123') — той самий seed-пароль, що в решті e2e torgashka-api.
const PWD: &str = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e";

static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
static SOURCES: OnceLock<PathBuf> = OnceLock::new();

/// `db_sources.toml` БЕЗ секції `[node]` → `NodeConfig::load()` → Primary.
/// Ізоляція від конфігів машини: робочий файл репозиторію не читається.
fn isolate_sources() -> &'static Path {
    SOURCES.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("torgashka_primary_write_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp-каталог db_sources");
        let cfg = dir.join("db_sources.toml");
        std::fs::write(
            &cfg,
            "# e2e primary_write: секції [node] немає → режим Primary\n",
        )
        .expect("db_sources.toml");
        std::env::set_var("TORGASHKA_DB_SOURCES", &cfg);
        cfg
    })
}

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

/// Пул для seed/перевірок: прямий (не read-only) — потрібен INSERT у `users`.
async fn direct_pool() -> sqlx::PgPool {
    let url = torgashka_infrastructure::db::resolve_database_url()
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env");
    sqlx::PgPool::connect(&url).await.expect("pool")
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

async fn seed_admin(pool: &sqlx::PgPool, tag: &str) -> String {
    let login = format!("pw_admin_{tag}");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Primary Write Admin', $2, $3, 'admin'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(&login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed admin");
    login
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

/// Повертає (статус, тіло, заголовки).
async fn req_full(
    base: &str,
    method: reqwest::Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, Value, reqwest::header::HeaderMap) {
    let client = reqwest::Client::new();
    let mut r = client
        .request(method, format!("{base}{path}"))
        .bearer_auth(token);
    if let Some(b) = body {
        r = r.json(&b);
    }
    let resp = r.send().await.expect("запит");
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body, headers)
}

fn header(h: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn primary_node_executes_business_write_into_postgres() {
    common::force_test_db();
    let _cfg = isolate_sources();
    apply_schema().await;

    let pool = direct_pool().await;
    let tag = Uuid::new_v4().simple().to_string();
    let admin_login = seed_admin(&pool, &tag[..10]).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = torgashka_api::run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let token = login(&base, &admin_login).await;

    let funnel_before = torgashka_infrastructure::readonly_guard::hits();
    let sanitized_before = torgashka_infrastructure::readonly_guard::sanitized_hits();

    // ── 1. Запис бізнесової сутності на primary → 201 ────────────────────────
    let store_name = format!("E2E Primary Store {}", &tag[..8]);
    let address = "вул. Тестова, 1 (e2e primary_write)";
    let (status, body, headers) = req_full(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        &token,
        Some(json!({
            "name": store_name,
            "address": address,
            "phone": "+380000000000",
            "legal_name": "E2E Primary Write LLC",
            "edrpou": "00000000",
        })),
    )
    .await;
    assert_eq!(
        status, 201,
        "запис на primary мусить пройти без гейта та без помилки: {body}"
    );
    assert_eq!(
        header(&headers, "x-torgashka-node-mode").as_deref(),
        Some("primary"),
        "вузол мусить бути primary (NodeMode::default без [node] у db_sources.toml)"
    );
    assert_eq!(
        header(&headers, "x-torgashka-upstream"),
        None,
        "F2: primary нікуди не проксюється — upstream-маркера бути не має"
    );
    let store_id = body["id"].as_str().expect("id створеної точки").to_string();
    let store_uuid = Uuid::parse_str(&store_id).expect("id — UUID");

    // ── 2. Незалежний доказ: рядок РЕАЛЬНО в PostgreSQL ─────────────────────
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT name, address FROM stores WHERE id = $1")
            .bind(store_uuid)
            .fetch_optional(&pool)
            .await
            .expect("SELECT stores");
    let (db_name, db_address) = row.expect("рядок stores мусить існувати в PG (запис дійшов)");
    assert_eq!(
        db_name, store_name,
        "ім'я в PG мусить дорівнювати надісланому"
    );
    assert_eq!(db_address.as_deref(), Some(address));

    // Автоприв'язка творця (owner) як власника точки — той самий комміт.
    let owner_links: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_stores WHERE store_id = $1 AND role = 'owner'",
    )
    .bind(store_uuid)
    .fetch_one(&pool)
    .await
    .expect("SELECT user_stores");
    assert_eq!(
        owner_links, 1,
        "творець мусить бути прив'язаний власником точки"
    );

    // ── 3. Читання через фасад бачить той самий id ──────────────────────────
    let (status, list, _) = req_full(
        &base,
        reqwest::Method::GET,
        "/api/v1/admin/stores",
        &token,
        None,
    )
    .await;
    assert_eq!(status, 200, "GET /admin/stores: {list}");
    let seen = list
        .as_array()
        .map(|arr| arr.iter().any(|s| s["id"] == json!(store_id)))
        .unwrap_or(false);
    assert!(seen, "щойно створена точка мусить бути у списку фасаду");

    // ── 4. Негатив: PG-помилка 22001 не тече текстом PG ─────────────────────
    let too_long = "X".repeat(300); // > varchar(255) у public.stores.name
    let (status, body, _) = req_full(
        &base,
        reqwest::Method::POST,
        "/api/v1/admin/stores",
        &token,
        Some(json!({ "name": too_long })),
    )
    .await;
    assert_eq!(
        status, 500,
        "переповнення varchar — помилка БД, а не валідації: {body}"
    );
    let text = body.to_string();
    for leak in [
        "value too long",
        "22001",
        "error returned from database",
        "character varying",
        "varchar",
        "ERROR:",
    ] {
        assert!(
            !text.contains(leak),
            "тіло не мусить містити сирий текст PG/драйвера ('{leak}'): {text}"
        );
    }

    // ── 5. Рубіжні шари ADR-0007 §11.8 не втручались ────────────────────────
    assert_eq!(
        torgashka_infrastructure::readonly_guard::hits(),
        funnel_before,
        "funnel_hits: жодного 25006 (запис ішов у справжній primary, не в репліку)"
    );
    assert_eq!(
        torgashka_infrastructure::readonly_guard::sanitized_hits(),
        sanitized_before,
        "sanitized_hits: сигнатура SQLx у тіло не потрапляла (AdminErr санує сам)"
    );

    eprintln!(
        "[primary_write] запис → PG підтверджено: stores.id={store_id}, \
         рядок у PG + owner-зв'язка + 22001 без витоку ✓"
    );
}
