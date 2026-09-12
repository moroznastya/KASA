//! E2E: push-kinds БАТЬКІВСЬКИХ сутностей (ADR-0008 §7.1-B) + ідемпотентність.
//!
//! Доведений прод-дефект (фіксується в `docs/audit/adr0008-baseline.md` як
//! baseline-доказ ДО фіксу):
//!   * kind `debtor` відсутній у `receiver_table` (`sync.rs`) → картка боржника,
//!     створена на вузлі, НЕ доїжджає на хаб;
//!   * вузол віддає дитину `debtor_payment` (kind Є у `receiver_table`) — хаб
//!     відхиляє її: `accept_debtor_payment` (`sync_receivers.rs`) не знаходить
//!     батька → «Боржника … не знайдено в цій точці — оплату відхилено»;
//!   * kind `work_session` вузол УЖЕ кладе в outbox
//!     (`offline/transactions.rs::open_work_session`, `TYPE_WORK_SESSION`),
//!     але хаб його не приймає (kind відсутній у `receiver_table`);
//!   * kind `prro_shift` (аудит фіскалізації по точках, ADR-0008 §7.1-B3).
//!
//! Після E1: усі три kinds приймаються ідемпотентно (partial UNIQUE на
//! `client_uuid`, Alembic 0019) — повторний push → `already_exists`, дублів 0.
//!
//! Порядок «батько → дитина» перевіряє сам сценарій: `debtor` іде в батчі
//! ПЕРШИМ, платіж посилається на `id` батька — той самий UUID, що згенерував
//! вузол локально (ідентичність зберігається, FK `debtor_payments.debtor_id`
//! резолвиться). Топосорт батча за DAG — етап E2b; тут потрібно лише, щоб
//! батько був у наборі прийнятних kinds.

mod common;

use std::time::Duration;

use serde_json::{json, Value};
use torgashka_api::run_facade;
use uuid::Uuid;

/// Тестова точка (seed онбордингу — та сама, що в sync_push_e2e).
const STORE1: &str = "d9be9608-c011-49be-b776-3317ca5e9af6";

#[path = "common/sync_schema.rs"]
mod sync_schema;

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
            // Sync-шар (Alembic 0011–0019): server_version, sync_meta,
            // soft-delete, client_uuid-приймачі — ensure_schema (schema.sql)
            // для мігрованих БД їх не додає.
            sync_schema::apply(&p).await;
            p.close().await;
        })
        .await;
}

async fn api_pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_readonly_pool(2)
        .await
        .expect("pool")
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

/// Seed: точка + адмін + user_stores. Повертає id адміна (потрібен для FK
/// `work_sessions.user_id`).
async fn ensure_seed(pool: &sqlx::PgPool) -> Uuid {
    apply_schema().await;
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Parents Точка') ON CONFLICT (id) DO NOTHING",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .execute(pool)
    .await
    .expect("seed store");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Parents Адмін', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
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
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE login = 'admin' LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("id адміна")
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
                let v: Value = r.json().await.expect("login json");
                return v["access_token"]
                    .as_str()
                    .expect("access_token")
                    .to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("login: сервер не піднявся");
}

/// POST /api/v1/sync/push одним пакетом → (код, тіло з per-item результатами).
async fn push(base: &str, token: &str, items: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/v1/sync/push"))
        .bearer_auth(token)
        .header("x-store-id", STORE1)
        .json(&items)
        .send()
        .await
        .expect("push запит");
    let code = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body)
}

/// Результат першого (єдиного) агрегата пакета — з людською помилкою в тексті
/// assert'а (щоб baseline-доказ було видно прямо у виводі тесту).
fn one(body: &Value) -> Value {
    body.as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_else(|| panic!("порожній результат push: {body}"))
}

fn assert_created(res: &Value) {
    assert_eq!(
        res["status"], "created",
        "агрегат не прийнято: status={} error={:?}",
        res["status"], res["error"]
    );
    assert!(res["server_id"].is_string(), "немає server_id: {res}");
}

fn assert_already_exists(res: &Value) {
    assert_eq!(
        res["status"], "already_exists",
        "повторний push мусить бути ідемпотентним: {res}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. B1: батько `debtor` + дитина `debtor_payment` — платіж ПРИЙНЯТО
// ─────────────────────────────────────────────────────────────────────────────

/// Сценарій приймання E1 (`ADR-0008 §3`, етап E1).
///
/// Вузол створює боржника ЛОКАЛЬНО (свій UUID — ідентичність, яку бачить
/// картка) і віддає його в хаб kind'ом `debtor`; дитина (`debtor_payment`)
/// посилається на `id` батька. ДО фіксу: перший push → `error` «тип 'debtor'
/// не підтримується push», платіж → `error` «Боржника … не знайдено в цій
/// точці — оплату відхилено».
#[tokio::test]
async fn debtor_then_payment_accepted() {
    common::force_test_db();
    let pool = api_pool().await;
    ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    let store = Uuid::parse_str(STORE1).unwrap();
    // Ідентичність боржника на вузлі (НЕ client_uuid — навмисно різні, щоб
    // довести, що хаб зберігає id вузла, а не генерує свій).
    let debtor_id = Uuid::new_v4();
    let debtor_cu = Uuid::new_v4();
    let payment_cu = Uuid::new_v4();

    // 1) Батько: боржник із боргом 150.00 (створений на вузлі).
    let (code, body) = push(
        &base,
        &token,
        json!([{
            "type": "debtor",
            "client_uuid": debtor_cu,
            "store_id": store,
            "created_at": "2026-09-12T09:15:00+03:00",
            "payload": {
                "id": debtor_id,
                "name": "Боржник E2E (створений на вузлі)",
                "phone": "+380501112233",
                "notes": "картка створена офлайн на вузлі",
                "total_debt": "150.00"
            }
        }]),
    )
    .await;
    assert_eq!(code, 200, "push батька: HTTP {code}, тіло {body}");
    let res = one(&body);
    assert_created(&res);
    assert_eq!(
        res["server_id"].as_str().unwrap(),
        debtor_id.to_string(),
        "хаб мусить зберегти ідентичність боржника вузла (FK оплат): {res}"
    );

    // 2) Дитина: платіж 50.00 по цьому боржнику (payload з вузла).
    let (code, body) = push(
        &base,
        &token,
        json!([{
            "type": "debtor_payment",
            "client_uuid": payment_cu,
            "store_id": store,
            "created_at": "2026-09-12T09:20:00+03:00",
            "payload": {
                "debtor_id": debtor_id,
                "amount": "50.00",
                "payment_method": "cash"
            }
        }]),
    )
    .await;
    assert_eq!(code, 200, "push дитини: HTTP {code}, тіло {body}");
    let res = one(&body);
    // Головний критерій приймання: платіж ПРИЙНЯТО (у базлайні — відхилено).
    assert_created(&res);

    // 3) Стан хаба: батько + дитина + борг зменшено рівно на суму оплати.
    let debtor: Option<(String, String)> =
        sqlx::query_as("SELECT name, total_debt::text FROM debtors WHERE client_uuid = $1")
            .bind(debtor_cu)
            .fetch_optional(&pool)
            .await
            .expect("SELECT debtors");
    let (name, debt) = debtor.expect("боржник вузла не доїхав на хаб");
    assert_eq!(name, "Боржник E2E (створений на вузлі)");
    assert_eq!(debt, "100.00", "борг мусить зменшитись на прийнятий платіж");
    let payments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM debtor_payments WHERE client_uuid = $1")
            .bind(payment_cu)
            .fetch_one(&pool)
            .await
            .expect("COUNT debtor_payments");
    assert_eq!(payments, 1, "платіж мусить бути на хабі рівно один раз");

    // 4) Ідемпотентність: повторний push батька І дитини (відповідь
    //    загубилась після COMMIT — вузол повторює) → already_exists, без дублів.
    let (_c, body) = push(
        &base,
        &token,
        json!([{
            "type": "debtor",
            "client_uuid": debtor_cu,
            "store_id": store,
            "payload": {
                "id": debtor_id,
                "name": "Боржник E2E (створений на вузлі)",
                "total_debt": "150.00"
            }
        }]),
    )
    .await;
    assert_already_exists(&one(&body));
    let (_c, body) = push(
        &base,
        &token,
        json!([{
            "type": "debtor_payment",
            "client_uuid": payment_cu,
            "store_id": store,
            "payload": { "debtor_id": debtor_id, "amount": "50.00" }
        }]),
    )
    .await;
    assert_already_exists(&one(&body));

    let debt_again: String =
        sqlx::query_scalar("SELECT total_debt::text FROM debtors WHERE id = $1")
            .bind(debtor_id)
            .fetch_one(&pool)
            .await
            .expect("SELECT debtors (повторний push)");
    assert_eq!(
        debt_again, "100.00",
        "повторний push не має списати борг вдруге"
    );
    let payments_again: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM debtor_payments WHERE client_uuid = $1")
            .bind(payment_cu)
            .fetch_one(&pool)
            .await
            .expect("COUNT debtor_payments (повторний push)");
    assert_eq!(payments_again, 1, "дублікат оплати на хабі");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. B2: `work_session` (сесія вузла; kind уже є в outbox вузла)
// ─────────────────────────────────────────────────────────────────────────────

/// Payload — той самий, що формує вузол
/// (`offline/transactions.rs::work_session_payload`).
#[tokio::test]
async fn work_session_pushed_and_idempotent() {
    common::force_test_db();
    let pool = api_pool().await;
    let admin = ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;
    let store = Uuid::parse_str(STORE1).unwrap();
    let cu = Uuid::new_v4();

    let item = json!([{
        "type": "work_session",
        "client_uuid": cu,
        "store_id": store,
        "created_at": "2026-09-12T09:00:00+03:00",
        "payload": {
            "user_id": admin,
            "store_id": store,
            "login_time": "2026-09-12T09:00:00+03:00",
            "logout_time": "2026-09-12T11:30:00+03:00",
            "duration_hours": 2.5
        }
    }]);
    let (code, body) = push(&base, &token, item.clone()).await;
    assert_eq!(code, 200, "HTTP {code}: {body}");
    assert_created(&one(&body));

    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT login_time::text, logout_time::text, duration_hours::text \
         FROM work_sessions WHERE client_uuid = $1",
    )
    .bind(cu)
    .fetch_optional(&pool)
    .await
    .expect("SELECT work_sessions");
    let (login_time, logout_time, dur) = row.expect("сесія вузла не доїхала на хаб");
    assert_eq!(login_time, "2026-09-12 06:00:00", "час каси → UTC");
    assert_eq!(logout_time, "2026-09-12 08:30:00", "час каси → UTC");
    assert_eq!(dur, "2.50", "тривалість з payload вузла");

    let (_c, body) = push(&base, &token, item).await;
    assert_already_exists(&one(&body));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_sessions WHERE client_uuid = $1")
        .bind(cu)
        .fetch_one(&pool)
        .await
        .expect("COUNT work_sessions");
    assert_eq!(n, 1, "дублікат сесії на хабі");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. B3: `prro_shift` (фіскальна зміна точки)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn prro_shift_pushed_and_idempotent() {
    common::force_test_db();
    let pool = api_pool().await;
    ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;
    let store = Uuid::parse_str(STORE1).unwrap();
    let cu = Uuid::new_v4();
    // Номер зміни — унікальний на точку в межах тесту (dev-БД спільна).
    let shift_number: i32 = (Uuid::new_v4().as_u128() % 9_000_000) as i32 + 1_000_000;

    let item = json!([{
        "type": "prro_shift",
        "client_uuid": cu,
        "store_id": store,
        "created_at": "2026-09-12T08:00:00+03:00",
        "payload": {
            "shift_number": shift_number,
            "opened_at": "2026-09-12T08:00:00+03:00",
            "status": "open",
            "signer_serial": "E2E-SERIAL-0001",
            "signer_name": "Підписувач E2E",
            "receipt_count": 3,
            "total_amount": "1250.00",
            "last_local_number": 7
        }
    }]);
    let (code, body) = push(&base, &token, item.clone()).await;
    assert_eq!(code, 200, "HTTP {code}: {body}");
    assert_created(&one(&body));

    let row: Option<(i32, String, String, String, i32)> = sqlx::query_as(
        "SELECT shift_number, status::text, opened_at::text, total_amount::text, \
                last_local_number \
         FROM prro_shifts WHERE client_uuid = $1",
    )
    .bind(cu)
    .fetch_optional(&pool)
    .await
    .expect("SELECT prro_shifts");
    let (num, status, opened_at, total, last_local) = row.expect("зміна вузла не доїхала на хаб");
    assert_eq!(num, shift_number);
    assert_eq!(status, "open");
    assert_eq!(opened_at, "2026-09-12 05:00:00", "час каси → UTC");
    assert_eq!(total, "1250.00");
    assert_eq!(last_local, 7);

    let (_c, body) = push(&base, &token, item).await;
    assert_already_exists(&one(&body));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM prro_shifts WHERE client_uuid = $1")
        .bind(cu)
        .fetch_one(&pool)
        .await
        .expect("COUNT prro_shifts");
    assert_eq!(n, 1, "дублікат зміни на хабі");
}

// ─────────────────────────────────────────────────────────────────────────────
// 1б. Прямий доказ прод-симптому: платіж до прибуття батька — ВІДХИЛЕНО
// ─────────────────────────────────────────────────────────────────────────────

/// Негативний контроль (той самий симптом, що в проде): якщо боржник ще не
/// доїхав на хаб, `accept_debtor_payment` відхиляє платіж ЛЮДСЬКОЮ помилкою
/// («Боржника … не знайдено в цій точці — оплату відхилено») — і НЕ пише
/// нічого в БД (pre-flight до INSERT). Після прибуття батька kind'ом `debtor`
/// той самий платіж (ТОЙ САМИЙ client_uuid — каса повторює) приймається:
/// невдалий push не отруює retry (важливо для E2b «defer замість failed»).
#[tokio::test]
async fn payment_before_debtor_arrives_is_rejected_then_accepted() {
    common::force_test_db();
    let pool = api_pool().await;
    ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;
    let store = Uuid::parse_str(STORE1).unwrap();

    let debtor_id = Uuid::new_v4();
    let payment_cu = Uuid::new_v4();
    let payment = json!([{
        "type": "debtor_payment",
        "client_uuid": payment_cu,
        "store_id": store,
        "created_at": "2026-09-12T09:20:00+03:00",
        "payload": { "debtor_id": debtor_id, "amount": "50.00" }
    }]);

    // 1) Платіж по боржнику, якого на хабі немає (батько не доїхав).
    let (_c, body) = push(&base, &token, payment.clone()).await;
    let res = one(&body);
    assert_eq!(res["status"], "error", "{res}");
    let err = res["error"].as_str().unwrap_or_default();
    eprintln!(
        "[e2e][evidence] платіж по боржнику, якого немає на хабі → status={} error={err}",
        res["status"]
    );
    assert!(
        err.contains("не знайдено в цій точці"),
        "платіж мусить бути відхилений людською помилкою батька: {err}"
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM debtor_payments WHERE client_uuid = $1")
        .bind(payment_cu)
        .fetch_one(&pool)
        .await
        .expect("COUNT debtor_payments");
    assert_eq!(n, 0, "відхилений платіж не має лишити слідів у БД");

    // 2) Батько доїхав.
    let (_c, body) = push(
        &base,
        &token,
        json!([{
            "type": "debtor",
            "client_uuid": Uuid::new_v4(),
            "store_id": store,
            "payload": { "id": debtor_id, "name": "Боржник E2E (запізнілий)", "total_debt": "50.00" }
        }]),
    )
    .await;
    assert_created(&one(&body));

    // 3) Той самий платіж — тепер прийнято.
    let (_c, body) = push(&base, &token, payment).await;
    assert_created(&one(&body));
    let debt: String = sqlx::query_scalar("SELECT total_debt::text FROM debtors WHERE id = $1")
        .bind(debtor_id)
        .fetch_one(&pool)
        .await
        .expect("SELECT debtors");
    assert_eq!(debt, "0.00", "оплата застосована до боргу");
}
