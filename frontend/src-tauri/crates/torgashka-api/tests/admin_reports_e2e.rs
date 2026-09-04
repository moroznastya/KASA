//! E2E: Звітність мережі (Етап 4, ТЗ 5.5/5.6).
//!
//! Реальні ендпоінти torgashka-api фасаду + PostgreSQL:
//!   GET /api/v1/admin/reports/network-sales?from=&to=
//!   GET /api/v1/admin/reports/cash-operations?from=&to=
//!   GET /api/v1/admin/reports/supplier-ledger?from=&to=
//!
//! Сценарій (критерій прийняття Етапа 4): 2 активні точки + 1 архівна,
//! чеки sale/return, cash_operations deposit/collection, supplier_ledger
//! записи → агрегати збігаються з ручною арифметикою; архівна точка
//! виключена; пустий період = 0 (не помилка); cashier → 403.
//!
//! Гігієна: на початку тест ОЧИЩУЄ дані тестової БД (TRUNCATE ... CASCADE) —
//! агрегати по мережі глобальні, повторні запуски мають бути детерміновані.
//! Схема — ensure_schema (ідемпотентно, спільний з admin_network_e2e).

use std::time::Duration;

use serde_json::{json, Value};
use torgashka_api::run_facade;
use uuid::Uuid;

mod common;

static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn apply_schema() {
    SCHEMA_ONCE
        .get_or_init(|| async {
            let p = torgashka_infrastructure::db::connect_test_pool(5)
                .await
                .expect("тестова БД недоступна");
            torgashka_infrastructure::db::ensure_schema(&p)
                .await
                .expect("ensure_schema на тестовій БД");
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

/// bcrypt('admin123') — спільний seed-пароль e2e torgashka-api.
const PWD: &str = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e";

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

async fn req(base: &str, path: &str, token: Option<&str>) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut r = client.get(format!("{base}{path}"));
    if let Some(t) = token {
        r = r.bearer_auth(t);
    }
    let resp = r.send().await.expect("запит");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// '1234.50' / '0' / '1050.00' → копійки (порівняння грошей без float).
fn cents(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() || s == "-" {
        return 0;
    }
    let (neg, s) = match s.strip_prefix('-') {
        Some(stripped) => (true, stripped),
        None => (false, s),
    };
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let mut frac = frac_part.to_string();
    while frac.len() < 2 {
        frac.push('0');
    }
    frac.truncate(2);
    let v: i64 = int_part.parse().unwrap_or(0) * 100 + frac.parse().unwrap_or(0);
    if neg {
        -v
    } else {
        v
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Сценарій: 2 активні точки + архівна; sale/return; каса; постачальники.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn network_reports_aggregates_are_correct_and_rbac_403() {
    common::force_test_db();
    let pool = api_pool().await;
    apply_schema().await;
    let tag = Uuid::new_v4().simple().to_string();

    // Повне очищення тестової БД (агрегати мережі глобальні).
    sqlx::query(
        "TRUNCATE devices, user_stores, stores, products, suppliers, users, \
         cash_operations, receipt_items, receipts, supplier_ledger \
         RESTART IDENTITY CASCADE",
    )
    .execute(&pool)
    .await
    .expect("truncate тестової БД");

    // ─── Точки: A, B — активні; C — архівна (має бути ВИКЛЮЧЕНА) ──────────
    let store_a = Uuid::new_v4();
    let store_b = Uuid::new_v4();
    let store_c = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stores (id, name, is_active, created_at, updated_at)
         VALUES ($1,'Точка А',true,now(),now()), ($2,'Точка Б',true,now(),now()),
                ($3,'Точка В (архів)',false,now(),now())",
    )
    .bind(store_a)
    .bind(store_b)
    .bind(store_c)
    .execute(&pool)
    .await
    .expect("seed stores");

    // ─── Товари ─────────────────────────────────────────────────────────────
    let p1 = Uuid::new_v4();
    let p2 = Uuid::new_v4();
    let p3 = Uuid::new_v4();
    let p9 = Uuid::new_v4();
    for (id, name) in [
        (p1, "Кава 200г"),
        (p2, "Молоко 1л"),
        (p3, "Хліб"),
        (p9, "Товар архівної точки"),
    ] {
        sqlx::query("INSERT INTO products (id, title) VALUES ($1, $2)")
            .bind(id)
            .bind(name)
            .execute(&pool)
            .await
            .expect("seed product");
    }

    // ─── Користувачі: owner (токен для звітів), cashier (RBAC 403) ─────────
    let owner_login = format!("rep_e2e_owner_{tag}");
    let cashier_login = format!("rep_e2e_cashier_{tag}");
    for (login, role) in [(&owner_login, "owner"), (&cashier_login, "cashier")] {
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
             VALUES ($1, 'E2E Reports', $2, $3, $4::public.user_role, true, now(), now(), true)
             ON CONFLICT (login) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(login)
        .bind(PWD)
        .bind(role)
        .execute(&pool)
        .await
        .expect("seed user");
    }
    let cashier_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = $1")
        .bind(&cashier_login)
        .fetch_one(&pool)
        .await
        .expect("cashier id");

    // ─── Чеки 2026-01-15 (created_at наївний UTC, як пише застосунок) ───────
    // Точка А: sale 100.00 (P1=60, P2=40); return 30.00 (P1=30).
    // Точка Б: sale 50.00 (P1=20, P3=30).
    // Точка В (архів): sale 999.00 (P9) — НЕ має увійти у звіт.
    async fn insert_receipt(
        pool: &sqlx::PgPool,
        id: Uuid,
        store: Uuid,
        cashier: Uuid,
        typ: &str,
        total: &str,
        is_return: bool,
        tag: &str,
    ) {
        sqlx::query(
            "INSERT INTO receipts (id, receipt_number, receipt_type, cashier_id, total_amount,
                                   is_return, store_id, created_at, payment_method)
             VALUES ($1, $2, $3::public.receipt_type, $4, $5::numeric,
                     $6, $7, $8::text::timestamp, 'cash'::public.receipt_payment_method)",
        )
        .bind(id)
        .bind(format!("R-{}-{}", tag, typ))
        .bind(typ)
        .bind(cashier)
        .bind(total)
        .bind(is_return)
        .bind(store)
        .bind("2026-01-15 10:00:00")
        .execute(pool)
        .await
        .expect("seed receipt");
    }
    async fn insert_item(
        pool: &sqlx::PgPool,
        receipt: Uuid,
        store: Uuid,
        product: Uuid,
        total: &str,
    ) {
        sqlx::query(
            "INSERT INTO receipt_items (id, receipt_id, product_id, quantity, price, total,
                                        store_id, created_at)
             VALUES ($1, $2, $3, 1::numeric, $4::numeric, $5::numeric, $6,
                     '2026-01-15 10:00:00'::timestamp)",
        )
        .bind(Uuid::new_v4())
        .bind(receipt)
        .bind(product)
        .bind(total)
        .bind(total)
        .bind(store)
        .execute(pool)
        .await
        .expect("seed receipt item");
    }

    let r1 = Uuid::new_v4();
    let r2 = Uuid::new_v4();
    let r3 = Uuid::new_v4();
    let r4 = Uuid::new_v4();
    insert_receipt(
        &pool, r1, store_a, cashier_id, "sale", "100.00", false, &tag,
    )
    .await;
    insert_receipt(
        &pool, r2, store_a, cashier_id, "return", "30.00", true, &tag,
    )
    .await;
    insert_receipt(&pool, r3, store_b, cashier_id, "sale", "50.00", false, &tag).await;
    insert_receipt(
        &pool, r4, store_c, cashier_id, "sale", "999.00", false, &tag,
    )
    .await;
    insert_item(&pool, r1, store_a, p1, "60.00").await;
    insert_item(&pool, r1, store_a, p2, "40.00").await;
    insert_item(&pool, r2, store_a, p1, "30.00").await;
    insert_item(&pool, r3, store_b, p1, "20.00").await;
    insert_item(&pool, r3, store_b, p3, "30.00").await;
    insert_item(&pool, r4, store_c, p9, "999.00").await;

    // ─── Каса (timestamptz UTC, 2026-01-15) ─────────────────────────────────
    async fn insert_cash_op(
        pool: &sqlx::PgPool,
        store: Uuid,
        user: Uuid,
        op: &str,
        amount: &str,
        ts: &str,
    ) {
        sqlx::query(
            "INSERT INTO cash_operations (store_id, user_id, operation_type, cash_type, amount,
                                          comment, created_at)
             VALUES ($1, $2, $3, $4, $5::numeric, 'e2e', $6::text::timestamptz)",
        )
        .bind(store)
        .bind(user)
        .bind(op)
        .bind("cash")
        .bind(amount)
        .bind(ts)
        .execute(pool)
        .await
        .expect("seed cash op");
    }
    // Точка А: deposit 1000 (cash) + deposit 50 (card) + collection 200 (card).
    insert_cash_op(
        &pool,
        store_a,
        cashier_id,
        "deposit",
        "1000.00",
        "2026-01-15T12:00:00Z",
    )
    .await;
    insert_cash_op(
        &pool,
        store_a,
        cashier_id,
        "deposit",
        "50.00",
        "2026-01-15T13:00:00Z",
    )
    .await;
    insert_cash_op(
        &pool,
        store_a,
        cashier_id,
        "collection",
        "200.00",
        "2026-01-15T14:00:00Z",
    )
    .await;
    // Точка Б: collection 300.
    insert_cash_op(
        &pool,
        store_b,
        cashier_id,
        "collection",
        "300.00",
        "2026-01-15T12:30:00Z",
    )
    .await;
    // Точка В (архів): deposit 99999 — НЕ увійде.
    insert_cash_op(
        &pool,
        store_c,
        cashier_id,
        "deposit",
        "99999.00",
        "2026-01-15T12:00:00Z",
    )
    .await;

    // ─── Постачальники (спільні на мережу; supplier_ledger без store_id) ────
    let s1 = Uuid::new_v4();
    let s2 = Uuid::new_v4();
    for (id, name) in [(s1, "Постачальник 1"), (s2, "Постачальник 2")] {
        sqlx::query("INSERT INTO suppliers (id, name) VALUES ($1, $2)")
            .bind(id)
            .bind(name)
            .execute(&pool)
            .await
            .expect("seed supplier");
    }
    async fn insert_ledger(
        pool: &sqlx::PgPool,
        supplier: Uuid,
        op: &str,
        amount: &str,
        balance_after: &str,
        doc_num: &str,
    ) {
        sqlx::query(
            "INSERT INTO supplier_ledger (id, supplier_id, operation_type, document_number,
                                          amount, balance_after, operation_date, created_at)
             VALUES ($1, $2, $3::public.ledger_operation_type, $4, $5::numeric,
                     $6::numeric, '2026-01-15 09:00:00'::timestamp, now())",
        )
        .bind(Uuid::new_v4())
        .bind(supplier)
        .bind(op)
        .bind(doc_num)
        .bind(amount)
        .bind(balance_after)
        .execute(pool)
        .await
        .expect("seed ledger");
    }
    // S1: invoice +1000 (balance 1000), payment -400 (balance 600).
    insert_ledger(&pool, s1, "invoice", "1000.00", "1000.00", "INV-1").await;
    insert_ledger(&pool, s1, "payment", "-400.00", "600.00", "PAY-1").await;
    // S2: invoice +500 (balance 500).
    insert_ledger(&pool, s2, "invoice", "500.00", "500.00", "INV-2").await;

    // ─── Фасад ───────────────────────────────────────────────────────────────
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let _h = run_facade(&format!("127.0.0.1:{port}"));
    wait_ready(&base).await;
    let owner = login(&base, &owner_login).await;
    let cashier = login(&base, &cashier_login).await;
    let period = "from=2026-01-14&to=2026-01-16";

    // ═══ 1. network-sales ═══════════════════════════════════════════════════
    let (s, v) = req(
        &base,
        &format!("/api/v1/admin/reports/network-sales?{period}"),
        Some(&owner),
    )
    .await;
    assert_eq!(s, 200, "network-sales: {v}");
    let stores = v["stores"].as_array().expect("stores").clone();
    assert_eq!(stores.len(), 2, "тільки 2 активні точки: {v}");
    let by_name = |n: &str| {
        stores
            .iter()
            .find(|x| x["store_name"] == json!(n))
            .unwrap_or_else(|| panic!("точка {n} у звіті"))
    };
    let a = by_name("Точка А");
    assert_eq!(cents(a["sales"].as_str().unwrap()), 10000, "А sales: {a}");
    assert_eq!(
        cents(a["returns"].as_str().unwrap()),
        3000,
        "А returns: {a}"
    );
    assert_eq!(cents(a["net_sales"].as_str().unwrap()), 7000, "А net: {a}");
    assert_eq!(a["sales_checks"], 1);
    assert_eq!(a["returns_checks"], 1);
    let b = by_name("Точка Б");
    assert_eq!(cents(b["sales"].as_str().unwrap()), 5000, "Б sales: {b}");
    assert_eq!(cents(b["returns"].as_str().unwrap()), 0, "Б returns: {b}");
    assert_eq!(cents(b["net_sales"].as_str().unwrap()), 5000, "Б net: {b}");
    assert_eq!(b["sales_checks"], 1);
    assert_eq!(b["returns_checks"], 0);

    let t = &v["totals"];
    assert_eq!(cents(t["sales"].as_str().unwrap()), 15000, "totals: {v}");
    assert_eq!(cents(t["returns"].as_str().unwrap()), 3000, "totals: {v}");
    assert_eq!(
        cents(t["net_sales"].as_str().unwrap()),
        12000,
        "totals: {v}"
    );
    assert_eq!(t["sales_checks"], 2, "sale-чеки мережі: {v}");
    assert_eq!(t["returns_checks"], 1, "return-чеки мережі: {v}");

    // Топ товарів: P1 = 60-30+20 = 50; P2 = 40; P3 = 30. Товар архівної точки
    // (999) ВИКЛЮЧЕНО.
    let top = v["top_products"].as_array().expect("top_products").clone();
    assert_eq!(top.len(), 3, "топ містить 3 товари: {v}");
    assert_eq!(top[0]["product_name"], json!("Кава 200г"));
    assert_eq!(cents(top[0]["total"].as_str().unwrap()), 5000, "P1: {v}");
    assert_eq!(cents(top[1]["total"].as_str().unwrap()), 4000, "P2: {v}");
    assert_eq!(cents(top[2]["total"].as_str().unwrap()), 3000, "P3: {v}");

    // Пустий період (дати без даних) → 0, не помилка.
    let (s0, v0) = req(
        &base,
        "/api/v1/admin/reports/network-sales?from=2020-01-01&to=2020-01-02",
        Some(&owner),
    )
    .await;
    assert_eq!(s0, 200, "пустий період: {v0}");
    assert_eq!(
        cents(v0["totals"]["net_sales"].as_str().unwrap()),
        0,
        "{v0}"
    );
    assert!(
        v0["stores"].as_array().expect("масив").len() == 2,
        "точки є, грошей 0"
    );

    // ═══ 2. cash-operations ═════════════════════════════════════════════════
    let (s, v) = req(
        &base,
        &format!("/api/v1/admin/reports/cash-operations?{period}"),
        Some(&owner),
    )
    .await;
    assert_eq!(s, 200, "cash-operations: {v}");
    let stores = v["stores"].as_array().expect("stores").clone();
    assert_eq!(stores.len(), 2, "2 активні точки: {v}");
    let a = stores
        .iter()
        .find(|x| x["store_name"] == json!("Точка А"))
        .expect("А");
    assert_eq!(
        cents(a["deposit"].as_str().unwrap()),
        105000,
        "deposit А: {a}"
    );
    assert_eq!(
        cents(a["collection"].as_str().unwrap()),
        20000,
        "collection А: {a}"
    );
    assert_eq!(a["operations"], 3, "операцій А: {a}");
    let b = stores
        .iter()
        .find(|x| x["store_name"] == json!("Точка Б"))
        .expect("Б");
    assert_eq!(cents(b["deposit"].as_str().unwrap()), 0, "deposit Б: {b}");
    assert_eq!(
        cents(b["collection"].as_str().unwrap()),
        30000,
        "collection Б: {b}"
    );
    assert_eq!(b["operations"], 1);
    let t = &v["totals"];
    assert_eq!(cents(t["deposit"].as_str().unwrap()), 105000, "totals: {v}");
    assert_eq!(
        cents(t["collection"].as_str().unwrap()),
        50000,
        "totals: {v}"
    );
    assert_eq!(t["operations"], 4, "операцій мережі: {v}");

    // ═══ 3. supplier-ledger ═════════════════════════════════════════════════
    let (s, v) = req(&base, "/api/v1/admin/reports/supplier-ledger", Some(&owner)).await;
    assert_eq!(s, 200, "supplier-ledger: {v}");
    let rows = v["suppliers"].as_array().expect("suppliers").clone();
    assert_eq!(rows.len(), 2, "2 постачальники: {v}");
    let s1row = rows
        .iter()
        .find(|x| x["supplier_name"] == json!("Постачальник 1"))
        .expect("S1");
    assert_eq!(s1row["period_operations"], 2);
    assert_eq!(
        cents(s1row["period_inflow"].as_str().unwrap()),
        100000,
        "S1 inflow: {v}"
    );
    assert_eq!(
        cents(s1row["period_outflow"].as_str().unwrap()),
        40000,
        "S1 outflow: {v}"
    );
    assert_eq!(
        cents(s1row["period_net"].as_str().unwrap()),
        60000,
        "S1 net: {v}"
    );
    assert_eq!(
        cents(s1row["current_balance"].as_str().unwrap()),
        60000,
        "S1 balance: {v}"
    );
    let s2row = rows
        .iter()
        .find(|x| x["supplier_name"] == json!("Постачальник 2"))
        .expect("S2");
    assert_eq!(s2row["period_operations"], 1);
    assert_eq!(cents(s2row["period_inflow"].as_str().unwrap()), 50000);
    assert_eq!(cents(s2row["period_outflow"].as_str().unwrap()), 0);
    assert_eq!(cents(s2row["current_balance"].as_str().unwrap()), 50000);
    let t = &v["totals"];
    assert_eq!(cents(t["inflow"].as_str().unwrap()), 150000, "totals: {v}");
    assert_eq!(cents(t["outflow"].as_str().unwrap()), 40000, "totals: {v}");
    assert_eq!(cents(t["net"].as_str().unwrap()), 110000, "totals: {v}");
    assert_eq!(cents(t["balance"].as_str().unwrap()), 110000, "баланс: {v}");

    // З фільтром по періоду — той самий оборот (усі операції 2026-01-15).
    let (s, v) = req(
        &base,
        &format!("/api/v1/admin/reports/supplier-ledger?{period}"),
        Some(&owner),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v["suppliers"].as_array().expect("sup").len(), 2);
    assert_eq!(
        cents(v["suppliers"][0]["period_inflow"].as_str().unwrap()) > 0,
        true
    );

    // ═══ 4. RBAC: cashier → 403 на всіх /admin/reports/* ════════════════════
    for path in [
        "/api/v1/admin/reports/network-sales?from=2026-01-14&to=2026-01-16",
        "/api/v1/admin/reports/cash-operations?from=2026-01-14&to=2026-01-16",
        "/api/v1/admin/reports/supplier-ledger",
    ] {
        let (cs, cv) = req(&base, path, Some(&cashier)).await;
        assert_eq!(cs, 403, "cashier 403 на {path}: {cv}");
    }

    // ═══ 5. from>to → 400 (валідація періоду) ═══════════════════════════════
    let (bs, bv) = req(
        &base,
        "/api/v1/admin/reports/network-sales?from=2026-02-01&to=2026-01-01",
        Some(&owner),
    )
    .await;
    assert_eq!(bs, 400, "from>to → 400: {bv}");
}
