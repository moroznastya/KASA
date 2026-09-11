//! E2E ADR-0007 §3.4 (клас LOCAL_SQLITE): ПРИБУТКОВА НАКЛАДНА (invoice)
//! з каси (standby) → серверний приймач /api/v1/sync/push.
//!
//! Каркас 1:1 з `sync_typed_push_e2e.rs` (ізольована _test БД, локальний
//! test-сервер run_facade, push через реальний клієнт каси).
//!
//! Каса (SQLite) створює накладну ОФЛАЙН атомарно: агрегат `invoices`
//! (synced=1) + деталізація `invoice_items` + outbox(pending) + локальний
//! stock +qty. Сервер приймає її ЧЕРЕЗ СЕРВІС інвойсів (create_v1 → confirm_v1):
//! stock +qty РІВНО ОДИН раз, статус 'confirmed'.
//!
//! ТЕСТ 1 `invoice_push_idempotent_and_stock_once`:
//!   push → 1 рядок invoices (client_uuid), N позицій, status='confirmed',
//!   stock +3; повторний push (done→pending) → already_exists + 0 дублів.
//! ТЕСТ 2 `invoice_unknown_catalog_ref_rejected_humanly`:
//!   неіснуючий постачальник / товар → status='error' з ЛЮДСЬКИМ текстом
//!   («каталоз», «не знайдено»), 0 рядків invoices, stock без змін.
//!
//! ВАЖЛИВО: `TORGASHKA_RUST_INVOICES=1` ставиться ДО run_facade — якщо фасад
//! інвойсів не змонтовано, приймач відповідає error і тест ПАДАЄ (не «проходить»
//! мовчки).

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_push::{
    open_connection, pending_count, push_pending_batch, PushConfig, PushSummary,
};
use torgashka_infrastructure::offline::transactions;
use uuid::Uuid;

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

async fn api_pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool")
}

#[path = "common/sync_schema.rs"]
mod sync_schema;

static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

/// Схема тестової БД: ensure_schema (schema.sql) + sync-шар (Alembic 0011-0016,
/// включно з partial UNIQUE uq_invoices_client_uuid).
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

/// Seed: адмін + точка + постачальник (товар — окремо).
async fn ensure_seed(pool: &sqlx::PgPool) -> (Uuid, Uuid) {
    apply_schema().await;
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Admin', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
         ON CONFLICT (login) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind("$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e")
    .execute(pool)
    .await
    .expect("seed admin");
    let supplier = Uuid::new_v4();
    sqlx::query("INSERT INTO suppliers (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
        .bind(supplier)
        .bind("E2E Invoice Постачальник")
        .execute(pool)
        .await
        .expect("seed supplier");
    let store = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
        .bind(store)
        .bind("E2E Invoice Точка")
        .execute(pool)
        .await
        .expect("seed store");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
         SELECT u.id, $1, 'owner', '{}'::jsonb, true, now()
         FROM users u WHERE u.login = 'admin'
         ON CONFLICT DO NOTHING",
    )
    .bind(store)
    .execute(pool)
    .await
    .expect("seed user_stores");
    (store, supplier)
}

async fn ensure_product(pool: &sqlx::PgPool) -> Uuid {
    let product = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E Invoice Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(pool)
    .await
    .expect("seed product");
    product
}

/// payload прибуткової накладної v1 (як його кладе каса в outbox).
fn invoice_payload(supplier: Uuid, product: Uuid, qty: &str) -> String {
    json!({
        "number": "INV-E2E-1",
        "supplier_id": supplier.to_string(),
        "invoice_date": "2026-08-30T12:00:00+03:00",
        "payment_method": null,
        "is_fiscal": false,
        "notes": "офлайн-накладна (e2e)",
        "total_amount": "300.00",
        "items": [{
            "product_id": product.to_string(),
            "quantity": qty,
            "price": "100.00",
            "total": "300.00",
        }],
    })
    .to_string()
}

/// Локальний каталог каси (SQLite `products_v2`) — як після master-pull.
/// Шляхи ПРИЙМАННЯ валідують позиції проти нього (ADR-0007 §5 AT-14).
fn seed_local_catalog(conn: &rusqlite::Connection, product_ids: &[String]) {
    for id in product_ids {
        conn.execute(
            "INSERT INTO products_v2 (id, name, price, is_deleted, server_version) \
             VALUES (?1, 'E2E каталог (pull)', 100.0, 0, 1) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            [id],
        )
        .expect("products_v2 (локальний каталог)");
    }
}

/// product_id позицій payload (щоб каса «знала» товари документа).
fn payload_products(payload: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(payload).expect("payload JSON");
    v["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|it| it["product_id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Каса: накладна в локальному контурі (агрегат + позиції + outbox + stock).
///
/// Каталог каси заповнюється товарами payload (як після master-pull):
/// приймання товару, якого точка не знає, локальний шлях НЕ створює
/// (ADR-0007 §5 AT-14).
fn build_cash_db(
    dir: &tempfile::TempDir,
    tag: &str,
    store: Uuid,
    payload: &str,
) -> (PathBuf, String) {
    let db_path = dir.path().join(format!("invoice-{tag}.db"));
    let mut conn = open_connection(&db_path).expect("каса БД");
    seed_local_catalog(&conn, &payload_products(payload));
    let out = transactions::enqueue_invoice(&mut conn, payload, &store.to_string())
        .expect("enqueue_invoice на касі");

    // Локальний контур замкнений: агрегат synced=1 + рівно один pending оп.
    let synced: i64 = conn
        .query_row(
            "SELECT synced FROM invoices WHERE client_uuid = ?1",
            rusqlite::params![out.client_uuid],
            |r| r.get(0),
        )
        .expect("synced");
    assert_eq!(synced, 1, "агрегат на касі — push-кандидат");
    assert_eq!(pending_count(&conn).expect("pending"), 1, "1 оп в outbox");
    drop(conn);
    (db_path, out.client_uuid)
}

/// Каса, яка НЕ знає товару (порожній локальний каталог) і НЕ валідує позиції:
/// агрегат + outbox-запис створюються напряму в SQLite — так виглядає черга,
/// написана СТАРОЮ версією каси / іншим вузлом. Потрібно, щоб перевірити
/// СЕРВЕРНИЙ pre-flight приймача (незалежний від локальної валідації каси).
fn build_cash_db_raw(
    dir: &tempfile::TempDir,
    tag: &str,
    store: Uuid,
    payload: &str,
) -> (PathBuf, String) {
    let db_path = dir.path().join(format!("invoice-{tag}.db"));
    let conn = open_connection(&db_path).expect("каса БД");
    let client_uuid = Uuid::new_v4().to_string();
    let envelope = serde_json::json!({
        "type": "invoice",
        "client_uuid": client_uuid,
        "store_id": store.to_string(),
        "created_at": chrono::Utc::now().to_rfc3339(),
        "payload": serde_json::from_str::<serde_json::Value>(payload).expect("payload JSON"),
    })
    .to_string();
    conn.execute(
        "INSERT INTO invoices (client_uuid, store_id, data, synced) VALUES (?1, ?2, ?3, 1)",
        rusqlite::params![client_uuid, store.to_string(), payload],
    )
    .expect("агрегат накладної (raw)");
    conn.execute(
        "INSERT INTO outbox (type, client_uuid, payload, status) VALUES ('invoice', ?1, ?2, 'pending')",
        rusqlite::params![client_uuid, envelope],
    )
    .expect("outbox-запис (raw)");
    assert_eq!(pending_count(&conn).expect("pending"), 1, "1 оп в outbox");
    drop(conn);
    (db_path, client_uuid)
}

/// Пакет push через РЕАЛЬНИЙ клієнт каси; повертає підсумок останнього пакета.
async fn push_once(db_path: &Path, client: &reqwest::Client, cfg: &PushConfig) -> PushSummary {
    push_pending_batch(db_path, client, cfg)
        .await
        .expect("push")
}

async fn login(base: &str) -> String {
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if let Ok(r) = client
            .post(format!("{base}/api/v1/auth/login"))
            .json(&json!({"login": "admin", "password": "admin123"}))
            .send()
            .await
        {
            if r.status().is_success() {
                let v: serde_json::Value = r.json().await.expect("login json");
                return v["access_token"].as_str().expect("token").to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("login не вдався");
}

/// Сирий per-item результат сервера: надсилаємо конверт(и) з outbox напряму.
async fn push_raw(
    db_path: &Path,
    base: &str,
    token: &str,
    store: Uuid,
    only_client_uuid: Option<&str>,
) -> Vec<serde_json::Value> {
    let conn = open_connection(db_path).expect("БД");
    let mut body: Vec<serde_json::Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT payload FROM outbox ORDER BY id")
            .expect("stmt");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<String>, _>>()
            .expect("rows");
        for p in rows {
            let v: serde_json::Value = serde_json::from_str(&p).expect("конверт JSON");
            if let Some(cu) = only_client_uuid {
                if v["client_uuid"].as_str() != Some(cu) {
                    continue;
                }
            }
            body.push(v);
        }
    }
    drop(conn);
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/sync/push"))
        .bearer_auth(token)
        .header("X-Store-Id", store.to_string())
        .json(&body)
        .send()
        .await
        .expect("push HTTP");
    assert!(
        resp.status().is_success(),
        "HTTP {} — приймач не мав відповісти 5xx/4xx",
        resp.status()
    );
    resp.json().await.expect("per-item JSON")
}

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ 1: накладна приймається рівно один раз, stock +qty один ефект
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test]
async fn invoice_push_idempotent_and_stock_once() {
    common::force_test_db();
    let pool = api_pool().await;
    let (store, supplier) = ensure_seed(&pool).await;
    let product = ensure_product(&pool).await;

    // Каса створює накладну ОФЛАЙН (сервер ще вимкнений).
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let payload = invoice_payload(supplier, product, "3");
    let (db, client_uuid) = build_cash_db(&dir, "ok", store, &payload);
    let cu = Uuid::parse_str(&client_uuid).expect("uuid");

    // Фаза 2: сервер піднято (Rust-гілка інвойсів УВІМКНЕНА) → push.
    std::env::set_var(torgashka_api::RUST_INVOICES_ENV, "1");
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;
    let client = reqwest::Client::new();
    let cfg = PushConfig {
        base_url: base.clone(),
        token: token.clone(),
        store_id: Some(store.to_string()),
        db_path: db.clone(),
        interval_secs: 30,
    };
    let s1 = push_once(&db, &client, &cfg).await;
    eprintln!("[invoice e2e] перший push: {s1:?}");
    assert_eq!(s1.done, 1, "перший push → created (done)");
    assert_eq!(s1.already_exists, 0, "перший push — не дублікат");
    assert_eq!(s1.failed, 0, "помилок немає");
    let conn = open_connection(&db).expect("БД");
    assert_eq!(
        pending_count(&conn).expect("pending"),
        0,
        "outbox спорожнів"
    );
    drop(conn);

    // Фаза 3: серверний стан — 1 накладна, 1 позиція, confirmed, stock +3.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE client_uuid = $1")
        .bind(cu)
        .fetch_one(&pool)
        .await
        .expect("count invoices");
    assert_eq!(n, 1, "накладна прийнята рівно 1 раз");
    let (status, srv_store, srv_supplier, srv_number): (String, Uuid, Uuid, String) = sqlx::query_as(
        "SELECT status::text, store_id, supplier_id, number FROM invoices WHERE client_uuid = $1",
    )
    .bind(cu)
    .fetch_one(&pool)
    .await
    .expect("invoice row");
    assert_eq!(status, "confirmed", "касова накладна одразу підтверджена");
    assert_eq!(srv_store, store);
    assert_eq!(srv_supplier, supplier);
    assert_eq!(srv_number, "INV-E2E-1", "номер з payload каси");

    let (items, qty): (i64, f64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(ii.quantity::float8), 0) FROM invoice_items ii \
         JOIN invoices i ON i.id = ii.invoice_id WHERE i.client_uuid = $1",
    )
    .bind(cu)
    .fetch_one(&pool)
    .await
    .expect("items");
    assert_eq!(items, 1, "1 позиція накладної");
    assert!((qty - 3.0).abs() < 0.001, "позиція 3 шт, маємо {qty}");

    let stock: f64 = sqlx::query_scalar(
        "SELECT quantity::float8 FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(store)
    .bind(product)
    .fetch_one(&pool)
    .await
    .expect("stock");
    assert!(
        (stock - 3.0).abs() < 0.001,
        "stock точки = +3 (один ефект confirm), маємо {stock}"
    );
    let pstock: f64 = sqlx::query_scalar("SELECT stock::float8 FROM products WHERE id = $1")
        .bind(product)
        .fetch_one(&pool)
        .await
        .expect("products.stock");
    assert!(
        (pstock - 3.0).abs() < 0.001,
        "products.stock = +3, маємо {pstock}"
    );

    // Фаза 4: повторний push (done→pending) → already_exists, 0 дублів.
    let conn = open_connection(&db).expect("БД");
    conn.execute(
        "UPDATE outbox SET status = 'pending', next_attempt_at = datetime('now') \
         WHERE status = 'done'",
        [],
    )
    .expect("reset done→pending");
    drop(conn);
    let s2 = push_once(&db, &client, &cfg).await;
    eprintln!("[invoice e2e] повторний push: {s2:?}");
    assert_eq!(s2.already_exists, 1, "повторний push → already_exists");
    assert_eq!(s2.done, 0, "нового created немає");
    assert_eq!(s2.failed, 0, "помилок немає");

    let n2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE client_uuid = $1")
        .bind(cu)
        .fetch_one(&pool)
        .await
        .expect("count invoices 2");
    let items2: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invoice_items ii JOIN invoices i ON i.id = ii.invoice_id \
         WHERE i.client_uuid = $1",
    )
    .bind(cu)
    .fetch_one(&pool)
    .await
    .expect("count items 2");
    let stock2: f64 = sqlx::query_scalar(
        "SELECT quantity::float8 FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(store)
    .bind(product)
    .fetch_one(&pool)
    .await
    .expect("stock 2");
    assert_eq!(n2, 1, "дублів накладних 0");
    assert_eq!(items2, 1, "дублів позицій 0");
    assert!(
        (stock2 - 3.0).abs() < 0.001,
        "stock НЕ подвоївся (має лишитись 3), маємо {stock2}"
    );
    eprintln!(
        "[sync_invoice_push_e2e] ✅ ТЕСТ 1: created → already_exists, 1 накладна, 1 позиція, stock +3 (один ефект)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ТЕСТ 2: неіснуючий каталог → людська помилка, нічого не застосовано
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test]
async fn invoice_unknown_catalog_ref_rejected_humanly() {
    common::force_test_db();
    let pool = api_pool().await;
    let (store, supplier) = ensure_seed(&pool).await;
    let product = ensure_product(&pool).await;

    std::env::set_var(torgashka_api::RUST_INVOICES_ENV, "1");
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    let dir = tempfile::TempDir::new().expect("tmpdir");

    // (а) постачальник = випадковий UUID (немає в suppliers).
    let ghost_supplier = Uuid::new_v4();
    let p1 = invoice_payload(ghost_supplier, product, "3");
    let (db1, cu1) = build_cash_db(&dir, "ghost-supplier", store, &p1);
    let res = push_raw(&db1, &base, &token, store, Some(&cu1)).await;
    eprintln!("[invoice e2e] ТЕСТ 2а (невідомий постачальник): {res:?}");
    assert_eq!(res.len(), 1, "один агрегат у пакеті");
    assert_eq!(res[0]["status"], "error", "статус = error (НЕ 500)");
    let err = res[0]["error"].as_str().unwrap_or_default().to_string();
    assert!(
        err.contains("каталоз") && err.contains("не знайдено"),
        "людське повідомлення про каталог, маємо: {err}"
    );
    assert!(
        err.contains(&ghost_supplier.to_string()),
        "у тексті — конкретний id постачальника: {err}"
    );
    let n1: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE client_uuid = $1")
        .bind(Uuid::parse_str(&cu1).expect("uuid"))
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(n1, 0, "накладну НЕ створено (pre-flight до INSERT)");
    let stock1: f64 = sqlx::query_scalar(
        "SELECT COALESCE(quantity::float8, 0) FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(store)
    .bind(product)
    .fetch_optional(&pool)
    .await
    .expect("stock")
    .unwrap_or(0.0);
    assert!(
        stock1.abs() < 0.001,
        "серверний stock без змін, маємо {stock1}"
    );

    // (б) товар = випадковий UUID (немає в products).
    let ghost_product = Uuid::new_v4();
    let p2 = invoice_payload(supplier, ghost_product, "3");
    // Локальний шлях каси такий документ НЕ створює (ADR-0007 §5 AT-14):
    // каталог каси порожній → людська відмова, жодного сліду в SQLite.
    {
        let mut conn = open_connection(&dir.path().join("invoice-at14-local.db")).expect("каса");
        let err = transactions::enqueue_invoice(&mut conn, &p2, &store.to_string())
            .expect_err("невідомий товар → відмова локально");
        assert!(
            err.contains(&ghost_product.to_string()) && err.contains("локальному каталозі"),
            "людське повідомлення про каталог, маємо: {err}"
        );
        for table in ["invoices", "invoice_items", "outbox", "stock"] {
            let n: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .expect("count");
            assert_eq!(n, 0, "AT-14: {table} без сліду відмови");
        }
    }
    // Серверний pre-flight перевіряємо на черзі, написаній БЕЗ локальної
    // валідації (стара версія каси): приймач мусить відхилити сам.
    let (db2, cu2) = build_cash_db_raw(&dir, "ghost-product", store, &p2);
    let res2 = push_raw(&db2, &base, &token, store, Some(&cu2)).await;
    eprintln!("[invoice e2e] ТЕСТ 2б (невідомий товар): {res2:?}");
    assert_eq!(res2[0]["status"], "error", "статус = error");
    let err2 = res2[0]["error"].as_str().unwrap_or_default().to_string();
    assert!(
        err2.contains("каталоз") && err2.contains("не знайдено"),
        "людське повідомлення про каталог, маємо: {err2}"
    );
    assert!(
        err2.contains(&ghost_product.to_string()),
        "у тексті — конкретний id товару: {err2}"
    );
    let n2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE client_uuid = $1")
        .bind(Uuid::parse_str(&cu2).expect("uuid"))
        .fetch_one(&pool)
        .await
        .expect("count 2");
    assert_eq!(n2, 0, "накладну НЕ створено");
    let orphan: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invoice_items ii JOIN invoices i ON i.id = ii.invoice_id \
         WHERE i.supplier_id = $1",
    )
    .bind(supplier)
    .fetch_one(&pool)
    .await
    .expect("orphan items");
    assert_eq!(orphan, 0, "«сирота»-позицій без накладної немає");

    // Клієнт каси: оп лишається в outbox (НЕ тихий ack) — каса бачить проблему.
    let client = reqwest::Client::new();
    let cfg = PushConfig {
        base_url: base.clone(),
        token: token.clone(),
        store_id: Some(store.to_string()),
        db_path: db2.clone(),
        interval_secs: 30,
    };
    let s = push_once(&db2, &client, &cfg).await;
    eprintln!("[invoice e2e] клієнт каси на error-відповідь: {s:?}");
    assert_eq!(s.failed, 1, "каса позначила оп failed (потребує уваги)");
    assert_eq!(s.done, 0, "тихого успіху немає");
    let conn = open_connection(&db2).expect("БД");
    let st: String = conn
        .query_row(
            "SELECT status FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![cu2],
            |r| r.get(0),
        )
        .expect("outbox status");
    assert_eq!(st, "failed", "оп лишився в outbox зі статусом failed");
    drop(conn);

    eprintln!(
        "[sync_invoice_push_e2e] ✅ ТЕСТ 2: невідомий постачальник/товар → error з людським текстом, 0 рядків, stock без змін"
    );
}
