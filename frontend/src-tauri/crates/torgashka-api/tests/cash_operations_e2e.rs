//! E2E: готівкові операції (внесення/інкасація) через реальний HTTP-фасад.
//! POST /api/v1/cash-operations (201), GET (200 + balance), валідація (422).
//! Потребує доступної PostgreSQL (як onboarding_e2e); прибирає за собою:
//! тимчасовий адмін-користувач, його user_stores-доступ і тестові операції.

use std::time::Duration;

use serde_json::json;
use torgashka_api::run_facade;
use uuid::Uuid;

/// «Білий магазин» (pos_system_fresh).
const STORE_ID: &str = "65d5db51-672f-4a38-9c1e-f36c5feb5374";
const TEST_MARK: &str = "__test_cash_e2e__";
const TEST_LOGIN: &str = "cash_e2e_admin";
const CASHIER_LOGIN: &str = "cash_e2e_cashier";
/// bcrypt("admin123") — згенеровано заздалегідь (детермінована перевірка).
const TEST_PWD_HASH: &str = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e";

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

async fn login(client: &reqwest::Client, base: &str, login_name: &str) -> serde_json::Value {
    for _ in 0..60 {
        if let Ok(r) = client
            .post(format!("{base}/api/v1/auth/login"))
            .json(&json!({"login": login_name, "password": "admin123"}))
            .send()
            .await
        {
            if r.status().is_success() {
                return r.json().await.expect("login json");
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("фасад не піднявся за 12с або логін {login_name}/admin123 невалідний");
}

/// Створює тимчасового користувача (admin|cashier) + доступ до точки.
async fn setup_test_user(pool: &sqlx::PgPool, login_name: &str, role: &str) -> Uuid {
    let _ = sqlx::query("DELETE FROM users WHERE login = $1")
        .bind(login_name)
        .execute(pool)
        .await;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Тест', $2, $3, $4::public.user_role, true, now(), now(), true)",
    )
    .bind(id)
    .bind(login_name)
    .bind(TEST_PWD_HASH)
    .bind(role)
    .execute(pool)
    .await
    .expect("INSERT тестовий користувач");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         VALUES ($1, $2, $3, '{}'::jsonb, false, now())",
    )
    .bind(id)
    .bind(Uuid::parse_str(STORE_ID).unwrap())
    .bind(role)
    .execute(pool)
    .await
    .expect("INSERT user_stores");
    id
}

#[tokio::test]
async fn cash_operations_http_roundtrip() {
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let handle = run_facade(&addr);
    let base = format!("http://127.0.0.1:{port}");

    let pool = torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool");
    // Прибирання залишків попередніх запусків (тест падав у різних місцях).
    sqlx::query("DELETE FROM cash_operations WHERE comment LIKE $1")
        .bind(format!("{TEST_MARK}%"))
        .execute(&pool)
        .await
        .expect("start cleanup ops");
    let _ = sqlx::query(
        "DELETE FROM user_stores WHERE user_id IN (SELECT id FROM users WHERE login IN ($1, $2))",
    )
    .bind(TEST_LOGIN)
    .bind(CASHIER_LOGIN)
    .execute(&pool)
    .await;
    let _ = sqlx::query("DELETE FROM users WHERE login IN ($1, $2)")
        .bind(TEST_LOGIN)
        .bind(CASHIER_LOGIN)
        .execute(&pool)
        .await;
    let admin_id = setup_test_user(&pool, TEST_LOGIN, "admin").await;

    let client = reqwest::Client::new();
    let auth = login(&client, &base, TEST_LOGIN).await;
    let token = auth["access_token"].as_str().expect("access_token");

    // 1. POST deposit → 201.
    let resp = client
        .post(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(token)
        .header("X-Store-Id", STORE_ID)
        .json(&json!({"operation_type": "deposit", "cash_type": "cash", "amount": 500.00, "comment": format!("{TEST_MARK} розмін")}))
        .send()
        .await
        .expect("POST deposit");
    assert_eq!(resp.status(), 201, "deposit має бути 201: {}", resp.text().await.unwrap_or_default());
    let dto: serde_json::Value = resp.json().await.expect("deposit json");
    assert_eq!(dto["operation_type"], "deposit");
    assert_eq!(dto["cash_type"], "cash", "cash_type з JSON: {dto}");
    assert_eq!(dto["amount"], "500.00", "amount зберігає scale колонки: {dto}");
    assert_eq!(dto["user_name"], "E2E Тест");
    assert_eq!(dto["store_id"], STORE_ID);
    assert_eq!(dto["user_id"].as_str().map(|s| s.to_lowercase()), Some(admin_id.to_string()), "user_id з JWT: {dto}");

    // 2. POST collection → 201.
    let resp = client
        .post(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(token)
        .header("X-Store-Id", STORE_ID)
        .json(&json!({"operation_type": "collection", "cash_type": "card", "amount": 200.00, "comment": format!("{TEST_MARK} інкасація")}))
        .send()
        .await
        .expect("POST collection");
    assert_eq!(resp.status(), 201, "collection має бути 201");
    let dto: serde_json::Value = resp.json().await.expect("collection json");
    assert_eq!(dto["operation_type"], "collection");
    assert_eq!(dto["cash_type"], "card");
    assert_eq!(dto["amount"], "200.00");

    // 2b. POST deposit card → 201 (безготівкова каса).
    let resp = client
        .post(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(token)
        .header("X-Store-Id", STORE_ID)
        .json(&json!({"operation_type": "deposit", "cash_type": "card", "amount": 150.50, "comment": format!("{TEST_MARK} оплата карткою")}))
        .send()
        .await
        .expect("POST deposit card");
    assert_eq!(resp.status(), 201, "deposit card має бути 201");
    let dto: serde_json::Value = resp.json().await.expect("deposit card json");
    assert_eq!(dto["cash_type"], "card");
    assert_eq!(dto["amount"], "150.50");

    // 3. GET → 200: список + окремі баланси cash/card.
    //    cash: 500.00 deposit − 0 = 500.00; card: 150.50 − 200.00 = −49.50.
    let resp = client
        .get(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(token)
        .header("X-Store-Id", STORE_ID)
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 200, "GET має бути 200");
    let list: serde_json::Value = resp.json().await.expect("list json");
    assert_eq!(list["balances"]["cash"], "500.00", "баланс готівкової каси: {list}");
    assert_eq!(list["balances"]["card"], "-49.50", "баланс безготівкової каси: {list}");
    let ops = list["operations"].as_array().expect("operations масив");
    assert!(ops.len() >= 2, "має бути ≥2 операцій: {list}");
    assert!(ops.iter().all(|o| o["user_name"] == "E2E Тест"));

    // 4. Валідація: від'ємна сума → 422.
    let resp = client
        .post(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(token)
        .header("X-Store-Id", STORE_ID)
        .json(&json!({"operation_type": "deposit", "cash_type": "cash", "amount": -5}))
        .send()
        .await
        .expect("POST negative");
    assert_eq!(resp.status(), 422, "від'ємна сума має бути 422");

    // 5. Валідація: невірний тип → 422.
    let resp = client
        .post(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(token)
        .header("X-Store-Id", STORE_ID)
        .json(&json!({"operation_type": "transfer", "cash_type": "cash", "amount": 100}))
        .send()
        .await
        .expect("POST wrong type");
    assert_eq!(resp.status(), 422, "невірний тип має бути 422");

    // 5b. Валідація: невірний cash_type → 422.
    let resp = client
        .post(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(token)
        .header("X-Store-Id", STORE_ID)
        .json(&json!({"operation_type": "deposit", "cash_type": "crypto", "amount": 100}))
        .send()
        .await
        .expect("POST bad cash_type");
    assert_eq!(resp.status(), 422, "невірний cash_type має бути 422");

    // 6. Касир (role=cashier) → 403 (require_admin: тільки admin|owner).
    let cashier_id = setup_test_user(&pool, CASHIER_LOGIN, "cashier").await;
    let cashier_auth = login(&client, &base, CASHIER_LOGIN).await;
    let cashier_token = cashier_auth["access_token"].as_str().expect("cashier token");
    let resp = client
        .post(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(cashier_token)
        .header("X-Store-Id", STORE_ID)
        .json(&json!({"operation_type": "deposit", "cash_type": "cash", "amount": 100.00}))
        .send()
        .await
        .expect("POST cashier");
    assert_eq!(resp.status(), 403, "касир має отримати 403: {}", resp.text().await.unwrap_or_default());
    let resp = client
        .get(format!("{base}/api/v1/cash-operations"))
        .bearer_auth(cashier_token)
        .header("X-Store-Id", STORE_ID)
        .send()
        .await
        .expect("GET cashier");
    assert_eq!(resp.status(), 403, "касир GET має отримати 403");

    // 7. Прибирання: операції, доступ, тестові користувачі.
    sqlx::query("DELETE FROM cash_operations WHERE comment LIKE $1")
        .bind(format!("{TEST_MARK}%"))
        .execute(&pool)
        .await
        .expect("cleanup ops");
    sqlx::query("DELETE FROM user_stores WHERE user_id = $1")
        .bind(admin_id)
        .execute(&pool)
        .await
        .expect("cleanup user_stores");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(admin_id)
        .execute(&pool)
        .await
        .expect("cleanup user");
    sqlx::query("DELETE FROM user_stores WHERE user_id = $1")
        .bind(cashier_id)
        .execute(&pool)
        .await
        .expect("cleanup cashier user_stores");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(cashier_id)
        .execute(&pool)
        .await
        .expect("cleanup cashier user");

    handle.abort();
}
