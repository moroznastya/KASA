//! E2E: GET /api/v1/sync/master (ЕТАП 3 offline-first) — pull майстер-даних.
//!
//! Сценарії:
//!   1. master_returns_upsert_then_delete_deltas — зміна категорії → upsert,
//!      soft-delete → op=delete; версії монотонні, дельта не повторюється.
//!   2. master_paginates_over_500 — 520 змінених товарів → 2 сторінки
//!      (500 + 20), has_more на першій.
//!   3. master_rls_isolates_stores — категорія чужої точки не потрапляє
//!      в дельту каси; доступ до чужої точки через X-Store-Id → 403.
//!
//! Потребує доступної PostgreSQL (backend/.env) — як інші інтеграційні
//! тести крейта. Схема: Alembic head (0011 + 0012_server_version_columns).

use std::time::Duration;

use serde_json::{json, Value};
use sqlx::Row;
use torgashka_api::run_facade;
use uuid::Uuid;

/// Тестова точка (seed онбордингу).
const STORE1: &str = "d9be9608-c011-49be-b776-3317ca5e9af6";

fn api_url() -> Option<String> {
    let _ = torgashka_infrastructure::db::resolve_database_url()
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env");
    std::env::var("ONBOARDING_E2E_BASE").ok()
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

/// Спільна ініціалізація: seed (admin + точка) + login → (base, token, pool).
struct Ctx {
    base: String,
    token: String,
    pool: sqlx::PgPool,
    _handle: Option<tokio::task::JoinHandle<()>>,
}

impl Ctx {
    async fn new() -> Self {
        // Схема ДО старту фасаду: run_facade сам виконує ensure_schema у фоні —
        // паралельний ensure_schema (фасад + apply_schema) ловить гонку
        // CREATE EXTENSION pg_trgm (duplicate key). Один виклик на процес.
        apply_schema().await;
        let port = free_port().await;
        let addr = format!("127.0.0.1:{port}");
        let (base, _handle) = match api_url() {
            Some(b) => (b, None),
            None => {
                let h = run_facade(&addr);
                (format!("http://127.0.0.1:{port}"), Some(h))
            }
        };
        let pool = torgashka_infrastructure::db::connect_readonly_pool(2)
            .await
            .expect("pool");
        ensure_seed(&pool).await;

        // Логін з ретраєм (фасад стартує асинхронно).
        let client = reqwest::Client::new();
        let token = loop {
            if let Ok(r) = client
                .post(format!("{base}/api/v1/auth/login"))
                .json(&json!({"login": "admin", "password": "admin123"}))
                .send()
                .await
            {
                if r.status().is_success() {
                    let v: Value = r.json().await.expect("login json");
                    break v["access_token"].as_str().expect("access_token").to_string();
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        };
        Self { base, token, pool, _handle }
    }

    async fn pull(&self, entity: &str, since: i64, store_id: &str) -> Value {
        let resp = reqwest::Client::new()
            .get(format!(
                "{}/api/v1/sync/master?entity={entity}&since_version={since}",
                self.base
            ))
            .bearer_auth(&self.token)
            .header("X-Store-Id", store_id)
            .send()
            .await
            .expect("sync request");
        if !resp.status().is_success() {
            panic!("sync master {} since={since}: HTTP {}", entity, resp.status());
        }
        resp.json().await.expect("sync json")
    }
}

/// Застосувати схему БД (довідники + sync-таблиці + owner-DDL) ОДИН раз на
/// процес. ensure_schema ідемпотентний: порожня БД отримує повну схему
/// (SCHEMA_SQL), мігрована — лише додатки. Без цього ensure_seed падає на
/// порожній тестовій БД («relation "stores" does not exist»), бо ensure_schema
/// виконується фасадом лише при старті (run_facade).
/// Один виклик на процес (OnceCell) — паралельні тести не конкурують за
/// CREATE TABLE на fresh-БД.
#[path = "common/sync_schema.rs"]
mod sync_schema;

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
            // Sync-шар (Alembic 0011-0014): server_version, sync_meta,
            // soft-delete, client_uuid — ensure_schema (schema.sql) їх не має.
            sync_schema::apply(&p).await;
            p.close().await;
        })
        .await;
}

/// Seed адміна + тестової точки (idempotent — ON CONFLICT DO NOTHING).
async fn ensure_seed(pool: &sqlx::PgPool) {
    apply_schema().await;
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Sync Точка') ON CONFLICT (id) DO NOTHING",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .execute(pool)
    .await
    .expect("seed store");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'Seed Адмін', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind("$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e")
    .execute(pool)
    .await
    .expect("seed admin");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, s.id, 'owner', '{}'::jsonb, true, now()
         FROM users u, stores s
         WHERE u.login = 'admin' AND s.id = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .execute(pool)
    .await
    .expect("seed user_stores");
}

/// Знаходить change за id у відповіді pull.
fn find_change<'a>(delta: &'a Value, id: &str) -> Option<&'a Value> {
    delta["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .find(|c| c["id"].as_str() == Some(id))
}

// ─── Тест 1: upsert → delete дельти ─────────────────────────────────────────

mod common;

#[tokio::test]
async fn master_returns_upsert_then_delete_deltas() {
    common::force_test_db();
    let ctx = Ctx::new().await;
    let suffix = format!("{:08x}", Uuid::new_v4().as_u128() & 0xffff_ffff);
    let name_a = format!("__sync_e2e_A_{suffix}");
    let name_b = format!("__sync_e2e_B_{suffix}");
    let client = reqwest::Client::new();

    // 1. Створення двох категорій через API (Rust CRUD → store_id NULL → видимі всім).
    async fn create_category(
        client: &reqwest::Client,
        base: &str,
        token: &str,
        name: &str,
    ) -> String {
        let r = client
            .post(format!("{base}/api/v1/categories"))
            .bearer_auth(token)
            .header("X-Store-Id", STORE1)
            .json(&json!({"name": name}))
            .send()
            .await
            .expect("create category");
        assert!(r.status().is_success(), "create {name}: {}", r.status());
        let v: Value = r.json().await.expect("create json");
        v["id"].as_str().expect("category id").to_string()
    }
    let id_a = create_category(&client, &ctx.base, &ctx.token, &name_a).await;
    let id_b = create_category(&client, &ctx.base, &ctx.token, &name_b).await;

    // 2. Перший pull since=0: дельта містить обидві (upsert).
    let d1 = ctx.pull("categories", 0, STORE1).await;
    let ca = find_change(&d1, &id_a).expect("категорія A у першій дельті");
    assert_eq!(ca["op"], "upsert");
    assert_eq!(ca["data"]["name"], name_a);
    let cb = find_change(&d1, &id_b).expect("категорія B у першій дельті");
    assert_eq!(cb["op"], "upsert");
    let since1 = d1["to"].as_i64().expect("to");

    // 3. Зміна назви A (PUT) + soft-delete B (прямий SQL — Rust delete_category
    //    робить фізичне видалення, що суперечить дизайну 1.4; зафіксовано).
    let rename = format!("{name_a} (перейменовано)");
    let r = client
        .put(format!("{}/api/v1/categories/{id_a}", ctx.base))
        .bearer_auth(&ctx.token)
        .header("X-Store-Id", STORE1)
        .json(&json!({"name": rename}))
        .send()
        .await
        .expect("rename category");
    assert!(r.status().is_success(), "rename: {}", r.status());
    sqlx::query("UPDATE categories SET is_deleted = true, updated_at = now() WHERE id = $1")
        .bind(Uuid::parse_str(&id_b).unwrap())
        .execute(&ctx.pool)
        .await
        .expect("soft-delete B");

    // 4. Pull since=to1: upsert A (нова назва) + delete B.
    let d2 = ctx.pull("categories", since1, STORE1).await;
    let ca2 = find_change(&d2, &id_a).expect("оновлена A у другій дельті");
    assert_eq!(ca2["op"], "upsert");
    assert_eq!(ca2["data"]["name"], rename, "дельти несуть новий стан");
    let cb2 = find_change(&d2, &id_b).expect("видалена B у другій дельті");
    assert_eq!(cb2["op"], "delete");
    assert!(cb2["data"].is_null(), "delete data = null");
    let since2 = d2["to"].as_i64().expect("to2");
    assert!(since2 > since1, "версії монотонні: {since2} > {since1}");

    // 5. Стабільність: pull з since2 — порожня дельта (без повторів).
    let d3 = ctx.pull("categories", since2, STORE1).await;
    assert!(find_change(&d3, &id_a).is_none(), "A не повторюється");
    assert!(find_change(&d3, &id_b).is_none(), "B не повторюється");

    // Cleanup: фізичне видалення тестових категорій.
    sqlx::query("DELETE FROM categories WHERE id = ANY($1)")
        .bind(&[Uuid::parse_str(&id_a).unwrap(), Uuid::parse_str(&id_b).unwrap()][..])
        .execute(&ctx.pool)
        .await
        .expect("cleanup categories");
    if let Some(h) = ctx._handle {
        h.abort();
    }
}

// ─── Тест 2: пагінація > 500 ────────────────────────────────────────────────

#[tokio::test]
async fn master_paginates_over_500() {
    common::force_test_db();
    let ctx = Ctx::new().await;
    let suffix = format!("{:08x}", Uuid::new_v4().as_u128() & 0xffff_ffff);
    let prefix = format!("__sync_e2e_pag_{suffix}");
    let pool = ctx.pool.clone();

    // Поточний максимум версій products — pull лише НОВИХ змін.
    let max_before: Option<i64> =
        sqlx::query_scalar("SELECT max(server_version) FROM products")
            .fetch_one(&pool)
            .await
            .expect("max version");
    let since = max_before.unwrap_or(0);

    // 520 нових товарів (BEFORE-тригер проставляє унікальні server_version).
    let mut tx = pool.begin().await.expect("tx begin");
    for i in 0..520 {
        sqlx::query(
            "INSERT INTO products (id, title, price, created_at, updated_at)
             VALUES (gen_random_uuid(), $1, 10.00, now(), now())",
        )
        .bind(format!("{prefix}_{i:03}"))
        .execute(&mut *tx)
        .await
        .expect("insert product");
    }
    tx.commit().await.expect("tx commit");

    // 1. Перша сторінка: 500 змін, has_more=true.
    let p1 = ctx.pull("products", since, STORE1).await;
    assert_eq!(p1["changes"].as_array().map(|a| a.len()), Some(500), "сторінка = 500");
    assert_eq!(p1["has_more"], true, "є ще сторінки");
    let to1 = p1["to"].as_i64().expect("to1");

    // 2. Друга сторінка з since=to1: решта 20, has_more=false.
    let p2 = ctx.pull("products", to1, STORE1).await;
    assert_eq!(p2["changes"].as_array().map(|a| a.len()), Some(20), "друга сторінка = 20");
    assert_eq!(p2["has_more"], false);
    assert!(p2["to"].as_i64().expect("to2") > to1);

    // Cleanup.
    sqlx::query("DELETE FROM products WHERE title LIKE $1")
        .bind(format!("{prefix}%"))
        .execute(&pool)
        .await
        .expect("cleanup products");
    if let Some(h) = ctx._handle {
        h.abort();
    }
}

// ─── Тест 3: RLS / ізоляція точок ───────────────────────────────────────────

#[tokio::test]
async fn master_rls_isolates_stores() {
    common::force_test_db();
    let ctx = Ctx::new().await;
    let suffix = format!("{:08x}", Uuid::new_v4().as_u128() & 0xffff_ffff);
    let store2 = Uuid::new_v4();
    let cat_name = format!("__sync_e2e_foreign_{suffix}");

    // Чужа точка + її категорія (прямий SQL, store_id = store2).
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'E2E Чужа Точка')")
        .bind(store2)
        .execute(&ctx.pool)
        .await
        .expect("insert store2");
    let cat2 = sqlx::query(
        "INSERT INTO categories (id, name, store_id, created_at, updated_at)
         VALUES (gen_random_uuid(), $1, $2, now(), now())
         RETURNING id",
    )
    .bind(&cat_name)
    .bind(store2)
    .fetch_one(&ctx.pool)
    .await
    .expect("insert foreign category");
    let cat2_id: Uuid = cat2.get(0);

    // 1. Pull з точки STORE1 не містить категорію чужої точки.
    let d = ctx.pull("categories", 0, STORE1).await;
    assert!(
        find_change(&d, &cat2_id.to_string()).is_none(),
        "чужа категорія не має потрапити в дельту каси STORE1"
    );

    // 2. Доступ до чужої точки через X-Store-Id без user_stores → 403.
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/sync/master?entity=categories&since_version=0", ctx.base))
        .bearer_auth(&ctx.token)
        .header("X-Store-Id", store2.to_string())
        .send()
        .await
        .expect("sync foreign store");
    assert_eq!(resp.status(), 403, "немає доступу до чужої точки → 403");

    // Cleanup.
    sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(cat2_id)
        .execute(&ctx.pool)
        .await
        .expect("cleanup foreign category");
    sqlx::query("DELETE FROM stores WHERE id = $1")
        .bind(store2)
        .execute(&ctx.pool)
        .await
        .expect("cleanup store2");
    if let Some(h) = ctx._handle {
        h.abort();
    }
}
