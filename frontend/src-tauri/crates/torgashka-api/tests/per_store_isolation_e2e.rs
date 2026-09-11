//! E2E (Етап 4 — регресія): per-store ізоляція offline-first синку.
//!
//! РЕАЛЬНА поверхня pull на цій гілці (sync-offline):
//!   GET /api/v1/sync/master?entity&since_version — МАЙСТЕР-ДАНІ:
//!     categories/settings  — store-scoped (pull фільтрує store_id);
//!     products/suppliers/employees — глобальні довідники власника
//!     (однакові для всіх точок; per-store ціна/залишок живе в `stock`
//!      і НЕ входить у pull — серверна сутність, push-ефект).
//!   POST /api/v1/sync/push — прийом агрегатів каси (receipt/...), per-store
//!     через X-Store-Id/DeviceCtx, ідемпотентність через client_uuid.
//!
//! Три групи регресії per-store ізоляції:
//!   1. pull_isolates_store_scoped_and_global_rows — 2 точки: pull device A
//!      містить ЛИШЕ store-scoped рядки A (categories/settings) + глобальні
//!      довідники; жоден store-scoped рядок B (і його stock/ціна) не
//!      потрапляє в pull A (і навпаки); дельта-ізоляція: нові рядки B після
//!      `to` A не з'являються в наступному pull A.
//!   2. soft_delete_reaches_only_owner_store — soft-delete store-scoped рядка
//!      точки A → pull A бачить op=delete; pull B — НЕ бачить (запис B
//!      живий); повторний pull B без delete-змін.
//!   3. five_stores_concurrent_push_no_conflicts_isolated — 5 точок × 3 чеки,
//!      одночасний push (tokio::join_all): усі created, 0 конфліктів/
//!      already_exists, COUNT по точках сходиться, pull кожної точки містить
//!      ТІЛЬКИ свій per-store рядок + глобальні, перехресних даних немає.
//!
//! Авторизація: pull — device_token (device каси, точка з DeviceCtx, без
//! X-Store-Id); push чека — JWT власника + X-Store-Id своєї точки (як у
//! sync_push_e2e/sync_typed_push_e2e; приймач чека пише cashier_id з JWT sub
//! → users.id; device-роль касиром чека бути не може — devices окрема
//! таблиця, див. звіт).
//!
//! БД: TEST_DATABASE_URL або робочий URL + _test (tests/common/mod.rs).
//! Самодостатній на ПОРОЖНІЙ БД: ensure_schema + sync_schema::apply
//! (Alembic 0011–0014: server_version/bump, soft-delete, client_uuid).

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use torgashka_api::run_facade;
use uuid::Uuid;

mod common;

/// Sync-шар схеми (0011–0014) — поза ensure_schema (schema.sql).
#[path = "common/sync_schema.rs"]
mod sync_schema;

/// Пароль тестових користувачів (bcrypt('admin123')) — як в інших e2e.
const PWD: &str = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e";
const XFF: &str = "198.51.100.40";
/// Всі 3 тести файлу працюють на ОДНІЙ тестовій БД і піднімають власний
/// фасад. Серіалізуємо їх (статичний Mutex), щоб 3 одночасні ensure_schema/
/// seed/активації не конкурували за DDL/коди активації — детермінованість.
static GATE: Mutex<()> = Mutex::new(());

fn gate() -> std::sync::MutexGuard<'static, ()> {
    GATE.lock().unwrap_or_else(|p| p.into_inner())
}

static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn apply_schema() {
    SCHEMA_ONCE
        .get_or_init(|| async {
            let p = torgashka_infrastructure::db::connect_test_pool(5)
                .await
                .expect(
                    "тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test",
                );
            torgashka_infrastructure::db::ensure_schema(&p)
                .await
                .expect("ensure_schema на тестовій БД");
            sync_schema::apply(&p).await;
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

// ─── HTTP-хелпери (патерни network_device_sync_e2e / sync_push_e2e) ────────

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

/// Login (повторюємо, поки фасад не піднявся).
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

/// POST /admin/stores/:id/activation-code → (status, тіло {code}).
async fn gen_code(base: &str, token: &str, store: Uuid) -> (u16, Value) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{base}/api/v1/admin/stores/{store}/activation-code"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("activation-code запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// POST /devices/activate (публічний) → {device_token, device_id, store_id}.
async fn activate(base: &str, code: &str, fingerprint: &str) -> (u16, Value) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/devices/activate"))
        .header("x-forwarded-for", XFF)
        .json(&json!({"code": code, "device_fingerprint": fingerprint}))
        .send()
        .await
        .expect("activate запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// GET /sync/master з device_token (без X-Store-Id) → (status, дельта).
async fn master_pull(base: &str, token: &str, entity: &str, since: i64) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .get(format!(
            "{base}/api/v1/sync/master?entity={entity}&since_version={since}"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("master pull запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// POST /sync/push (JWT + X-Store-Id) масивом агрегатів → (status, results[]).
async fn http_push(base: &str, token: &str, store_id: Uuid, envelopes: &[Value]) -> (u16, Value) {
    let req = reqwest::Client::new()
        .post(format!("{base}/api/v1/sync/push"))
        .bearer_auth(token)
        .header("X-Store-Id", store_id.to_string())
        .json(envelopes);
    let resp = req.send().await.expect("push запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// Агрегат чека sale (формат PushEnvelope sync.rs, payload ReceiptCreate).
fn receipt_env(client_uuid: Uuid, store: Uuid, product: Uuid, note: &str) -> Value {
    json!({
        "type": "receipt",
        "client_uuid": client_uuid,
        "store_id": store,
        "created_at": "2026-08-31T10:00:00+03:00",
        "payload": {
            "items": [{"product_id": product.to_string(), "quantity": 1, "price": "10.00"}],
            "payment_method": "cash",
            "cash_amount": "10.00",
            "notes": note,
        }
    })
}

fn find_change<'a>(delta: &'a Value, id: &str) -> Option<&'a Value> {
    delta["changes"]
        .as_array()
        .and_then(|arr| arr.iter().find(|c| c["id"].as_str() == Some(id)))
}

fn has_change_id(delta: &Value, id: &str) -> bool {
    find_change(delta, id).is_some()
}

fn f64_of(change: &Value, key: &str) -> f64 {
    change["data"][key]
        .as_str()
        .unwrap_or("0")
        .parse::<f64>()
        .unwrap_or(-1.0)
}

// ─── Seed-хелпери (прямі INSERT — як ensure_seed в інших e2e) ───────────────

/// Власник (role owner) з доступом до всіх store. Повертає id користувача.
async fn seed_owner(pool: &sqlx::PgPool, login: &str, name: &str, stores: &[Uuid]) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, $2, $3, $4, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO UPDATE SET name = EXCLUDED.name",
    )
    .bind(id)
    .bind(name)
    .bind(login)
    .bind(PWD)
    .execute(pool)
    .await
    .expect("seed owner");
    for s in stores {
        sqlx::query(
            "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
             VALUES ($1, $2, 'owner', '{}'::jsonb, true, now())
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(s)
        .execute(pool)
        .await
        .expect("seed user_stores");
    }
    id
}

async fn seed_store(pool: &sqlx::PgPool, id: Uuid, name: &str) {
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed store");
}

/// Категорія: store_id = None → глобальна (бачать усі точки).
async fn seed_category(pool: &sqlx::PgPool, id: Uuid, name: &str, store: Option<Uuid>) {
    sqlx::query("INSERT INTO categories (id, name, store_id) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(name)
        .bind(store)
        .execute(pool)
        .await
        .expect("seed category");
}

/// Store-scoped налаштування (system_settings, UNIQUE(store_id, key)).
async fn seed_setting(pool: &sqlx::PgPool, id: Uuid, store: Uuid, key: &str, value: &str) {
    sqlx::query(
        "INSERT INTO system_settings
           (id, module, key, value, value_type, label, is_active, created_at, updated_at, store_id)
         VALUES ($1, 'general', $2, $3, 'string', $2, true, now(), now(), $4)
         ON CONFLICT (store_id, key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(id)
    .bind(key)
    .bind(value)
    .bind(store)
    .execute(pool)
    .await
    .expect("seed setting");
}

/// Глобальний товар (products.price — глобальна ціна для всіх точок).
async fn seed_product(pool: &sqlx::PgPool, id: Uuid, title: &str, price: f64) {
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, $2, $3, 20.00)",
    )
    .bind(id)
    .bind(title)
    .bind(price)
    .execute(pool)
    .await
    .expect("seed product");
}

/// Per-store залишок+ціна (stock(store_id, product_id)) — СЕРВЕРНА сутність,
/// у pull майстер-даних не входить (перевірка ізоляції нижче).
async fn seed_stock(pool: &sqlx::PgPool, store: Uuid, product: Uuid, qty: f64, price: f64) {
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, price, updated_at) \
         VALUES ($1, $2, $3, $4, now())",
    )
    .bind(store)
    .bind(product)
    .bind(qty)
    .bind(price)
    .execute(pool)
    .await
    .expect("seed stock");
}

/// Активує касу (device) точки → device_token.
async fn register_device(base: &str, owner: &str, store: Uuid, fp: &str) -> (String, Uuid) {
    let (cs, code_body) = gen_code(base, owner, store).await;
    assert_eq!(cs, 200, "код активації точки {store}: {code_body}");
    let code = code_body["code"].as_str().expect("code").to_string();
    let (as_, act) = activate(base, &code, fp).await;
    assert_eq!(as_, 200, "активація каси {store}: {act}");
    let token = act["device_token"]
        .as_str()
        .expect("device_token")
        .to_string();
    let sid = Uuid::parse_str(act["store_id"].as_str().expect("store_id")).expect("uuid");
    assert_eq!(sid, store, "device прив'язаний до своєї точки");
    (token, sid)
}

/// Унікальний суфікс імен (захист від паралельних запусків на спільній БД).
fn suffix() -> String {
    format!("{:08x}", Uuid::new_v4().as_u128() & 0xffff_ffff)
}

// ═════════════════════════════════════════════════════════════════════════════
// Група 1: ізоляція даних per-store на pull
// ═════════════════════════════════════════════════════════════════════════════

// gate() тримається НАВМИСНО на весь тест: серіалізує 3 тести, що ділять
// одну тестову БД (DDL/seed/активації). Звуження scope ламає взаємне
// виключення → флейки. Тут дозволено свідомо.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pull_isolates_store_scoped_and_global_rows() {
    let _g = gate();
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let sx = suffix();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    seed_store(&pool, a, &format!("ISO-G1 A {sx}")).await;
    seed_store(&pool, b, &format!("ISO-G1 B {sx}")).await;
    seed_owner(
        &pool,
        &format!("iso_g1_owner_{sx}"),
        "ISO G1 Owner",
        &[a, b],
    )
    .await;

    // Категорії: per-store (A/B) + одна глобальна (store_id NULL).
    let cat_a = Uuid::new_v4();
    let cat_b = Uuid::new_v4();
    let cat_g = Uuid::new_v4();
    seed_category(&pool, cat_a, &format!("ISO-G1 cat A {sx}"), Some(a)).await;
    seed_category(&pool, cat_b, &format!("ISO-G1 cat B {sx}"), Some(b)).await;
    seed_category(&pool, cat_g, &format!("ISO-G1 cat GLOBAL {sx}"), None).await;

    // Налаштування per-store.
    let key_a = format!("iso_g1_key_a_{sx}");
    let key_b = format!("iso_g1_key_b_{sx}");
    seed_setting(&pool, Uuid::new_v4(), a, &key_a, "value-a-only").await;
    seed_setting(&pool, Uuid::new_v4(), b, &key_b, "value-b-only").await;

    // Товар P: глобальна ціна 100; per-store ЦІНА В stock: A=10, B=20 —
    // у pull приходить ЛИШЕ глобальна 100 (stock у pull не входить).
    let prod_p = Uuid::new_v4();
    let prod_p2 = Uuid::new_v4();
    seed_product(&pool, prod_p, &format!("ISO-G1 P {sx}"), 100.0).await;
    seed_product(&pool, prod_p2, &format!("ISO-G1 P2 {sx}"), 100.0).await;
    seed_stock(&pool, a, prod_p, 5000.0, 10.0).await;
    seed_stock(&pool, b, prod_p, 5000.0, 20.0).await;
    seed_stock(&pool, b, prod_p2, 5000.0, 20.0).await; // P2 лише в точці B

    // ── Фасад + пристрій A та B ────────────────────────────────────────────
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner = login(&base, &format!("iso_g1_owner_{sx}")).await;
    let (tok_a, _) = register_device(&base, &owner, a, &format!("ISO-DEV-G1-A-{sx}")).await;
    let (tok_b, _) = register_device(&base, &owner, b, &format!("ISO-DEV-G1-B-{sx}")).await;

    // ── Чеки в A (активність точки, client_uuid) ───────────────────────────
    let cu_a1 = Uuid::new_v4();
    let cu_a2 = Uuid::new_v4();
    let (sp, push_body) = http_push(
        &base,
        &owner,
        a,
        &[
            receipt_env(cu_a1, a, prod_p, &format!("iso-g1-a1 {sx}")),
            receipt_env(cu_a2, a, prod_p, &format!("iso-g1-a2 {sx}")),
        ],
    )
    .await;
    assert_eq!(sp, 200, "push A: {push_body}");
    for it in push_body.as_array().expect("results") {
        assert_eq!(it["status"], "created", "чек A прийнято: {it}");
    }
    let n_a: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM receipts WHERE store_id = $1 AND client_uuid IS NOT NULL",
    )
    .bind(a)
    .fetch_one(&pool)
    .await
    .expect("count receipts A");
    let n_b: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM receipts WHERE store_id = $1 AND client_uuid IS NOT NULL",
    )
    .bind(b)
    .fetch_one(&pool)
    .await
    .expect("count receipts B");
    assert_eq!(n_a, 2, "чеки A на місці");
    assert_eq!(n_b, 0, "чеки A НЕ потрапили в точку B");

    // ── Pull device A: store-scoped ряди ЛИШЕ своєї точки ─────────────────
    let (s, d_cat_a) = master_pull(&base, &tok_a, "categories", 0).await;
    assert_eq!(s, 200, "pull categories A: {d_cat_a}");
    assert!(
        has_change_id(&d_cat_a, &cat_a.to_string()),
        "категорія A у pull A"
    );
    assert!(
        has_change_id(&d_cat_a, &cat_g.to_string()),
        "глобальна категорія у pull A"
    );
    assert!(
        !has_change_id(&d_cat_a, &cat_b.to_string()),
        "категорія точки B НЕ у pull A: {d_cat_a}"
    );

    let (s, d_set_a) = master_pull(&base, &tok_a, "settings", 0).await;
    assert_eq!(s, 200, "pull settings A: {d_set_a}");
    let keys_a: Vec<&str> = d_set_a["changes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["data"]["key"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        keys_a.iter().any(|k| *k == key_a),
        "налаштування A у pull A: {keys_a:?}"
    );
    assert!(
        !keys_a.iter().any(|k| *k == key_b),
        "налаштування B НЕ у pull A: {keys_a:?}"
    );

    // Pull B — симетрично.
    let (s, d_cat_b) = master_pull(&base, &tok_b, "categories", 0).await;
    assert_eq!(s, 200);
    assert!(
        has_change_id(&d_cat_b, &cat_b.to_string()),
        "категорія B у pull B"
    );
    assert!(
        has_change_id(&d_cat_b, &cat_g.to_string()),
        "глобальна у pull B"
    );
    assert!(
        !has_change_id(&d_cat_b, &cat_a.to_string()),
        "категорія A НЕ у pull B"
    );
    let (s, d_set_b) = master_pull(&base, &tok_b, "settings", 0).await;
    assert_eq!(s, 200);
    let keys_b: Vec<&str> = d_set_b["changes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["data"]["key"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(keys_b.iter().any(|k| *k == key_b));
    assert!(!keys_b.iter().any(|k| *k == key_a));

    // ── Ціни: pull віддає ГЛОБАЛЬНУ ціну товару (100), НЕ per-store stock
    //    (A=10, B=20). Stock-рядок з store_id B у pull A не з'являється
    //    взагалі (сутність stock поза master). ─────────────────────────────
    let (s, d_prod_a) = master_pull(&base, &tok_a, "products", 0).await;
    assert_eq!(s, 200, "pull products A: {d_prod_a}");
    let (s, d_prod_b) = master_pull(&base, &tok_b, "products", 0).await;
    assert_eq!(s, 200, "pull products B: {d_prod_b}");
    for (tag, d) in [("A", &d_prod_a), ("B", &d_prod_b)] {
        let ch_p = find_change(d, &prod_p.to_string()).expect("товар P у pull");
        assert_eq!(
            f64_of(ch_p, "price"),
            100.0,
            "pull {tag}: глобальна ціна P (не stock A=10/B=20): {ch_p}"
        );
        let ch_p2 = find_change(d, &prod_p2.to_string()).expect("товар P2 у pull");
        assert_eq!(
            f64_of(ch_p2, "price"),
            100.0,
            "pull {tag}: P2 глобальна ціна (stock B=20 не протікає): {ch_p2}"
        );
    }
    // Сутність stock НЕ є частиною pull (немає в ALLOWED_ENTITIES).
    let (s_stock, _) = master_pull(&base, &tok_a, "stock", 0).await;
    assert_eq!(s_stock, 400, "stock — не сутність master pull");

    // DB: продаж A зменшив ЛИШЕ stock A (перехресного ефекту на B немає).
    let stock_a: f64 = sqlx::query_scalar(
        "SELECT quantity::float8 FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(a)
    .bind(prod_p)
    .fetch_one(&pool)
    .await
    .expect("stock A");
    let stock_b: f64 = sqlx::query_scalar(
        "SELECT quantity::float8 FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(b)
    .bind(prod_p)
    .fetch_one(&pool)
    .await
    .expect("stock B");
    let price_b: f64 = sqlx::query_scalar(
        "SELECT price::float8 FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(b)
    .bind(prod_p)
    .fetch_one(&pool)
    .await
    .expect("price B");
    assert!(
        (stock_a - 4998.0).abs() < 1e-6,
        "stock A зменшено на 2 чеки: {stock_a}"
    );
    assert!(
        (stock_b - 5000.0).abs() < 1e-6,
        "stock B недоторканий: {stock_b}"
    );
    assert!((price_b - 20.0).abs() < 1e-6, "ціна B залишилась 20");

    // ── Дельта-ізоляція: нові ряди після `to` точки ───────────────────────
    let to_cat_a = d_cat_a["to"].as_i64().expect("to categories A");
    let to_set_a = d_set_a["to"].as_i64().expect("to settings A");
    let cat_a2 = Uuid::new_v4();
    let cat_b2 = Uuid::new_v4();
    seed_category(&pool, cat_a2, &format!("ISO-G1 cat A2 {sx}"), Some(a)).await;
    seed_category(&pool, cat_b2, &format!("ISO-G1 cat B2 {sx}"), Some(b)).await;
    seed_setting(
        &pool,
        Uuid::new_v4(),
        a,
        &format!("iso_g1_key_a2_{sx}"),
        "a2",
    )
    .await;
    seed_setting(
        &pool,
        Uuid::new_v4(),
        b,
        &format!("iso_g1_key_b2_{sx}"),
        "b2",
    )
    .await;

    let (s, d2_cat_a) = master_pull(&base, &tok_a, "categories", to_cat_a).await;
    assert_eq!(s, 200, "інкрементальний pull A: {d2_cat_a}");
    assert!(
        has_change_id(&d2_cat_a, &cat_a2.to_string()),
        "новий рядок A у дельті A"
    );
    assert!(
        !has_change_id(&d2_cat_a, &cat_b2.to_string()),
        "новий рядок B НЕ протікає в дельту A: {d2_cat_a}"
    );
    let (s, d2_set_a) = master_pull(&base, &tok_a, "settings", to_set_a).await;
    assert_eq!(s, 200, "інкрементальний pull settings A: {d2_set_a}");
    let keys2_a: Vec<&str> = d2_set_a["changes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["data"]["key"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        keys2_a
            .iter()
            .any(|k| k.starts_with(&format!("iso_g1_key_a2_{sx}"))),
        "новий ключ A у дельті A: {keys2_a:?}"
    );
    assert!(
        !keys2_a
            .iter()
            .any(|k| k.starts_with(&format!("iso_g1_key_b2_{sx}"))),
        "новий ключ B НЕ у дельті A: {keys2_a:?}"
    );

    let to_cat_b = d_cat_b["to"].as_i64().expect("to categories B");
    let (s, d2_cat_b) = master_pull(&base, &tok_b, "categories", to_cat_b).await;
    assert_eq!(s, 200);
    assert!(
        has_change_id(&d2_cat_b, &cat_b2.to_string()),
        "новий рядок B у дельті B"
    );
    assert!(
        !has_change_id(&d2_cat_b, &cat_a2.to_string()),
        "новий рядок A НЕ у дельті B"
    );

    eprintln!(
        "[per_store_isolation] ✅ Група 1: per-store pull (2 точки) — без перехресних рядків/цін"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Група 2: op=delete доходить ЛИШЕ до точки-власника рядка
// ═════════════════════════════════════════════════════════════════════════════

// gate() тримається НАВМИСНО на весь тест: серіалізує 3 тести, що ділять
// одну тестову БД (DDL/seed/активації). Звуження scope ламає взаємне
// виключення → флейки. Тут дозволено свідомо.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn soft_delete_reaches_only_owner_store() {
    let _g = gate();
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let sx = suffix();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    seed_store(&pool, a, &format!("ISO-G2 A {sx}")).await;
    seed_store(&pool, b, &format!("ISO-G2 B {sx}")).await;
    seed_owner(
        &pool,
        &format!("iso_g2_owner_{sx}"),
        "ISO G2 Owner",
        &[a, b],
    )
    .await;

    // «Той самий товар у двох точках»: однаковий products.id, різні ряди
    // stock(store_id). Store-scoped master-рядок (категорія) — теж по одному
    // на точку з ОДНАКОВОЮ назвою.
    let prod = Uuid::new_v4();
    seed_product(&pool, prod, &format!("ISO-G2 shared {sx}"), 50.0).await;
    seed_stock(&pool, a, prod, 1000.0, 50.0).await;
    seed_stock(&pool, b, prod, 1000.0, 60.0).await;

    let cat_a = Uuid::new_v4();
    let cat_b = Uuid::new_v4();
    let shared_name = format!("ISO-G2 спільний товар {sx}");
    seed_category(&pool, cat_a, &shared_name, Some(a)).await;
    seed_category(&pool, cat_b, &shared_name, Some(b)).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner = login(&base, &format!("iso_g2_owner_{sx}")).await;
    let (tok_a, _) = register_device(&base, &owner, a, &format!("ISO-DEV-G2-A-{sx}")).await;
    let (tok_b, _) = register_device(&base, &owner, b, &format!("ISO-DEV-G2-B-{sx}")).await;

    // До видалення: pull A бачить живий рядок A; pull B — живий рядок B.
    let (s, d0_a) = master_pull(&base, &tok_a, "categories", 0).await;
    assert_eq!(s, 200);
    let ch_a0 = find_change(&d0_a, &cat_a.to_string()).expect("рядок A у pull A");
    assert_eq!(ch_a0["op"], "upsert", "до видалення рядок A живий: {ch_a0}");
    assert_eq!(ch_a0["data"]["name"], shared_name);
    assert!(
        !has_change_id(&d0_a, &cat_b.to_string()),
        "рядок B не видно з A"
    );
    let (s, d0_b) = master_pull(&base, &tok_b, "categories", 0).await;
    assert_eq!(s, 200);
    let ch_b0 = find_change(&d0_b, &cat_b.to_string()).expect("рядок B у pull B");
    assert_eq!(ch_b0["op"], "upsert", "до видалення рядок B живий: {ch_b0}");
    assert!(
        !has_change_id(&d0_b, &cat_a.to_string()),
        "рядок A не видно з B"
    );

    // Soft-delete ЛИШЕ рядка точки A.
    sqlx::query("UPDATE categories SET is_deleted = true WHERE id = $1")
        .bind(cat_a)
        .execute(&pool)
        .await
        .expect("soft-delete рядка A");
    let alive_a: bool = sqlx::query_scalar("SELECT is_deleted FROM categories WHERE id = $1")
        .bind(cat_a)
        .fetch_one(&pool)
        .await
        .expect("row A exists");
    assert!(
        alive_a,
        "soft-delete: рядок A ЗАЛИШАЄТЬСЯ в БД (is_deleted=true)"
    );

    // Pull A: op=delete для свого рядка.
    let (s, d1_a) = master_pull(&base, &tok_a, "categories", 0).await;
    assert_eq!(s, 200);
    let ch_a1 = find_change(&d1_a, &cat_a.to_string()).expect("рядок A у pull A після delete");
    assert_eq!(ch_a1["op"], "delete", "pull A містить op=delete: {ch_a1}");
    assert!(ch_a1["data"].is_null(), "delete без data: {ch_a1}");
    assert!(
        !has_change_id(&d1_a, &cat_b.to_string()),
        "рядок B (живий) НЕ у pull A: {d1_a}"
    );

    // Pull B: запис B живий, delete рядка A НЕ протікає.
    let (s, d1_b) = master_pull(&base, &tok_b, "categories", 0).await;
    assert_eq!(s, 200);
    let ch_b1 = find_change(&d1_b, &cat_b.to_string()).expect("рядок B у pull B після delete A");
    assert_eq!(ch_b1["op"], "upsert", "запис B живий: {ch_b1}");
    assert_eq!(ch_b1["data"]["name"], shared_name);
    assert!(
        !has_change_id(&d1_b, &cat_a.to_string()),
        "чужого op=delete НЕ видно з B: {d1_b}"
    );
    let del_in_b = d1_b["changes"]
        .as_array()
        .map(|arr| arr.iter().filter(|c| c["op"] == "delete").count())
        .unwrap_or(0);
    assert_eq!(
        del_in_b, 0,
        "pull B не містить жодного delete (все своє живе): {d1_b}"
    );

    // DB: рядок B недоторканий.
    let alive_b: bool = sqlx::query_scalar("SELECT is_deleted FROM categories WHERE id = $1")
        .bind(cat_b)
        .fetch_one(&pool)
        .await
        .expect("row B");
    assert!(!alive_b, "рядок B живий у БД");

    eprintln!("[per_store_isolation] ✅ Група 2: soft-delete досягає ЛИШЕ точки-власника");
}

// ═════════════════════════════════════════════════════════════════════════════
// Група 3: навантаження — 5 точок, паралельний push, 0 конфліктів, ізоляція
// ═════════════════════════════════════════════════════════════════════════════

// gate() тримається НАВМИСНО на весь тест: серіалізує 3 тести, що ділять
// одну тестову БД (DDL/seed/активації). Звуження scope ламає взаємне
// виключення → флейки. Тут дозволено свідомо.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn five_stores_concurrent_push_no_conflicts_isolated() {
    let _g = gate();
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let sx = suffix();
    const N: usize = 5;
    const PER: usize = 3;

    let owner_login = format!("iso_g3_owner_{sx}");
    let stores: Vec<Uuid> = (0..N).map(|_| Uuid::new_v4()).collect();
    for (k, s) in stores.iter().enumerate() {
        seed_store(&pool, *s, &format!("ISO-G3 Точка {k} {sx}")).await;
    }
    seed_owner(&pool, &owner_login, "ISO G3 Owner", &stores).await;

    // Per-store: товар + залишок (100000), категорія; одна глобальна.
    let mut prods = Vec::new();
    let mut cats = Vec::new();
    for (k, s) in stores.iter().enumerate() {
        let p = Uuid::new_v4();
        let c = Uuid::new_v4();
        seed_product(&pool, p, &format!("ISO-G3 товар {k} {sx}"), 10.0).await;
        seed_stock(&pool, *s, p, 100000.0, 10.0).await;
        seed_category(&pool, c, &format!("ISO-G3 cat {k} {sx}"), Some(*s)).await;
        prods.push(p);
        cats.push(c);
    }
    let cat_g = Uuid::new_v4();
    seed_category(&pool, cat_g, &format!("ISO-G3 cat GLOBAL {sx}"), None).await;

    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner = login(&base, &owner_login).await;

    // 5 device-кас (по одній на точку) — pull після push.
    let mut dev_tokens = Vec::new();
    for (k, s) in stores.iter().enumerate() {
        let (t, _) = register_device(&base, &owner, *s, &format!("ISO-DEV-G3-{k}-{sx}")).await;
        dev_tokens.push(t);
    }

    // ── Паралельний push: N×PER унікальних чеків (client_uuid), одночасно ──
    let mut envelopes = Vec::new();
    let mut uuids: Vec<Vec<Uuid>> = vec![Vec::new(); N];
    for (k, s) in stores.iter().enumerate() {
        for n in 0..PER {
            let cu = Uuid::new_v4();
            uuids[k].push(cu);
            envelopes.push((
                k,
                receipt_env(cu, *s, prods[k], &format!("iso-g3-s{k}-r{n} {sx}")),
            ));
        }
    }
    let base_push = base.clone();
    let owner_push = owner.clone();
    let mut set = tokio::task::JoinSet::new();
    for (k, env) in envelopes {
        let b = base_push.clone();
        let t = owner_push.clone();
        let store = stores[k];
        set.spawn(async move {
            let (st, body) = http_push(&b, &t, store, std::slice::from_ref(&env)).await;
            (st, body)
        });
    }
    let mut results = Vec::new();
    while let Some(r) = set.join_next().await {
        results.push(r.expect("push task"));
    }
    for (st, body) in results {
        assert_eq!(st, 200, "паралельний push статус: {body}");
        let arr = body.as_array().expect("push results масив");
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0]["status"], "created",
            "КРИТЕРІЙ: усі паралельні push → created, без already_exists/конфліктів: {body}"
        );
    }

    // ── COUNT сходиться: рівно PER чеків на точку, дублікатів 0 ───────────
    let all_uuids: Vec<Uuid> = uuids.iter().flatten().copied().collect();
    for (k, s) in stores.iter().enumerate() {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM receipts WHERE store_id = $1 AND client_uuid = ANY($2)",
        )
        .bind(s)
        .bind(&uuids[k])
        .fetch_one(&pool)
        .await
        .expect("count store");
        assert_eq!(n, PER as i64, "точка {k}: рівно {PER} чеків на місці");
        let distinct: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT client_uuid) FROM receipts WHERE store_id = $1 AND client_uuid = ANY($2)",
        )
        .bind(s)
        .bind(&uuids[k])
        .fetch_one(&pool)
        .await
        .expect("distinct store");
        assert_eq!(
            distinct, PER as i64,
            "точка {k}: без дублікатів client_uuid"
        );
    }
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM receipts WHERE client_uuid = ANY($1)")
            .bind(&all_uuids)
            .fetch_one(&pool)
            .await
            .expect("total");
    assert_eq!(total, (N * PER) as i64, "сумарно всі чеки на місці (15)");

    // ── Перехресних даних немає: кожен client_uuid осів у СВОЇЙ точці ─────
    for (k, s) in stores.iter().enumerate() {
        let got: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT store_id FROM receipts WHERE client_uuid = ANY($1)",
        )
        .bind(&uuids[k])
        .fetch_all(&pool)
        .await
        .expect("store of uuids");
        assert_eq!(
            got,
            vec![*s],
            "точка {k}: чеки осіли в своїй точці, не в чужих: {got:?}"
        );
    }

    // sync_log: per-store рівно PER записів ok; жодного error/already_exists.
    for (k, s) in stores.iter().enumerate() {
        let ok: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sync_log WHERE store_id = $1 AND direction = 'push' \
             AND client_uuid = ANY($2) AND status = 'ok'",
        )
        .bind(s)
        .bind(&uuids[k])
        .fetch_one(&pool)
        .await
        .expect("sync_log ok");
        assert_eq!(ok, PER as i64, "точка {k}: sync_log ok = {PER}");
    }
    let bad: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sync_log WHERE client_uuid = ANY($1) AND status <> 'ok'",
    )
    .bind(&all_uuids)
    .fetch_one(&pool)
    .await
    .expect("sync_log bad");
    assert_eq!(bad, 0, "жодного error/already_exists у sync_log");

    // ── Stock: паралельні продажі не загубили оновлення (кожна точка своя) ─
    for (k, s) in stores.iter().enumerate() {
        let qty: f64 = sqlx::query_scalar(
            "SELECT quantity::float8 FROM stock WHERE store_id = $1 AND product_id = $2",
        )
        .bind(s)
        .bind(prods[k])
        .fetch_one(&pool)
        .await
        .expect("stock");
        assert!(
            (qty - (100000.0 - PER as f64)).abs() < 1e-6,
            "точка {k}: stock {qty} == 100000 − {PER} (без втрат оновлень)"
        );
    }

    // ── Pull кожної точки після push: ЛИШЕ свій per-store рядок + глобальні ─
    for (k, tok) in dev_tokens.iter().enumerate() {
        let (s, d) = master_pull(&base, tok, "categories", 0).await;
        assert_eq!(s, 200, "pull точки {k}: {d}");
        assert!(
            has_change_id(&d, &cats[k].to_string()),
            "точка {k}: своя категорія у pull"
        );
        assert!(
            has_change_id(&d, &cat_g.to_string()),
            "точка {k}: глобальна у pull"
        );
        for (j, cj) in cats.iter().enumerate() {
            if j != k {
                assert!(
                    !has_change_id(&d, &cj.to_string()),
                    "точка {k}: чужа категорія {j} НЕ у pull: {d}"
                );
            }
        }
    }

    eprintln!("[per_store_isolation] ✅ Група 3: 5 точок × {PER} паралельних push — 0 конфліктів, COUNT точний, pull ізольований");
}
