//! E2E ЕТАП 7 (дизайн розділ 9): симуляція «4 каси × відмова мережі».
//!
//! Повний цикл офлайн-каси з РЕАЛЬНИМ серверним прийомом (як sync_push_e2e):
//! каса (SQLite, enqueue_receipt з client_uuid) → push_pending_batch
//! (POST /api/v1/sync/push) → сервер (PG receipts) → outbox done.
//!
//! Сценарій:
//!   Фаза 1 (outage): сервер ВИМКНЕНИЙ — 4 каси працюють офлайн: чеки
//!     (sale + return) накопичуються в outbox (pending росте), локальна
//!     закупка (purchase_order, ЕТАП 6) зберігається synced=0, у sync_log
//!     фіксується push_fail (мережева помилка).
//!   Фаза 2 (відновлення): сервер піднято → кожна каса вивантажує outbox
//!     (done); ПОВТОРНИЙ push тих самих client_uuid (скидання done→pending,
//!     симуляція «відповідь загубилась», дизайн 3.3) → already_exists.
//!   Фаза 3 (консистентність): на сервері РІВНО 1 запис на client_uuid
//!     (дублікатів 0) по всіх 4 касах; кількість чеків і сума total за точку
//!     сходяться з локально згенерованими (зведений агрегат відсутній —
//!     точність перевіряється по таблицях транзакцій; див. звіт ЕТАП 7).
//!
//! Детермінованість: локальний test-сервер (run_facade) на 127.0.0.1,
//! жодних зовнішніх мереж; PostgreSQL — як в інших e2e крейта.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use sqlx::Row;
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_push::{
    enqueue_receipt, open_connection, pending_count, push_pending_batch, PushConfig,
};
use torgashka_infrastructure::offline::transactions::{self, TYPE_PURCHASE_ORDER};
use uuid::Uuid;

/// 4 точки симуляції (випадкові UUID — ізоляція від паралельних e2e).
const STORES: usize = 4;

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

/// Seed: адмін + user_stores для 4 точок + продукт із залишком на точку.
async fn ensure_seed(pool: &sqlx::PgPool) -> Vec<(Uuid, Uuid)> {
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

    let mut out = Vec::new();
    for i in 0..STORES {
        let store = Uuid::new_v4();
        let product = Uuid::new_v4();
        sqlx::query("INSERT INTO stores (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(store)
            .bind(format!("E2E 4-каси Точка {i}"))
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
        sqlx::query(
            "INSERT INTO products (id, barcode, title, price, tax_rate) \
             VALUES ($1, NULL, 'E2E 4-каси Товар', 100.00, 20.00)",
        )
        .bind(product)
        .execute(pool)
        .await
        .expect("seed product");
        sqlx::query(
            "INSERT INTO stock (store_id, product_id, quantity, price) VALUES ($1, $2, 100000, 100.00)",
        )
        .bind(store)
        .bind(product)
        .execute(pool)
        .await
        .expect("seed stock");
        out.push((store, product));
    }
    out
}

/// Каса №k: НОВА SQLite-БД; N sale-чеків + R return-чеків у outbox;
/// 1 локальна закупка (purchase_order, synced=0 — ЕТАП 6).
///
/// Повертає (db_path, очікувана сума total на сервері після push).
fn build_cash_db(
    dir: &tempfile::TempDir,
    k: usize,
    store: Uuid,
    product: Uuid,
) -> (PathBuf, f64) {
    let db_path = dir.path().join(format!("cash-{k}.db"));
    let mut conn = open_connection(&db_path).expect("каса БД");

    let n_sale = 3 + k; // 3,4,5,6 чеки
    let n_return = if k % 2 == 0 { 1 } else { 0 }; // 2 каси з поверненням
    let mut expected_total = 0.0;

    for n in 0..n_sale {
        let qty = (n % 3) + 1; // 1..=3
        let total = 100.0 * qty as f64;
        let receipt = json!({
            "receipt_type": "sale",
            "receipt_number": format!("4S{k}-{n}"),
            "items": [{"product_id": product.to_string(), "quantity": qty, "price": "100.00"}],
            "total_amount": format!("{total:.2}"),
            "payment_method": "cash",
            "cash_amount": format!("{total:.2}"),
        })
        .to_string();
        enqueue_receipt(&mut conn, &receipt, Some(&store.to_string())).expect("enqueue sale");
        expected_total += total;
    }
    for n in 0..n_return {
        let total = 100.0;
        let receipt = json!({
            "receipt_type": "return",
            "receipt_number": format!("4R{k}-{n}"),
            "items": [{"product_id": product.to_string(), "quantity": 1, "price": "100.00"}],
            "total_amount": format!("{total:.2}"),
            "payment_method": "cash",
            "cash_amount": format!("{total:.2}"),
        })
        .to_string();
        enqueue_receipt(&mut conn, &receipt, Some(&store.to_string())).expect("enqueue return");
        expected_total += total;
    }
    // Локальна закупка (ЕТАП 6): запис synced=0, серверний push її не бере
    // (приймач sale/return; поширення — серверна частина ЕТАП 7+, у звіті).
    let po = json!({
        "number": format!("PO-{k}"),
        "items": [{"product_id": product.to_string(), "quantity": 10, "price": "80.00"}],
        "total_amount": "800.00",
    })
    .to_string();
    transactions::enqueue_transaction(&mut conn, TYPE_PURCHASE_ORDER, &po, &store.to_string())
        .expect("enqueue purchase_order");

    // Перевірки Фази 1 (каса працює офлайн, сервер ще вимкнений).
    let pending = pending_count(&conn).expect("pending count");
    assert_eq!(pending, n_sale + n_return, "каса {k}: outbox накопичує офлайн");
    let po_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM purchase_orders WHERE synced = 0", [], |r| r.get(0))
        .expect("purchase_orders");
    assert_eq!(po_rows, 1, "каса {k}: закупка збережена synced=0");
    // stock: sale −qty, return +qty (ЕТАП 6, локальний ефект).
    let sale_qty: i64 = (0..n_sale).map(|n| ((n % 3) + 1) as i64).sum();
    let return_qty: i64 = n_return as i64;
    let qty_milli: i64 = conn
        .query_row(
            "SELECT quantity FROM stock WHERE store_id = ?1 AND product_id = ?2",
            rusqlite::params![store.to_string(), product.to_string()],
            |r| r.get(0),
        )
        .expect("stock");
    assert_eq!(
        qty_milli,
        (10000 + return_qty * 1000 - sale_qty * 1000),
        "каса {k}: локальний stock = 10 (закупка) + повернення − продажі (міліодиниці)"
    );
    drop(conn);
    (db_path, expected_total)
}

/// push-цикл: повторює батчі, поки outbox не спорожніє (done/failed).
async fn flush_outbox(db_path: &Path, client: &reqwest::Client, cfg: &PushConfig) {
    for _ in 0..20 {
        let s = push_pending_batch(db_path, client, cfg)
            .await
            .expect("push (сервер увімкнено)");
        if s.sent == 0 {
            break;
        }
        let progressed = s.done > 0 || s.already_exists > 0;
        if !progressed {
            break; // deferred/failed — далі молотити нічого
        }
    }
    let conn = open_connection(db_path).expect("БД");
    assert_eq!(
        pending_count(&conn).expect("pending"),
        0,
        "після відновлення outbox спорожнів"
    );
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
                return v["access_token"].as_str().expect("access_token").to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("login: сервер не піднявся");
}

// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn four_stores_outage_then_sync_consistent() {
    let pool = api_pool().await;
    let seeds = ensure_seed(&pool).await;

    // ── Фаза 1: СЕРВЕР ВИМКНЕНИЙ — каси накопичують офлайн ────────────────
    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let dir = tempfile::TempDir::new().expect("tmpdir");

    let mut stores = Vec::new();
    for (k, (store, product)) in seeds.iter().enumerate() {
        let (db_path, expected_total) = build_cash_db(&dir, k, *store, *product);
        stores.push((*store, *product, db_path, expected_total));
    }

    // Кожна каса пробує push з вимкненим сервером → мережева помилка,
    // pending БЕЗ змін, у sync_log подія push_fail (моніторинг бачить).
    let client = reqwest::Client::new();
    for (store, _product, db_path, _expected) in &stores {
        let cfg = PushConfig {
            base_url: base.clone(),
            token: "неважливо".to_string(),
            store_id: store.to_string(),
            db_path: db_path.clone(),
            interval_secs: 30,
        };
        let err = push_pending_batch(db_path, &client, &cfg).await;
        assert!(err.is_err(), "вимкнений сервер → мережева помилка (каса {store})");
        let conn = open_connection(db_path).expect("БД");
        let fail_events: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_log WHERE kind = 'push_fail'", [], |r| r.get(0))
            .expect("sync_log push_fail");
        assert!(fail_events >= 1, "sync_log фіксує мережеву помилку push");
        drop(conn);
    }

    // ── Фаза 2: СЕРВЕР ПІДНЯТО — всі каси вивантажуються ──────────────────
    let _h = run_facade(&addr);
    let token = login(&base).await;

    for (store, _product, db_path, _expected) in &stores {
        let cfg = PushConfig {
            base_url: base.clone(),
            token: token.clone(),
            store_id: store.to_string(),
            db_path: db_path.clone(),
            interval_secs: 30,
        };
        flush_outbox(db_path, &client, &cfg).await;
    }

    // ── Фаза 3a: ідемпотентність — повторний push → already_exists, 0 дублікатів
    // (симуляція «відповідь загубилась після COMMIT сервера», дизайн 3.3).
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE client_uuid IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("count receipts");
    for (store, _product, db_path, _expected) in &stores {
        let conn = open_connection(db_path).expect("БД");
        conn.execute(
            "UPDATE outbox SET status = 'pending', next_attempt_at = datetime('now') \
             WHERE status = 'done'",
            [],
        )
        .expect("скинути done → pending для повтору");
        drop(conn);
        let cfg = PushConfig {
            base_url: base.clone(),
            token: token.clone(),
            store_id: store.to_string(),
            db_path: db_path.clone(),
            interval_secs: 30,
        };
        flush_outbox(db_path, &client, &cfg).await;
    }
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE client_uuid IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("count receipts after");
    assert_eq!(before, after, "КРИТЕРІЙ: повторний push не створив дублікатів");

    // ── Фаза 3b: консистентність даних по 4 касах ─────────────────────────
    // Зведеного ендпоінта-агрегата немає (див. звіт) — точність звіту
    // перевіряється по таблицях транзакцій: count і сума total за точку.
    for (store, _product, _db_path, expected_total) in &stores {
        let row = sqlx::query(
            "SELECT COUNT(*) AS n, COALESCE(SUM(total_amount::numeric), 0)::text AS sum \
             FROM receipts WHERE store_id = $1 AND client_uuid IS NOT NULL",
        )
        .bind(store)
        .fetch_one(&pool)
        .await
        .expect("зведені дані точки");
        let n: i64 = row.get("n");
        let sum: f64 = row.get::<String, _>("sum").parse().expect("sum");
        let dups: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) - COUNT(DISTINCT client_uuid) FROM receipts WHERE store_id = $1",
        )
        .bind(store)
        .fetch_one(&pool)
        .await
        .expect("дублікати");
        assert_eq!(dups, 0, "точка {store}: дублікатів 0");
        assert_eq!(n, expected_n(store, &seeds).await, "точка {store}: кількість чеків");
        assert!(
            (sum - expected_total).abs() < 0.01,
            "точка {store}: сума total на сервері ({sum}) == згенерована ({expected_total})"
        );
    }
    eprintln!("[sync_4stores_outage_e2e] ✅ 4 каси × outage: 0 дублікатів, суми сходяться");
}

/// Очікувана кількість чеків точки на сервері (sale + return).
async fn expected_n(store: &Uuid, seeds: &[(Uuid, Uuid)]) -> i64 {
    let k = seeds.iter().position(|(s, _)| s == store).expect("каса");
    (3 + k) as i64 + if k % 2 == 0 { 1 } else { 0 }
}
