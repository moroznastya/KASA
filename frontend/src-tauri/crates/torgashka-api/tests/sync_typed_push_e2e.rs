//! E2E ЕТАП 7b (БЛОКЕР 1 + 2): push не-чекових типів каси ЕТАПУ 6.
//!
//! Сценарій: 4 каси (SQLite) створюють офлайн ВСІ типи операцій ЕТАПУ 6 —
//! purchase_order, inventory, transfer, write_off (кожен атомарно:
//! агрегат synced=1 + outbox pending + локальний stock-ефект). Сервер
//! (run_facade) приймає через /api/v1/sync/push → документи confirmed +
//! stock-ефекти в серверному stock. Перевірки:
//!   * «всі дані на сервері»: по 1 запису кожного типу на кожну точку;
//!   * дублікатів 0 (COUNT == COUNT DISTINCT client_uuid); повторний push
//!     (done→pending) → already_exists, кількість не змінилась;
//!   * created_at (БЛОКЕР 2) = created_at каси (RFC3339→UTC), не now();
//!   * stock-ефекти: inventory (абсолют) → purchase +qty → write_off −qty →
//!     transfer ±qty за стороною каси.
//!
//! Детермінованість: локальний test-сервер (run_facade), PostgreSQL —
//! ІЗОЛЬОВАНА _test БД (common::force_test_db, гігієна QA §5.2).

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use sqlx::Row;
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_push::{
    open_connection, pending_count, push_pending_batch, PushConfig,
};
use torgashka_infrastructure::offline::transactions::{
    self, TYPE_INVENTORY, TYPE_PURCHASE_ORDER, TYPE_TRANSFER, TYPE_WRITE_OFF,
};
use uuid::Uuid;

const STORES: usize = 4;
/// Стара дата каси (багатоденний офлайн) — сервер має зберегти її як є.
const TS_OLD: &str = "2026-08-30T10:00:00+03:00"; // = 07:00:00Z

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

async fn api_pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool")
}

/// Seed: адмін + 4 точки + постачальник (продукти — окремо в ensure_product).
async fn ensure_seed(pool: &sqlx::PgPool) -> (Vec<Uuid>, Uuid) {
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
        .bind("E2E Types Постачальник")
        .execute(pool)
        .await
        .expect("seed supplier");
    let mut stores = Vec::new();
    for i in 0..STORES {
        let store = Uuid::new_v4();
        sqlx::query("INSERT INTO stores (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(store)
            .bind(format!("E2E Types Точка {i}"))
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
        stores.push(store);
    }
    (stores, supplier)
}

async fn ensure_product(pool: &sqlx::PgPool) -> Uuid {
    let product = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, barcode, title, price, tax_rate) \
         VALUES ($1, NULL, 'E2E Types Товар', 100.00, 20.00)",
    )
    .bind(product)
    .execute(pool)
    .await
    .expect("seed product");
    product
}

fn fix_outbox_created_at(conn: &rusqlite::Connection, client_uuid: &str, created_at: &str) {
    let payload: String = conn
        .query_row(
            "SELECT payload FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![client_uuid],
            |r| r.get(0),
        )
        .expect("outbox payload");
    let mut v: serde_json::Value = serde_json::from_str(&payload).expect("payload JSON");
    v["created_at"] = serde_json::Value::String(created_at.to_string());
    conn.execute(
        "UPDATE outbox SET payload = ?1 WHERE client_uuid = ?2",
        rusqlite::params![v.to_string(), client_uuid],
    )
    .expect("update payload");
}

/// Каса №k: 4 агрегати (purchase/inventory/transfer/write_off) в outbox.
fn build_cash_db(
    dir: &tempfile::TempDir,
    k: usize,
    store: Uuid,
    other: Uuid,
    product: Uuid,
    supplier: Uuid,
) -> PathBuf {
    let db_path = dir.path().join(format!("typed-{k}.db"));
    let mut conn = open_connection(&db_path).expect("каса БД");
    let sid = store.to_string();

    // 1) inventory: АБСОЛЮТНИЙ факт 5 шт (найперший — далі purchase/write/transfer).
    let inv = json!({
        "location": format!("Точка {k}"),
        "inventory_date": "2026-08-30T08:00:00+03:00",
        "notes": "ЕТАП 7b e2e",
        "items": [{
            "product_id": product.to_string(),
            "quantity": 5,
            "actual_quantity": 5,
            "accounting_quantity": 4,
            "difference": 1,
            "cost_price": "60.00",
            "price": "100.00",
        }],
    })
    .to_string();
    let o = transactions::enqueue_transaction(&mut conn, TYPE_INVENTORY, &inv, &sid)
        .expect("inventory");
    fix_outbox_created_at(&conn, &o.client_uuid, TS_OLD);

    // 2) purchase: надходження +10.
    let po = json!({
        "supplier_id": supplier.to_string(),
        "order_date": "2026-08-30T09:00:00+03:00",
        "items": [{"product_id": product.to_string(), "quantity": 10, "price": "80.00",
                   "total": "800.00"}],
        "total_amount": "800.00",
    })
    .to_string();
    let o = transactions::enqueue_transaction(&mut conn, TYPE_PURCHASE_ORDER, &po, &sid)
        .expect("purchase");
    fix_outbox_created_at(&conn, &o.client_uuid, TS_OLD);

    // 3) write_off: −2.
    let wo = json!({
        "reason": "псування (e2e)",
        "write_off_date": "2026-08-30T10:00:00+03:00",
        "items": [{"product_id": product.to_string(), "quantity": 2, "cost_price": "60.00",
                   "price": "100.00"}],
    })
    .to_string();
    let o = transactions::enqueue_transaction(&mut conn, TYPE_WRITE_OFF, &wo, &sid)
        .expect("write_off");
    fix_outbox_created_at(&conn, &o.client_uuid, TS_OLD);

    // 4) transfer: парні каси — out (−4 у свою точку), непарні — in (+4).
    let (from, to) = if k % 2 == 0 {
        (store, other)
    } else {
        (other, store)
    };
    let tr = json!({
        "from_store_id": from.to_string(),
        "to_store_id": to.to_string(),
        "notes": "e2e transfer",
        "items": [{"product_id": product.to_string(), "quantity": 4, "cost_price": "60.00",
                   "price": "100.00"}],
    })
    .to_string();
    let o = transactions::enqueue_transaction(&mut conn, TYPE_TRANSFER, &tr, &sid)
        .expect("transfer");
    fix_outbox_created_at(&conn, &o.client_uuid, TS_OLD);

    // Кожен агрегат в outbox (pending) — «не synced=0 в нікуди».
    assert_eq!(pending_count(&conn).expect("pending"), 4, "каса {k}: 4 агрегати в outbox");
    drop(conn);
    db_path
}

async fn flush_outbox(db_path: &Path, client: &reqwest::Client, cfg: &PushConfig) {
    for _ in 0..20 {
        let s = push_pending_batch(db_path, client, cfg).await.expect("push");
        if s.sent == 0 {
            break;
        }
        if !(s.done > 0 || s.already_exists > 0) {
            break;
        }
    }
    let conn = open_connection(db_path).expect("БД");
    assert_eq!(pending_count(&conn).expect("pending"), 0, "outbox спорожнів");
    drop(conn);
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

#[tokio::test]
async fn typed_operations_push_idempotent_and_consistent() {
    common::force_test_db();
    let pool = api_pool().await;
    let (stores, supplier) = ensure_seed(&pool).await;
    let product = ensure_product(&pool).await;

    // Каси створюють агрегати ОФЛАЙН (сервер ще вимкнений).
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let mut dbs = Vec::new();
    for k in 0..STORES {
        let other = stores[(k + 1) % STORES];
        let db = build_cash_db(&dir, k, stores[k], other, product, supplier);
        dbs.push(db);
    }

    // ── Фаза 2: сервер піднято → усі каси вивантажуються ──────────────────
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;
    let client = reqwest::Client::new();

    for (k, db) in dbs.iter().enumerate() {
        let cfg = PushConfig {
            base_url: base.clone(),
            token: token.clone(),
            store_id: stores[k].to_string(),
            db_path: db.clone(),
            interval_secs: 30,
        };
        flush_outbox(db, &client, &cfg).await;
    }

    // ── Фаза 3: «всі дані на сервері», дублікатів 0 ────────────────────────
    for (k, store) in stores.iter().enumerate() {
        for (table, entity) in [
            ("inventories", "inventory"),
            ("purchase_orders", "purchase_order"),
            ("write_offs", "write_off"),
            ("transfers", "transfer"),
        ] {
            let n: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM {table} WHERE store_id = $1 AND client_uuid IS NOT NULL"
            ))
            .bind(store)
            .fetch_one(&pool)
            .await
            .expect("count");
            assert_eq!(n, 1, "точка {k}: {entity} доставлено рівно 1 раз");
            let dups: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) - COUNT(DISTINCT client_uuid) FROM {table} WHERE store_id = $1"
            ))
            .bind(store)
            .fetch_one(&pool)
            .await
            .expect("dups");
            assert_eq!(dups, 0, "точка {k}: {entity} без дублікатів");
        }
    }

    // Статуси — confirmed (касовий факт, не чернетка власника).
    for (table, col) in [
        ("inventories", "status::text"),
        ("purchase_orders", "status::text"),
        ("transfers", "status::text"),
    ] {
        let statuses: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT {col} FROM {table} WHERE store_id = ANY($1)"
        ))
        .bind(&stores)
        .fetch_all(&pool)
        .await
        .expect("statuses");
        assert_eq!(statuses.len(), STORES, "{table}: усі точки");
        assert!(
            statuses.iter().all(|s| s == "confirmed"),
            "{table}: статуси = confirmed, маємо {statuses:?}"
        );
    }

    // ── Фаза 4: created_at (БЛОКЕР 2) = created_at каси, не now() ──────────
    for (table, expected_utc) in [
        ("inventories", "2026-08-30T07:00:00"),
        ("purchase_orders", "2026-08-30T07:00:00"),
        ("write_offs", "2026-08-30T07:00:00"),
        ("transfers", "2026-08-30T07:00:00"),
    ] {
        let rows: Vec<chrono::NaiveDateTime> = sqlx::query_scalar(&format!(
            "SELECT created_at FROM {table} WHERE store_id = ANY($1) \
             AND client_uuid IS NOT NULL ORDER BY created_at"
        ))
        .bind(&stores)
        .fetch_all(&pool)
        .await
        .expect("created_at");
        assert_eq!(rows.len(), STORES, "{table}: created_at усіх точок");
        for r in &rows {
            assert_eq!(
                r.format("%Y-%m-%dT%H:%M:%S").to_string(),
                expected_utc,
                "{table}: created_at = created_at каси (RFC3339→UTC), не now()"
            );
        }
    }

    // ── Фаза 5: stock-ефекти (та сама таблиця stock сервера) ───────────────
    // Порядок застосування (FIFO enqueue): inventory(абсолют 5) → purchase(+10)
    // → write_off(−2) → transfer(out: −4; in: +4).
    for (k, store) in stores.iter().enumerate() {
        let is_out = k % 2 == 0;
        let expected: i64 = if is_out { 9 } else { 17 };
        let srv: f64 = sqlx::query_scalar::<_, f64>(
            "SELECT quantity::float8 FROM stock WHERE store_id = $1 AND product_id = $2",
        )
        .bind(store)
        .bind(product)
        .fetch_one(&pool)
        .await
        .expect("stock");
        assert!(
            (srv - expected as f64).abs() < 0.001,
            "точка {k}: серверний stock {srv} == {expected} \
             (inventory 5 + purchase 10 − write_off 2 ± transfer 4)"
        );
    }

    // ── Фаза 6: ідемпотентність — повторний push → already_exists ─────────
    let before_total: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM inventories WHERE client_uuid IS NOT NULL) \
         + (SELECT COUNT(*) FROM purchase_orders WHERE client_uuid IS NOT NULL) \
         + (SELECT COUNT(*) FROM write_offs WHERE client_uuid IS NOT NULL) \
         + (SELECT COUNT(*) FROM transfers WHERE client_uuid IS NOT NULL)",
    )
    .fetch_one(&pool)
    .await
    .expect("total before");
    for (k, db) in dbs.iter().enumerate() {
        let conn = open_connection(db).expect("БД");
        conn.execute(
            "UPDATE outbox SET status = 'pending', next_attempt_at = datetime('now') \
             WHERE status = 'done'",
            [],
        )
        .expect("reset done→pending");
        drop(conn);
        let cfg = PushConfig {
            base_url: base.clone(),
            token: token.clone(),
            store_id: stores[k].to_string(),
            db_path: db.clone(),
            interval_secs: 30,
        };
        flush_outbox(db, &client, &cfg).await;
    }
    let after_total: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM inventories WHERE client_uuid IS NOT NULL) \
         + (SELECT COUNT(*) FROM purchase_orders WHERE client_uuid IS NOT NULL) \
         + (SELECT COUNT(*) FROM write_offs WHERE client_uuid IS NOT NULL) \
         + (SELECT COUNT(*) FROM transfers WHERE client_uuid IS NOT NULL)",
    )
    .fetch_one(&pool)
    .await
    .expect("total after");
    assert_eq!(
        before_total, after_total,
        "КРИТЕРІЙ: повторний push усіх типів → already_exists, 0 дублікатів"
    );
    eprintln!("[sync_typed_push_e2e] ✅ 4 каси × 4 типи: 0 дублікатів, created_at з каси, stock точний");
}
