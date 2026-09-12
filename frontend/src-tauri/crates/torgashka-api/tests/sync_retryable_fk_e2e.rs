//! E2b (ADR-0008 §4.3, §7.1-E1/E2): ПОРЯДОК і RETRY — топосорт за DAG +
//! `RETRYABLE_FK` (defer) замість незворотного `failed`.
//!
//! Два дефекти, які закриваються:
//!   1. порядок агрегатів у батчі був хронологічний (FIFO), не топологічний:
//!      дитина могла піти раніше батька → FK-відмова;
//!   2. БУДЬ-ЯКА помилка агрегата на клієнті ставала `failed` — назавжди
//!      («потребує ручного втручання»), хоча повтор після прибуття батька був
//!      би успішним.
//!
//! Що доводить тест:
//!   * SQLSTATE відмови «дитина без батька» — саме **23503**
//!     (`debtor_payments.debtor_id → debtors.id`, прямий INSERT у PG);
//!   * дитина РАНІШЕ батька в ОДНОМУ батчі → обидва прийнято (топосорт за DAG:
//!     без нього дитина була б відкинута — pre-flight батька у приймачі);
//!   * дитина БЕЗ батька в батчі → сервер віддає `error_class = RETRYABLE_FK`
//!     (хоч би й через pre-flight приймача — той самий FK-контракт);
//!   * клієнт НЕ ставить такий агрегат у `failed`: він ПОВЕРТАЄТЬСЯ в чергу
//!     (`pending`, attempts+1, backoff, подія `retry`) — і після прибуття
//!     батька той самий агрегат приймається (`done` + рядок у PG).

mod common;

use std::time::Duration;

use serde_json::{json, Value};
use torgashka_api::run_facade;
use torgashka_infrastructure::offline::sync_push::{
    open_connection, push_pending_batch, PushConfig,
};
use torgashka_infrastructure::offline::transactions::enqueue_transaction;
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
            sync_schema::apply(&p).await;
            p.close().await;
        })
        .await;
}

async fn api_pool() -> sqlx::PgPool {
    let _ = torgashka_infrastructure::db::resolve_database_url()
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env");
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

async fn ensure_seed(pool: &sqlx::PgPool) {
    apply_schema().await;
    sqlx::query(
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E FK Точка') ON CONFLICT (id) DO NOTHING",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .execute(pool)
    .await
    .expect("seed store");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E FK Адмін', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
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

fn item_of(body: &Value, cu: Uuid) -> &Value {
    body.as_array()
        .and_then(|a| a.iter().find(|r| r["client_uuid"] == cu.to_string()))
        .unwrap_or_else(|| panic!("немає результату для {cu}: {body}"))
}

fn debtor_item(cu: Uuid, debtor_id: Uuid, store: Uuid) -> Value {
    json!({
        "type": "debtor",
        "client_uuid": cu,
        "store_id": store,
        "created_at": "2026-09-20T09:00:00+03:00",
        "payload": { "id": debtor_id, "name": "Боржник E2b", "total_debt": "50.00" }
    })
}

fn payment_item(cu: Uuid, debtor_id: Uuid, store: Uuid) -> Value {
    json!({
        "type": "debtor_payment",
        "client_uuid": cu,
        "store_id": store,
        "created_at": "2026-09-20T09:30:00+03:00",
        "payload": { "debtor_id": debtor_id, "amount": "25.00", "payment_method": "cash" }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// E2b: дитина → defer (не failed) → батько приїхав → повтор → прийнято
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn child_before_parent_deferred_then_accepted() {
    common::force_test_db();
    let pool = api_pool().await;
    ensure_seed(&pool).await;
    let store = Uuid::parse_str(STORE1).unwrap();

    // ── 0. ДОКАЗ SQLSTATE: FK `debtor_payments.debtor_id → debtors.id` дає
    //    саме 23503 (прямий INSERT у PG, повз API — щоб код не можна було
    //    «підмінити» власною валідацією додатка).
    let probe_debtor = Uuid::new_v4();
    let probe = sqlx::query(
        "INSERT INTO debtor_payments (id, debtor_id, amount, payment_method, store_id, created_at, client_uuid) \
         VALUES ($1, $2, 1.00, 'cash', $3, now(), $4)",
    )
    .bind(Uuid::new_v4())
    .bind(probe_debtor)
    .bind(store)
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await
    .expect_err("INSERT дитини без батька мусить порушити FK");
    let sqlstate = match &probe {
        sqlx::Error::Database(db) => db.code().map(|c| c.to_string()),
        other => panic!("очікували помилку БД, маємо {other}"),
    };
    eprintln!(
        "[e2e][evidence] FK debtor_payments→debtors без батька: SQLSTATE = {sqlstate:?} \
         (очікується \"23503\")"
    );
    assert_eq!(
        sqlstate.as_deref(),
        Some("23503"),
        "відмова «дитина без батька» — порушення FK, SQLSTATE 23503"
    );

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;

    // ── 1. ТОПОСОРТ: дитина СТОЇТЬ РАНІШЕ батька в ОДНОМУ батчі.
    //    Без топологічного сортування приймач відкинув би дитину
    //    (`accept_debtor_payment` перевіряє батька ДО вставки).
    let debtor_a = Uuid::new_v4();
    let child_a = Uuid::new_v4();
    let debtor_a_cu = Uuid::new_v4();
    // client_uuid агрегатів, створених ЦИМ тестом (для прибирання журналу).
    let mut own_uuids: Vec<Uuid> = vec![child_a, debtor_a_cu];
    let batch = json!([
        payment_item(child_a, debtor_a, store), // дитина — ПЕРША в запиті
        debtor_item(debtor_a_cu, debtor_a, store)  // батько — ДРУГИЙ
    ]);
    let (code, body) = push(&base, &token, batch).await;
    assert_eq!(code, 200, "push батча: {body}");
    let child_res = item_of(&body, child_a);
    assert_eq!(
        child_res["status"], "created",
        "дитина мусить бути прийнята: топосорт обробляє батька РАНІШЕ (без нього була б FK-відмова): {child_res}"
    );
    eprintln!(
        "[e2e][evidence] топосорт: у запиті дитина[0] → батько[1]; результат дитини = created \
         (обробку відсортовано за DAG)"
    );

    // ── 2. КЛІЄНТ: локальна оплата боргу каси (та сама функція, що в проді)
    //    кладе агрегат в outbox — батька на хабі ще немає.
    let debtor_b = Uuid::new_v4();
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let db_path = dir.path().join("cash-e2b.db");
    let mut conn = open_connection(&db_path).expect("каса БД");
    let payload = json!({
        "debtor_id": debtor_b.to_string(),
        "amount": "25.00",
        "payment_method": "cash"
    })
    .to_string();
    let enq = enqueue_transaction(&mut conn, "debtor_payment", &payload, STORE1).expect("enqueue");
    let client_child_uuid = Uuid::parse_str(&enq.client_uuid).expect("uuid");
    let envelope: String = conn
        .query_row(
            "SELECT payload FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![enq.client_uuid],
            |r| r.get(0),
        )
        .expect("конверт агрегата в outbox");
    drop(conn);

    own_uuids.push(client_child_uuid);

    // ── 2a. СЕРВЕР: той самий агрегат без батька → клас відмови.
    //    (pre-flight приймача батька = ТОЙ САМИЙ FK-контракт, що дав 23503.)
    let envelope_json: Value = serde_json::from_str(&envelope).expect("JSON конверта");
    let (code, body) = push(&base, &token, json!([envelope_json])).await;
    assert_eq!(code, 200, "per-item помилка не валить пакет: {body}");
    let res = item_of(&body, client_child_uuid);
    assert_eq!(res["status"], "error", "{res}");
    assert_eq!(
        res["error_class"], "RETRYABLE_FK",
        "FK-батько ще не приїхав → ПОВТОРЮВАНИЙ клас, а не VALIDATION: {res}"
    );
    eprintln!(
        "[e2e][evidence] дитина без батька: status=error, error_class={}, error={}",
        res["error_class"],
        res["error"].as_str().unwrap_or_default()
    );
    let in_pg: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM debtor_payments WHERE client_uuid = $1")
            .bind(client_child_uuid)
            .fetch_one(&pool)
            .await
            .expect("COUNT debtor_payments");
    assert_eq!(in_pg, 0, "відкинутий агрегат не лишає слідів у БД");

    let client = reqwest::Client::new();
    let cfg = PushConfig {
        base_url: base.clone(),
        token: token.clone(),
        store_id: Some(STORE1.to_string()),
        db_path: db_path.clone(),
        interval_secs: 30,
    };
    let s1 = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push-цикл 1 (батька ще немає)");
    assert_eq!(s1.sent, 1, "у черзі один агрегат");
    assert_eq!(
        (s1.failed, s1.deferred, s1.done),
        (0, 1, 0),
        "RETRYABLE_FK → defer, НЕ failed (E2b)"
    );

    let conn = open_connection(&db_path).expect("каса БД");
    let row = conn
        .query_row(
            "SELECT status, attempts, next_attempt_at > datetime('now') FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![enq.client_uuid],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        )
        .expect("рядок outbox");
    assert_eq!(
        row.0, "pending",
        "агрегат ПОВЕРНУВСЯ в чергу, а не 'failed'"
    );
    assert_eq!(row.1, 1, "одна невдала спроба врахована");
    assert_eq!(row.2, 1, "backoff: наступна спроба — у майбутньому");
    let retry_detail: String = conn
        .query_row(
            "SELECT detail FROM sync_log WHERE kind = 'retry' ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("подія retry у журналі каси");
    assert!(
        retry_detail.contains("RETRYABLE_FK"),
        "журнал каси мусить називати причину: {retry_detail}"
    );
    drop(conn);
    eprintln!(
        "[e2e][evidence] клієнт: outbox status=pending (НЕ failed), attempts=1, backoff активний, \
         sync_log.retry = \"{retry_detail}\""
    );

    // ── 4. БАТЬКО ПРИЇХАВ (kind `debtor`, E1) → той самий агрегат приймається.
    let debtor_b_cu = Uuid::new_v4();
    own_uuids.push(debtor_b_cu);
    let (code, body) = push(
        &base,
        &token,
        json!([debtor_item(debtor_b_cu, debtor_b, store)]),
    )
    .await;
    assert_eq!(code, 200, "push батька: {body}");
    let parent_res = body
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        parent_res["status"], "created",
        "батько прийнятий: {parent_res}"
    );

    // Backoff каси (2 с на першій спробі) минає — наступний цикл забирає агрегат.
    tokio::time::sleep(Duration::from_millis(2300)).await;
    let s2 = push_pending_batch(&db_path, &client, &cfg)
        .await
        .expect("push-цикл 2 (батько на хабі)");
    assert_eq!(
        (s2.done, s2.failed, s2.deferred),
        (1, 0, 0),
        "після прибуття батька агрегат прийнято (не загинув)"
    );

    let conn = open_connection(&db_path).expect("каса БД");
    let status: String = conn
        .query_row(
            "SELECT status FROM outbox WHERE client_uuid = ?1",
            rusqlite::params![enq.client_uuid],
            |r| r.get(0),
        )
        .expect("статус outbox");
    assert_eq!(status, "done", "outbox агрегата закрито");
    drop(conn);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM debtor_payments WHERE client_uuid = $1")
        .bind(client_child_uuid)
        .fetch_one(&pool)
        .await
        .expect("COUNT debtor_payments");
    assert_eq!(n, 1, "оплата на хабі рівно один раз");
    let debt: String = sqlx::query_scalar("SELECT total_debt::text FROM debtors WHERE id = $1")
        .bind(debtor_b)
        .fetch_one(&pool)
        .await
        .expect("SELECT debtors");
    assert_eq!(
        debt, "25.00",
        "борг зменшено прийнятим платежем (50.00 − 25.00)"
    );
    eprintln!(
        "[e2e][evidence] після прибуття батька: outbox=done, debtor_payments=1, борг 50.00 → {debt}"
    );

    // Прибирання (та сама тестова БД для багатьох тестів).
    let _ = sqlx::query("DELETE FROM debtor_payments WHERE debtor_id = $1")
        .bind(debtor_b)
        .execute(&pool)
        .await;
    let _ = sqlx::query("DELETE FROM debtors WHERE id = $1")
        .bind(debtor_b)
        .execute(&pool)
        .await;
    let _ = sqlx::query("DELETE FROM debtor_payments WHERE debtor_id = $1")
        .bind(debtor_a)
        .execute(&pool)
        .await;
    let _ = sqlx::query("DELETE FROM debtors WHERE id = $1")
        .bind(debtor_a)
        .execute(&pool)
        .await;
    // Журнал/батчі своїх агрегатів (спільна pos_system_fresh_test): батчі
    // прибираємо РАНІШЕ за sync_log — на них посилається batch_id.
    let _ = sqlx::query(
        "DELETE FROM sync_batches WHERE id IN (\
             SELECT DISTINCT batch_id FROM sync_log \
             WHERE client_uuid = ANY($1) AND batch_id IS NOT NULL)",
    )
    .bind(&own_uuids[..])
    .execute(&pool)
    .await;
    let _ = sqlx::query("DELETE FROM sync_log WHERE client_uuid = ANY($1)")
        .bind(&own_uuids[..])
        .execute(&pool)
        .await;
    // probe_debtor: проба лише фіксує SQLSTATE — рядка в БД не створено.
    let _ = probe_debtor;
}
