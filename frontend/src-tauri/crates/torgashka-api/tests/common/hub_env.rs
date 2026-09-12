//! Спільний каркас e2e «вузол ↔ хаб» (ADR-0008, етапи E3/E4).
//!
//! Модель ADR-0008 вимагає ДВОХ інстансів однієї системи: вузол точки
//! (приймає документи кас локально) і хаб мережі (приймає від вузлів). Тому
//! e2e не може обмежитись одним `run_facade` на спільній тестовій БД — тест
//! мусить мати ДВІ БД і ДВА фасади:
//!
//!   * ХАБ — звичайна тестова БД (`common::force_test_db`, env `DATABASE_URL`);
//!   * ВУЗОЛ — окрема БД у тому ж кластері, `<тестова>_node_<тег>` (ім'я
//!     мусить містити `test` — запобіжник проти робочої БД), з тим самим
//!     шаром схеми (`ensure_schema` + `sync_schema::apply`).
//!
//! `run_facade` тут не підходить: він резолвить БД із env ПРОЦЕСУ, а тесту
//! потрібні дві різні БД в одному процесі. Тому фасади будуються явно —
//! `router_v1::build_router(AppState)` + `axum::serve` на власному порту з
//! ЯВНИМ пулом. Це той самий роутер, що в проді (жодного тестового дубля).
//!
//! Роль інстанса («вузол із хабом» vs «сам хаб») задається налаштуванням
//! `system_settings.sync.hub_url` у ВЛАСНІЙ БД — саме так, як у проді
//! (`hub_forwarder::HubForwardConfig::from_pool`), тому тест перевіряє
//! справжній поділ ролей, а не тестовий прапорець.

#![allow(dead_code)] // кожен e2e бере свою підмножину хелперів

use std::sync::Arc;

use axum::Router;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use torgashka_api::auth::create_access_token;
use torgashka_api::{router_v1, AppState};
use torgashka_infrastructure::node_config::NodeConfig;
use torgashka_infrastructure::repositories::auth::SqlxAuth;
use torgashka_infrastructure::repositories::directories::SqlxDirectories;
use torgashka_infrastructure::repositories::pos::SqlxPos;
use torgashka_infrastructure::repositories::setup::SqlxSetupService;
use torgashka_infrastructure::store_ctx::StorePool;
use uuid::Uuid;

#[path = "sync_schema.rs"]
mod sync_schema;

/// Спільний JWT-секрет обох фасадів (у проді — той самий SECRET_KEY мережі).
pub const SECRET: &str = "hub-forward-e2e-secret";

// ─────────────────────────────────────────────────────────────────────────────
// БД
// ─────────────────────────────────────────────────────────────────────────────

/// URL тестової БД (після `common::force_test_db()`).
pub fn test_db_url() -> String {
    std::env::var("DATABASE_URL").expect("force_test_db() мусить бути викликаний першим")
}

fn db_name(url: &str) -> String {
    let before = url.split('?').next().unwrap_or(url);
    before[before.rfind('/').expect("слеш у URL") + 1..].to_string()
}

/// URL БД вузла: `<тестова>_node_<тег>` (тег — щоб тести не ділили БД).
pub fn node_db_url(tag: &str) -> String {
    let url = test_db_url();
    let idx = url.rfind('/').expect("слеш у URL");
    let name = db_name(&url);
    let node_name = format!("{name}_node_{tag}");
    assert!(
        node_name.contains("test"),
        "БД вузла мусить мати 'test' в імені (запобіжник), маємо '{node_name}'"
    );
    format!("{}{node_name}", &url[..=idx])
}

/// Пул до довільної БД кластера (без `_test`-суфіксації).
pub async fn pool_to(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(url)
        .await
        .unwrap_or_else(|e| panic!("пул до {url}: {e}"))
}

/// Створити БД, якщо її немає (ідемпотентно; CREATE DATABASE — поза транзакцією).
pub async fn ensure_database(admin: &PgPool, name: &str) {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(name)
            .fetch_one(admin)
            .await
            .expect("pg_database");
    if exists {
        return;
    }
    // Гонка (паралельний прогін/повторний запуск) — БД уже створено: це не
    // помилка, далі все ідемпотентно (ensure_schema/sync_schema/seed).
    if let Err(e) = sqlx::query(&format!("CREATE DATABASE \"{name}\""))
        .execute(admin)
        .await
    {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
                .bind(name)
                .fetch_one(admin)
                .await
                .expect("pg_database");
        assert!(exists, "CREATE DATABASE {name}: {e}");
    }
}

/// Повний шар схеми (базова + sync-шар) на вказаній БД — ідемпотентно.
pub async fn ensure_schema_on(pool: &PgPool) {
    torgashka_infrastructure::db::ensure_schema(pool)
        .await
        .expect("ensure_schema");
    sync_schema::apply(pool).await;
}

/// Пул ХАБА: ВЛАСНА БД (`<тестова>_node_hub`), а не спільна `*_test`.
///
/// Навіщо: e2e хабa сіє точку/власника/товари — якщо робити це у СПІЛЬНІЙ
/// тестовій БД, суіти, що очікують порожню мережу (напр.
/// `admin_migrate_legacy_e2e`: «жодної точки — одиночна інсталяція»), ламаються
/// від наших рядків. Власна БД хаба = повна ізоляція e2e (і жодного впливу на
/// чужі тести).
/// `tag` — щоб КОЖЕН тест мав власний хаб (тести одного бінаря виконуються
/// паралельно за замовчуванням: спільна БД хаба = гонка на `DELETE`/`COUNT`).
pub async fn hub_pool(admin: &PgPool, tag: &str) -> PgPool {
    node_pool(admin, &format!("hub_{tag}")).await
}

/// Пул вузла: БД створена (якщо треба) і має повний шар схеми.
pub async fn node_pool(admin: &PgPool, tag: &str) -> PgPool {
    let url = node_db_url(tag);
    ensure_database(admin, &db_name(&url)).await;
    let pool = pool_to(&url).await;
    ensure_schema_on(&pool).await;
    pool
}

// ─────────────────────────────────────────────────────────────────────────────
// Seed і стан фасаду
// ─────────────────────────────────────────────────────────────────────────────

/// Точка, спільна для вузла й хаба (у хабі БД мережі — точки всіх вузлів).
pub fn store_id() -> Uuid {
    Uuid::parse_str("7c9f1d20-3a5b-4e6f-8a11-2b3c4d5e6f70").expect("uuid точки")
}

/// Власник мережі (той самий uuid в обох БД — токен один на два фасади).
pub fn owner_id() -> Uuid {
    Uuid::parse_str("7c9f1d20-3a5b-4e6f-8a11-2b3c4d5e6f71").expect("uuid власника")
}

/// Seed: власник + точка + доступ + товар (той самий товар в обох БД —
/// чек, прийнятий вузлом, мусить бути прийнятним і хабом).
pub async fn seed(pool: &PgPool, product: Uuid, note: &str) {
    let store = store_id();
    let owner = owner_id();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .bind(format!("E2E {note} Точка"))
        .execute(pool)
        .await
        .expect("seed store");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, $2, $3, $4, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(owner)
    .bind(format!("E2E {note} Власник"))
    .bind(format!("hub_owner_{note}"))
    .bind("$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e")
    .execute(pool)
    .await
    .expect("seed owner");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         VALUES ($1, $2, 'owner', '{}'::jsonb, true, now()) ON CONFLICT DO NOTHING",
    )
    .bind(owner)
    .bind(store)
    .execute(pool)
    .await
    .expect("seed user_stores");
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate)
         VALUES ($1, NULL, $2, 10.00, 20.00) ON CONFLICT (id) DO NOTHING",
    )
    .bind(product)
    .bind(format!("E2E {note} Товар"))
    .execute(pool)
    .await
    .expect("seed product");
    // Залишок: приймач чека перевіряє наявність товару (і на вузлі, і на
    // хабі — чек мусить бути прийнятним на обох боках).
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, price, updated_at)
         VALUES ($1, $2, 10.000, 10.00, now()) ON CONFLICT (store_id, product_id) DO NOTHING",
    )
    .bind(store)
    .bind(product)
    .execute(pool)
    .await
    .expect("seed stock");
}

/// Стан фасаду: рівно те, що потрібно прийому push і `/setup/status`
/// (решта гілок вимкнені — жодних тестових дублів логіки).
pub fn app_state(pool: &PgPool) -> AppState {
    let store_pool = StorePool::new(pool.clone());
    AppState {
        jwt_secret: Arc::new(SECRET.to_string()),
        readdirs: None,
        write: None,
        write_pool: Some(pool.clone()),
        pos: Some(Arc::new(SqlxPos::new(store_pool.clone()))),
        ledger: None,
        auth: None,
        prro: None,
        debtors: None,
        documents: None,
        documents_pool: None,
        invoices_v1: None,
        invoices_v2: None,
        invoices_pool: None,
        return_invoices: None,
        return_invoices_pool: None,
        purchase_orders: None,
        purchase_orders_pool: None,
        print_templates: None,
        print_pool: None,
        products_v2: None,
        products_v2_pool: None,
        ocr: None,
        ocr_pool: None,
        uploads_dir: std::path::PathBuf::from("uploads"),
        store_pool: Some(store_pool),
        stores: None,
        setup: Some(Arc::new(SqlxSetupService::new(StorePool::new(
            pool.clone(),
        )))),
        node_config: NodeConfig::default(),
        local: None,
    }
}

/// Стан ВУЗЛА в тестах E5-частини B (Б2/D3): `readdirs` (роздача/прийом
/// довідників) + `auth` (створення касира `POST /api/v1/users`, PIN-логін
/// `/api/v1/auth/login-pin`). У проді обидві гілки вмикаються прапорцями
/// (`TORGASHKA_RUST_AUTH=1` і readdirs) — тут потрібні, щоб тест йшов РЕАЛЬНИМ
/// шляхом створення касира, а не INSERT'ом із тесту.
pub fn app_state_node(pool: &PgPool) -> AppState {
    let mut state = app_state_with_readdirs(pool);
    state.auth = Some(Arc::new(SqlxAuth::new(StorePool::new(pool.clone()))));
    state
}

/// Стан фасаду З гілкою readdirs: потрібен тестам, які перевіряють роздачу
/// довідників вузлам (`GET /api/v1/sync/master` реєструється лише за
/// `state.readdirs.is_some()` — як у проді). Решта гілок та сама.
pub fn app_state_with_readdirs(pool: &PgPool) -> AppState {
    let mut state = app_state(pool);
    state.readdirs = Some(Arc::new(SqlxDirectories::new(StorePool::new(pool.clone()))));
    state
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP-фасади
// ─────────────────────────────────────────────────────────────────────────────

/// Вільний порт (тестовий сервер на 127.0.0.1).
pub async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

/// Фасад на конкретному порту (хаб піднімається пізніше — порт уже знаний).
pub fn serve_on(port: u16, state: AppState) -> tokio::task::JoinHandle<()> {
    let addr = format!("127.0.0.1:{port}");
    let app: Router = router_v1::build_router(state);
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .expect("bind фасаду");
        axum::serve(listener, app).await.expect("axum::serve");
    })
}

/// Фасад на довільному порту → (base URL, порт).
pub async fn serve_any(state: AppState) -> (String, u16) {
    let port = free_port().await;
    serve_on(port, state);
    (format!("http://127.0.0.1:{port}"), port)
}

/// Токен власника (JWT, той самий формат, що видає `/auth/login`).
pub fn owner_token() -> String {
    create_access_token(&owner_id().to_string(), "owner", &[], SECRET).expect("JWT власника")
}

/// Чекати готовності фасаду (публічна readiness-проба).
pub async fn wait_ready(base: &str) {
    let client = reqwest::Client::new();
    for _ in 0..100 {
        if let Ok(r) = client
            .get(format!("{base}/api/v1/setup/status"))
            .send()
            .await
        {
            if r.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("фасад {base} не піднявся");
}

/// Конверт агрегата-чека (`PushEnvelope` приймача sync.rs).
pub fn receipt_env(client_uuid: Uuid, product: Uuid, note: &str) -> Value {
    json!({
        "type": "receipt",
        "client_uuid": client_uuid,
        "store_id": store_id(),
        "created_at": "2026-09-20T10:00:00+03:00",
        "payload": {
            "items": [{"product_id": product.to_string(), "quantity": 1, "price": "10.00"}],
            "payment_method": "cash",
            "cash_amount": "10.00",
            "notes": note,
        }
    })
}

/// `POST /api/v1/sync/push` пакетом із явним `batch_id` →
/// (код, тіло, `X-Sync-Batch-Id` із відповіді).
pub async fn push(
    base: &str,
    token: &str,
    batch_id: Uuid,
    envelopes: &[Value],
) -> (u16, Value, String) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/v1/sync/push"))
        .bearer_auth(token)
        .header("x-store-id", store_id().to_string())
        .header("x-sync-batch-id", batch_id.to_string())
        .json(envelopes)
        .send()
        .await
        .expect("push-запит");
    let code = r.status().as_u16();
    let batch = r
        .headers()
        .get("x-sync-batch-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body, batch)
}

// ─────────────────────────────────────────────────────────────────────────────
// Роль інстанса: налаштування хаба у ВЛАСНІЙ БД
// ─────────────────────────────────────────────────────────────────────────────

/// Задати апстрім-хаб у БД вузла (`sync.hub_url` + `sync.hub_token`).
/// Саме це налаштування робить інстанс вузлом, а не хабом.
pub async fn set_hub_upstream(pool: &PgPool, hub_base: &str, token: &str) {
    for key in ["sync.hub_url", "sync.hub_token"] {
        sqlx::query("DELETE FROM system_settings WHERE key = $1")
            .bind(key)
            .execute(pool)
            .await
            .expect("очистити налаштування хаба");
    }
    for (key, value) in [("sync.hub_url", hub_base), ("sync.hub_token", token)] {
        sqlx::query(
            "INSERT INTO system_settings \
                (id, module, key, value, value_type, label, is_active, created_at, updated_at, store_id) \
             VALUES ($1, 'sync', $2, $3, 'string', 'E2E хаб', true, now(), now(), NULL)",
        )
        .bind(Uuid::new_v4())
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .expect("налаштування хаба");
    }
}

/// Політика входу для касира, ще не підтвердженого хабом (ADR-0008 §10 №2,
/// варіант C): налаштування `sync.require_hub_confirm_before_login` у ВЛАСНІЙ БД
/// інстанса — так само, як роль вузла (`sync.hub_url`), а НЕ env-прапорець
/// процесу (той спільний для всіх інстансів і протікав між паралельними тестами
/// одного бінаря).
///
/// `require = true` → активний рядок (варіант C); `false` → ключа немає
/// (продакшн-дефолт: вхід дозволено, offline-first).
pub async fn set_require_hub_confirm_before_login(pool: &PgPool, require: bool) {
    let key = torgashka_infrastructure::sync_settings::REQUIRE_HUB_CONFIRM_BEFORE_LOGIN_SETTING;
    sqlx::query("DELETE FROM system_settings WHERE key = $1")
        .bind(key)
        .execute(pool)
        .await
        .expect("очистити політику входу");
    if require {
        sqlx::query(
            "INSERT INTO system_settings \
                (id, module, key, value, value_type, label, is_active, created_at, updated_at, store_id) \
             VALUES ($1, 'sync', $2, 'true', 'bool', 'E2E політика входу', true, now(), now(), NULL)",
        )
        .bind(Uuid::new_v4())
        .bind(key)
        .execute(pool)
        .await
        .expect("політика входу");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// E5/E6: пропозиції довідників, черга конфліктів, стан синку, pull довідників
// ─────────────────────────────────────────────────────────────────────────────

/// Конверт пропозиції зміни спільного довідника (`CatalogProposal`).
/// `payload` — ТОЙ САМИЙ конверт, що віддає pull-дельта (`Change.data`).
pub fn proposal_env(
    entity: &str,
    row_id: Uuid,
    op: &str,
    client_uuid: Uuid,
    payload: Value,
    base_version: i64,
) -> Value {
    json!({
        "entity": entity,
        "row_id": row_id,
        "op": op,
        "payload": payload,
        "client_uuid": client_uuid,
        "store_id": store_id(),
        "base_version": base_version,
    })
}

/// `POST /api/v1/sync/catalog-proposal` → (код, повна відповідь).
pub async fn propose(base: &str, token: &str, items: &[Value]) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/v1/sync/catalog-proposal"))
        .bearer_auth(token)
        .header("x-store-id", store_id().to_string())
        .json(items)
        .send()
        .await
        .expect("catalog-proposal запит");
    let code = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body)
}

/// `POST /api/v1/users` (require_admin + X-Store-Id) → (код, повна відповідь).
pub async fn create_user(base: &str, token: &str, body: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/v1/users"))
        .bearer_auth(token)
        .header("x-store-id", store_id().to_string())
        .json(&body)
        .send()
        .await
        .expect("POST /api/v1/users");
    let code = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body)
}

/// `POST /api/v1/auth/login-pin` (публічний) → (код, повна відповідь).
pub async fn login_pin(base: &str, login: &str, pin: &str) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/v1/auth/login-pin"))
        .json(&json!({"login": login, "pin_code": pin}))
        .send()
        .await
        .expect("POST /api/v1/auth/login-pin");
    let code = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body)
}

/// `GET /api/v1/admin/sync/conflicts` → (код, повна відповідь).
pub async fn admin_conflicts(base: &str, token: &str) -> (u16, Value) {
    let r = reqwest::Client::new()
        .get(format!("{base}/api/v1/admin/sync/conflicts"))
        .bearer_auth(token)
        .send()
        .await
        .expect("conflicts запит");
    let code = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body)
}

/// `GET /api/v1/sync/status` у скоупі точки → (код, повна відповідь).
pub async fn sync_status(base: &str, token: &str) -> (u16, Value) {
    let r = reqwest::Client::new()
        .get(format!("{base}/api/v1/sync/status"))
        .bearer_auth(token)
        .header("x-store-id", store_id().to_string())
        .send()
        .await
        .expect("sync/status запит");
    let code = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body)
}

/// `GET /api/v1/sync/master?entity&since_version` — канал роздачі довідників
/// вузлам (наявний pull; E5 перевіряє саме його, без нового каналу).
pub async fn master_delta(base: &str, token: &str, entity: &str, since: i64) -> (u16, Value) {
    let r = reqwest::Client::new()
        .get(format!(
            "{base}/api/v1/sync/master?entity={entity}&since_version={since}"
        ))
        .bearer_auth(token)
        .header("x-store-id", store_id().to_string())
        .send()
        .await
        .expect("sync/master запит");
    let code = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body)
}
