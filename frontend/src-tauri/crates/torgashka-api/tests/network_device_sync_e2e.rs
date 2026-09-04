//! E2E: серверна device-авторизація sync-ендпоінтів (Частина 4).
//!
//! Каса синкає через device_token (Bearer, SHA-256 у БД) БЕЗ JWT касира і
//! БЕЗ X-Store-Id; сервер оновлює devices.last_seen_at + store_sync_state.
//! Старі каси (JWT касира + X-Store-Id) працюють без змін.
//!
//! Сценарії (один наскрізний тест, детермінований порядок):
//!   1. device master: GET /sync/master?entity=categories&since_version=0 з
//!      device_token без JWT/X-Store-Id → 200; дельта містить категорію
//!      точки A і НЕ містить категорію точки B (ізоляція);
//!   2. після успішного sync у БД: devices.last_seen_at IS NOT NULL і
//!      store_sync_state.status='ok' з правильним device_id
//!      (last_local_seq не чіпається);
//!   3. невалідний device_token → 401;
//!   4. block каси (admin) → 403 "Пристрій заблоковано"; unblock → 200;
//!   5. стара JWT-каса: sync/master з JWT касира + X-Store-Id A → 200;
//!   6. push smoke: POST /sync/push з device_token (порожній пакет) → 400
//!      «порожній пакет push» — авторизація device пройшла (не 401/403).
//!
//! БД: TEST_DATABASE_URL або робочий URL + _test (tests/common/mod.rs).
//! Самодостатній на ПОРОЖНІЙ БД: apply_schema застосовує ensure_schema
//! (users/stores/owner-DDL) + мінімальний sync-DDL 0011/0012 (sync_meta,
//! categories.is_deleted/server_version, bump-тригер) — на вже мігрованій
//! БД усі операції ідемпотентні (IF NOT EXISTS / OR REPLACE).

use std::time::Duration;

use serde_json::{json, Value};
use sqlx::Row;
use torgashka_api::run_facade;
use uuid::Uuid;

mod common;

/// Застосувати схему (users/stores + owner-DDL + sync-DDL 0011/0012) ОДИН
/// раз на процес — паралельні тести не конкурують за CREATE TABLE.
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
            // Мінімальний sync-шар (аналог Alembic 0011+0012): без нього
            // master падає на fresh-БД (немає sync_meta/server_version).
            sqlx::raw_sql(
                r#"
                CREATE TABLE IF NOT EXISTS public.sync_meta (
                    entity character varying(50) NOT NULL,
                    version bigint DEFAULT 0 NOT NULL,
                    CONSTRAINT sync_meta_pkey PRIMARY KEY (entity)
                );
                INSERT INTO public.sync_meta (entity) VALUES
                    ('categories'), ('products'), ('suppliers'),
                    ('employees'), ('settings'), ('stock_norms')
                ON CONFLICT (entity) DO NOTHING;

                ALTER TABLE public.categories ADD COLUMN IF NOT EXISTS
                    is_deleted boolean NOT NULL DEFAULT false;
                ALTER TABLE public.categories ADD COLUMN IF NOT EXISTS
                    server_version bigint NOT NULL DEFAULT 0;

                CREATE OR REPLACE FUNCTION public.bump_sync_version() RETURNS trigger AS $$
                DECLARE new_ver bigint;
                BEGIN
                    UPDATE public.sync_meta SET version = version + 1
                    WHERE entity = TG_ARGV[0] RETURNING version INTO new_ver;
                    IF TG_OP IN ('INSERT', 'UPDATE') THEN
                        NEW.server_version := new_ver;
                        RETURN NEW;
                    END IF;
                    RETURN OLD;
                END; $$ LANGUAGE plpgsql;

                DROP TRIGGER IF EXISTS trg_categories_bump ON public.categories;
                CREATE TRIGGER trg_categories_bump
                BEFORE INSERT OR UPDATE OR DELETE ON public.categories
                FOR EACH ROW EXECUTE FUNCTION public.bump_sync_version('categories');
                "#,
            )
            .execute(&p)
            .await
            .expect("sync-DDL на тестовій БД");
            p.close().await;
        })
        .await;
}

const XFF: &str = "198.51.100.10";

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

/// POST /api/v1/admin/stores/:id/activation-code → (status, тіло).
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

/// POST /api/v1/devices/activate (публічний) → (status, тіло).
async fn activate(base: &str, code: &str) -> (u16, Value) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/devices/activate"))
        .header("x-forwarded-for", XFF)
        .json(&json!({"code": code, "device_fingerprint": "DEV-SYNC-E2E-7F3A9C"}))
        .send()
        .await
        .expect("activate запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// GET /sync/master?entity=categories з device_token (БЕЗ X-Store-Id).
async fn device_master(base: &str, token: &str, since: i64) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .get(format!(
            "{base}/api/v1/sync/master?entity=categories&since_version={since}"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("device master запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// GET /sync/master з JWT касира + X-Store-Id (стара каса).
async fn cashier_master(base: &str, token: &str, store_id: Uuid, since: i64) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .get(format!(
            "{base}/api/v1/sync/master?entity=categories&since_version={since}"
        ))
        .bearer_auth(token)
        .header("X-Store-Id", store_id.to_string())
        .send()
        .await
        .expect("cashier master запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

fn find_change<'a>(delta: &'a Value, id: &str) -> Option<&'a Value> {
    delta["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .find(|c| c["id"].as_str() == Some(id))
}

// ─────────────────────────────────────────────────────────────────────────────
// Основний сценарій: device-авторизація master/push + стан у БД + legacy JWT
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn device_sync_master_auth_isolates_stores_and_tracks_state() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;

    // ── Seed: точки A і B + власник (обидві) + касир (тільки A) ──────────
    let store_a = Uuid::new_v4();
    let store_b = Uuid::new_v4();
    let pwd = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e"; // admin123
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'DevSync A'), ($2, 'DevSync B') ON CONFLICT (id) DO NOTHING")
        .bind(store_a).bind(store_b)
        .execute(&pool).await.expect("seed stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'DevSync Owner', 'nsync_owner', $2, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4()).bind(pwd)
    .execute(&pool).await.expect("seed owner");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'DevSync Cashier', 'nsync_cashier', $2, 'cashier'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4()).bind(pwd)
    .execute(&pool).await.expect("seed cashier");
    // owner → A і B; cashier → тільки A.
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, s.id, u.role::text, '{}'::jsonb, true, now()
         FROM users u, stores s
         WHERE u.login = 'nsync_owner' AND s.id IN ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(store_a).bind(store_b)
    .execute(&pool).await.expect("seed owner user_stores");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, s.id, u.role::text, '{}'::jsonb, true, now()
         FROM users u, stores s
         WHERE u.login = 'nsync_cashier' AND s.id = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(store_a)
    .execute(&pool).await.expect("seed cashier user_stores");

    // ── Категорії точок (server_version проставляє bump-тригер) ──────────
    let cat_a = Uuid::new_v4();
    let cat_b = Uuid::new_v4();
    let suffix = format!("{:08x}", Uuid::new_v4().as_u128() & 0xffff_ffff);
    sqlx::query("INSERT INTO categories (id, name, store_id) VALUES ($1, $2, $3)")
        .bind(cat_a)
        .bind(format!("__devsync_A_{suffix}"))
        .bind(store_a)
        .execute(&pool).await.expect("seed category A");
    sqlx::query("INSERT INTO categories (id, name, store_id) VALUES ($1, $2, $3)")
        .bind(cat_b)
        .bind(format!("__devsync_B_{suffix}"))
        .bind(store_b)
        .execute(&pool).await.expect("seed category B");

    // ── Фасад + активація каси точки A ─────────────────────────────────────
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner = login(&base, "nsync_owner").await;

    let (cs, code_body) = gen_code(&base, &owner, store_a).await;
    assert_eq!(cs, 200, "код точки A: {code_body}");
    let code = code_body["code"].as_str().expect("code").to_string();

    let (as_, act) = activate(&base, &code).await;
    assert_eq!(as_, 200, "активація каси A: {act}");
    let device_token = act["device_token"].as_str().expect("device_token").to_string();
    let device_id = Uuid::parse_str(act["device_id"].as_str().expect("device_id"))
        .expect("device_id uuid");
    assert_eq!(
        Uuid::parse_str(act["store_id"].as_str().unwrap()).unwrap(),
        store_a,
        "каса прив'язана до точки A"
    );

    // ── 1. Device master: без JWT і без X-Store-Id → 200, ізоляція ────────
    let (s1, d1) = device_master(&base, &device_token, 0).await;
    assert_eq!(s1, 200, "device master: {d1}");
    let ca = find_change(&d1, &cat_a.to_string()).expect("категорія A у дельті");
    assert_eq!(ca["op"], "upsert");
    assert_eq!(ca["data"]["name"], format!("__devsync_A_{suffix}"));
    assert!(
        find_change(&d1, &cat_b.to_string()).is_none(),
        "категорія B чужої точки НЕ потрапляє в дельту A: {d1}"
    );

    // ── 2. Стан у БД: last_seen_at + store_sync_state ──────────────────────
    let row = sqlx::query("SELECT last_seen_at FROM devices WHERE id = $1")
        .bind(device_id)
        .fetch_one(&pool)
        .await
        .expect("device у БД");
    let last_seen: Option<chrono::NaiveDateTime> = row.get("last_seen_at");
    assert!(last_seen.is_some(), "last_seen_at оновлено після sync");

    let row = sqlx::query(
        "SELECT device_id, status, last_synced_at, last_local_seq \
         FROM store_sync_state WHERE store_id = $1",
    )
    .bind(store_a)
    .fetch_one(&pool)
    .await
    .expect("store_sync_state точки A");
    let s_device: Option<Uuid> = row.get("device_id");
    let s_status: String = row.get("status");
    let s_synced: Option<chrono::NaiveDateTime> = row.get("last_synced_at");
    let s_seq: i64 = row.get("last_local_seq");
    assert_eq!(s_device, Some(device_id), "device_id у store_sync_state");
    assert_eq!(s_status, "ok");
    assert!(s_synced.is_some(), "last_synced_at заповнено");
    assert_eq!(s_seq, 0, "last_local_seq не чіпаємо");

    // ── 3. Невалідний device_token → 401 ────────────────────────────────────
    let (s_bad, bad_body) = device_master(&base, "f".repeat(48).as_str(), 0).await;
    assert_eq!(s_bad, 401, "невалідний device_token: {bad_body}");

    // ── 4. block → 403; unblock → 200 ───────────────────────────────────────
    let client = reqwest::Client::new();
    let rb = client
        .post(format!("{base}/api/v1/admin/devices/{device_id}/block"))
        .bearer_auth(&owner)
        .send()
        .await
        .expect("block запит");
    assert!(rb.status().is_success(), "block: {}", rb.status());
    let (s_blk, blk_body) = device_master(&base, &device_token, 0).await;
    assert_eq!(s_blk, 403, "заблокована каса: {blk_body}");
    assert_eq!(blk_body["detail"], "Пристрій заблоковано або видалено");

    let ru = client
        .post(format!("{base}/api/v1/admin/devices/{device_id}/unblock"))
        .bearer_auth(&owner)
        .send()
        .await
        .expect("unblock запит");
    assert!(ru.status().is_success(), "unblock: {}", ru.status());
    let (s_ub, ub_body) = device_master(&base, &device_token, 0).await;
    assert_eq!(s_ub, 200, "розблокована каса: {ub_body}");

    // ── 5. Стара JWT-каса (cashier + X-Store-Id A) → 200 ────────────────────
    let cashier = login(&base, "nsync_cashier").await;
    let (s_leg, leg_body) = cashier_master(&base, &cashier, store_a, 0).await;
    assert_eq!(s_leg, 200, "legacy JWT-каса: {leg_body}");
    assert!(
        find_change(&leg_body, &cat_a.to_string()).is_some(),
        "JWT-каса бачить категорію A"
    );

    // ── 6. Push smoke: device_token проходить авторизацію (порожній пакет
    //       → 400 «порожній пакет push», а не 401/403) ─────────────────────
    let rp = client
        .post(format!("{base}/api/v1/sync/push"))
        .bearer_auth(&device_token)
        .json(&json!([]))
        .send()
        .await
        .expect("push smoke запит");
    let sp = rp.status().as_u16();
    assert_eq!(sp, 400, "device auth на push пропустив до валідації тіла");
    let push_body: Value = rp.json().await.unwrap_or(Value::Null);
    assert!(
        push_body["detail"]
            .as_str()
            .map(|d| d.contains("порожній пакет"))
            .unwrap_or(false),
        "деталь push: {push_body}"
    );
}
