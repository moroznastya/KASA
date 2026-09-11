//! E2E: PUT /api/v1/users/{id} з {"onboarding_completed": false} —
//! реальний HTTP-запит до Rust-фасаду + перевірка збереження у БД (SELECT).
//! Потребує доступної PostgreSQL (як інші інтеграційні тести крейта).

use std::time::Duration;

use serde_json::json;
use torgashka_api::run_facade;
use uuid::Uuid;

fn api_url() -> Option<String> {
    // Резолв DATABASE_URL заздалегідь — щоб фасад стартував з тим самим env.
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

/// Створює seed для чистої БД (CI): admin/admin123 + тестова точка продажу.
/// ON CONFLICT DO NOTHING — на робочій БД з наявним seed нічого не ламає.
async fn ensure_seed(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Онбординг Точка') ON CONFLICT (id) DO NOTHING",
    )
    .bind(Uuid::parse_str("d9be9608-c011-49be-b776-3317ca5e9af6").unwrap())
    .execute(pool)
    .await
    .expect("INSERT тестовий store");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'Seed Адмін', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind("$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e")
    .execute(pool)
    .await
    .expect("INSERT seed admin");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, s.id, 'owner', '{}'::jsonb, true, now()
         FROM users u, stores s
         WHERE u.login = 'admin' AND s.id = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(Uuid::parse_str("d9be9608-c011-49be-b776-3317ca5e9af6").unwrap())
    .execute(pool)
    .await
    .expect("INSERT seed user_stores");
}

async fn login(client: &reqwest::Client, base: &str) -> serde_json::Value {
    for _ in 0..50 {
        if let Ok(r) = client
            .post(format!("{base}/api/v1/auth/login"))
            .json(&json!({"login": "admin", "password": "admin123"}))
            .send()
            .await
        {
            if r.status().is_success() {
                return r.json().await.expect("login json");
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("фасад не піднявся за 10с або логін admin/admin123 невалідний");
}

mod common;

#[tokio::test]
async fn put_onboarding_completed_persists_in_db() {
    common::force_test_db();
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    // Зовнішній фасад — лише якщо ONBOARDING_E2E_BASE задано явно.
    let (base, _handle) = match api_url() {
        Some(b) => (b, None),
        None => {
            let h = run_facade(&addr);
            (format!("http://127.0.0.1:{port}"), Some(h))
        }
    };

    let client = reqwest::Client::new();
    // Seed (admin + store) потрібен ДО login — фасад логіниться відразу.
    let pool = torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool");
    ensure_seed(&pool).await;
    let login = login(&client, &base).await;
    let token = login["access_token"].as_str().expect("access_token");
    let user_id: uuid::Uuid = login["user"]["id"]
        .as_str()
        .expect("user.id")
        .parse()
        .expect("user.id uuid");

    // 1. Поточне значення у БД.
    let before: bool = sqlx::query_scalar("SELECT onboarding_completed FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("SELECT before");

    // 2. PUT onboarding_completed = !before (перемикаємо, щоб побачити реальну зміну).
    let want = !before;
    let resp = client
        .put(format!("{base}/api/v1/users/{user_id}"))
        .bearer_auth(token)
        .header("X-Store-Id", "d9be9608-c011-49be-b776-3317ca5e9af6")
        .json(&json!({"onboarding_completed": want}))
        .send()
        .await
        .expect("PUT send");
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        panic!("PUT status: {status} body: {body}");
    }
    let dto: serde_json::Value = resp.json().await.expect("PUT json");

    // 3. UserDto містить поле зі значенням.
    assert_eq!(
        dto["onboarding_completed"].as_bool(),
        Some(want),
        "UserDto має містити onboarding_completed={want}: {dto}"
    );

    // 4. Значення реально збережене в БД.
    let after: bool = sqlx::query_scalar("SELECT onboarding_completed FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("SELECT after");
    assert_eq!(after, want, "БД має зберігати onboarding_completed={want}");

    // 5. Повертаємо початковий стан (тест не має залишати слідів).
    let restore = client
        .put(format!("{base}/api/v1/users/{user_id}"))
        .bearer_auth(token)
        .header("X-Store-Id", "d9be9608-c011-49be-b776-3317ca5e9af6")
        .json(&json!({"onboarding_completed": before}))
        .send()
        .await
        .expect("PUT restore");
    assert!(
        restore.status().is_success(),
        "restore status: {}",
        restore.status()
    );
    let back: bool = sqlx::query_scalar("SELECT onboarding_completed FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("SELECT restore");
    assert_eq!(back, before, "стан відновлено");

    if let Some(h) = _handle {
        h.abort();
    }
}
