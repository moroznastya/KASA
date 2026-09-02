//! E2E: pull-клієнт каси (ЕТАП 3 offline-first) з РЕАЛЬНИМ ендпоінтом
//! GET /api/v1/sync/master (torgashka-api фасад + PostgreSQL).
//!
//! Покриває:
//!   * початковий pull порожньої каси (онбординг) — усі сутності в порядку
//!     дизайну (розділ 5); products може зайняти кілька сторінок — очікувано;
//!   * застосування дельти в SQLite master-таблиці (0003) + sync_meta;
//!   * ідемпотентність: повторний pull не дублює рядки;
//!   * op=delete (soft-delete на сервері) → is_deleted=1 локально.

use std::time::Duration;

use serde_json::{json, Value};
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_pull::{open_connection, pull_all, PullConfig};
use uuid::Uuid;

const STORE1: &str = "d9be9608-c011-49be-b776-3317ca5e9af6";

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

async fn ensure_seed(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Pull Точка') ON CONFLICT (id) DO NOTHING",
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

mod common;

#[tokio::test]
async fn pull_client_initial_and_repeat_sync() {
    common::force_test_db();
    // ── Фасад + seed + login ──────────────────────────────────────────────
    let _ = torgashka_infrastructure::db::resolve_database_url()
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env");
    let port = free_port().await;
    let handle = run_facade(&format!("127.0.0.1:{port}"));
    let base = format!("http://127.0.0.1:{port}");
    let pool = torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool");
    ensure_seed(&pool).await;

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

    // ── Тестові дані на сервері ──────────────────────────────────────────
    let suffix = format!("{:08x}", Uuid::new_v4().as_u128() & 0xffff_ffff);
    let cat_name = format!("__sync_pull_cat_{suffix}");
    let prod_title = format!("__sync_pull_prod_{suffix}");
    let supp_name = format!("__sync_pull_supp_{suffix}");

    // Категорія через API (store_id NULL — спільна, видима всім точкам).
    let r = client
        .post(format!("{base}/api/v1/categories"))
        .bearer_auth(&token)
        .header("X-Store-Id", STORE1)
        .json(&json!({"name": cat_name}))
        .send()
        .await
        .expect("create category");
    assert!(r.status().is_success(), "create category: {}", r.status());
    let created: Value = r.json().await.expect("create json");
    let cat_id: Uuid = created["id"]
        .as_str()
        .expect("category id")
        .parse()
        .expect("uuid");

    // Товар + постачальник напряму (SQL).
    sqlx::query(
        "INSERT INTO products (id, title, price, created_at, updated_at)
         VALUES (gen_random_uuid(), $1, 99.90, now(), now()) RETURNING id",
    )
    .bind(&prod_title)
    .fetch_one(&pool)
    .await
    .expect("insert product");
    sqlx::query(
        "INSERT INTO suppliers (id, name, phone, created_at, updated_at)
         VALUES (gen_random_uuid(), $1, '+380000000000', now(), now())",
    )
    .bind(&supp_name)
    .execute(&pool)
    .await
    .expect("insert supplier");

    // ── SQLite каса (порожня, міграції до 0003) ───────────────────────────
    let tmp = tempfile::NamedTempFile::new().expect("temp db");
    let db_path = tmp.path().to_path_buf();
    let cfg = PullConfig {
        base_url: base.clone(),
        token: token.clone(),
        store_id: STORE1.to_string(),
        interval_secs: 30,
        db_path: db_path.clone(),
    };

    // ── 1. Початковий pull (онбординг порожньої каси) ─────────────────────
    let ok = pull_all(&db_path, &client, &cfg)
        .await
        .expect("перший цикл pull");
    assert_eq!(ok, 6, "усі 6 сутностей успішно оновлені");

    let conn = open_connection(&db_path).expect("open db");
    let version: i64 = conn
        .query_row(
            "SELECT version FROM sync_meta WHERE entity = 'categories'",
            [],
            |r| r.get(0),
        )
        .expect("sync_meta categories");
    assert!(version > 0, "categories since_version просунулась: {version}");

    // Категорія з'явилась локально (із серверним id).
    let row: (String, i64) = conn
        .query_row(
            "SELECT name, is_deleted FROM categories WHERE id = ?1",
            rusqlite::params![cat_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("локальна категорія");
    assert_eq!(row.0, cat_name);
    assert_eq!(row.1, 0, "нова категорія не видалена");

    // Товар у products_v2.
    let prod_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM products_v2 WHERE name = ?1",
            rusqlite::params![prod_title],
            |r| r.get(0),
        )
        .expect("локальний товар");
    assert_eq!(prod_count, 1, "товар сервера застосований у products_v2");
    let prod_deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM products_v2 WHERE name = ?1",
            rusqlite::params![prod_title],
            |r| r.get(0),
        )
        .expect("is_deleted товару");
    assert_eq!(prod_deleted, 0);

    // Постачальник.
    let supp_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM suppliers WHERE name = ?1",
            rusqlite::params![supp_name],
            |r| r.get(0),
        )
        .expect("локальний постачальник");
    assert_eq!(supp_count, 1);
    drop(conn);

    // ── 2. Повторний pull — ідемпотентний (без дублікатів) ────────────────
    pull_all(&db_path, &client, &cfg)
        .await
        .expect("повторний цикл pull");
    let conn = open_connection(&db_path).expect("open db 2");
    let cat_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM categories WHERE id = ?1",
            rusqlite::params![cat_id.to_string()],
            |r| r.get(0),
        )
        .expect("count категорий");
    assert_eq!(cat_count, 1, "повторний pull не дублює категорію");
    let prod_count2: i64 = conn
        .query_row(
            "SELECT count(*) FROM products_v2 WHERE name = ?1",
            rusqlite::params![prod_title],
            |r| r.get(0),
        )
        .expect("count товарів");
    assert_eq!(prod_count2, 1, "повторний pull не дублює товар");
    drop(conn);

    // ── 3. Soft-delete на сервері → op=delete → is_deleted=1 локально ─────
    sqlx::query("UPDATE categories SET is_deleted = true, updated_at = now() WHERE id = $1")
        .bind(cat_id)
        .execute(&pool)
        .await
        .expect("soft-delete категорії на сервері");
    pull_all(&db_path, &client, &cfg)
        .await
        .expect("pull після soft-delete");

    let conn = open_connection(&db_path).expect("open db 3");
    let deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM categories WHERE id = ?1",
            rusqlite::params![cat_id.to_string()],
            |r| r.get(0),
        )
        .expect("is_deleted локально");
    assert_eq!(deleted, 1, "soft-delete сервера → is_deleted=1 на касі");
    drop(conn);

    // ── Cleanup ────────────────────────────────────────────────────────────
    sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(cat_id)
        .execute(&pool)
        .await
        .expect("cleanup category");
    sqlx::query("DELETE FROM products WHERE title = $1")
        .bind(&prod_title)
        .execute(&pool)
        .await
        .expect("cleanup product");
    sqlx::query("DELETE FROM suppliers WHERE name = $1")
        .bind(&supp_name)
        .execute(&pool)
        .await
        .expect("cleanup supplier");
    handle.abort();
}
