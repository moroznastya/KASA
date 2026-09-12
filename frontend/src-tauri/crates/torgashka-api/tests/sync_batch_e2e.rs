//! E2a (ADR-0008 §7.1-A2, §4.3): БАТЧІ push — ідентичність пакета агрегатів.
//!
//! Дефект, який закривається: прийом N агрегатів одним запитом не мав єдиної
//! ідентичності. Частковий збій (2 агрегати лягли, 1 відкинуто; або пакет
//! упав посеред обробки) був невидимий у журналі: неможливо було сказати
//! «ці агрегати прийнято РАЗОМ чи ні» і не можна було відрізнити «пакет у
//! процесі» від «загублено».
//!
//! Що доводить тест:
//!   1. один push-запит → рядок `sync_batches` з `items` = кількості агрегатів;
//!   2. `status` батча виставляється за ФАКТИЧНИМ результатом: 2 прийнято +
//!      1 відкинуто → `partial` (не «accepted на віру»);
//!   3. усі агрегати батча (і прийняті, і відкинутий) мають у `sync_log` ТОЙ
//!      САМИЙ `batch_id`;
//!   4. `batch_id` клієнта (заголовок `X-Sync-Batch-Id`) повертається сервером
//!      без змін — той самий ідентифікатор на обох кінцях (критерій E3);
//!   5. клас помилки відкинутого агрегата — `VALIDATION` (незворотний), а не
//!      `RETRYABLE_FK`.

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
            // Sync-шар (Alembic 0011–0020): sync_meta/sync_log, server_version,
            // client_uuid-приймачі, sync_batches — ensure_schema (schema.sql)
            // для мігрованих БД їх не додає.
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
        "INSERT INTO stores (id, name) VALUES ($1, 'E2E Batch Точка') ON CONFLICT (id) DO NOTHING",
    )
    .bind(Uuid::parse_str(STORE1).unwrap())
    .execute(pool)
    .await
    .expect("seed store");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
         VALUES ($1, 'E2E Batch Адмін', 'admin', $2, 'owner'::public.user_role, true, now(), now(), true)
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

/// POST /api/v1/sync/push батчем із ЯВНО заданим `batch_id` →
/// (код, тіло, `X-Sync-Batch-Id` із відповіді).
async fn push_batch(base: &str, token: &str, batch_id: Uuid, items: Value) -> (u16, Value, String) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/v1/sync/push"))
        .bearer_auth(token)
        .header("x-store-id", STORE1)
        .header("x-sync-batch-id", batch_id.to_string())
        .json(&items)
        .send()
        .await
        .expect("push запит");
    let code = r.status().as_u16();
    let resp_batch = r
        .headers()
        .get("x-sync-batch-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    (code, body, resp_batch)
}

// ─────────────────────────────────────────────────────────────────────────────
// Критерій E2a: 3 агрегати, 1 невалідний → status='partial', спільний batch_id
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_partial_status_and_log_batch_id() {
    common::force_test_db();
    let pool = api_pool().await;
    ensure_seed(&pool).await;

    let port = free_port().await;
    let addr = format!("127.0.0.1:{port}");
    let base = format!("http://{addr}");
    let _h = run_facade(&addr);
    let token = login(&base).await;
    let store = Uuid::parse_str(STORE1).unwrap();

    let batch_id = Uuid::new_v4();
    let debtor_cu = Uuid::new_v4();
    let shift_cu = Uuid::new_v4();
    let bad_cu = Uuid::new_v4();

    // 3 агрегати одного пакета: 2 валідні + 1 невалідний (порожнє ім'я
    // боржника → людська валідація `accept_debtor`, БЕЗ участі БД).
    let items = json!([
        {
            "type": "debtor",
            "client_uuid": debtor_cu,
            "store_id": store,
            "created_at": "2026-09-20T10:00:00+03:00",
            "payload": { "id": debtor_cu, "name": "Боржник батча E2a", "total_debt": "0.00" }
        },
        {
            "type": "prro_shift",
            "client_uuid": shift_cu,
            "store_id": store,
            "payload": {
                "id": shift_cu, "shift_number": 4242,
                "opened_at": "2026-09-20T08:00:00+03:00", "status": "open"
            }
        },
        {
            "type": "debtor",
            "client_uuid": bad_cu,
            "store_id": store,
            "payload": { "name": "   " }
        }
    ]);

    let (code, body, resp_batch) = push_batch(&base, &token, batch_id, items).await;
    assert_eq!(code, 200, "push: HTTP {code}, тіло {body}");
    let results = body.as_array().expect("масив результатів");
    assert_eq!(
        results.len(),
        3,
        "per-item результатів мусить бути 3: {body}"
    );

    let by_uuid = |u: Uuid| -> &Value {
        results
            .iter()
            .find(|r| r["client_uuid"] == u.to_string())
            .unwrap_or_else(|| panic!("немає результату для {u}: {body}"))
    };
    for cu in [debtor_cu, shift_cu] {
        assert_eq!(by_uuid(cu)["status"], "created", "{:?}", by_uuid(cu));
    }
    let bad = by_uuid(bad_cu);
    assert_eq!(bad["status"], "error", "{bad}");
    assert_eq!(
        bad["error_class"], "VALIDATION",
        "невалідний payload — незворотний клас: {bad}"
    );

    // 1. Рядок батча: items = кількості агрегатів, status — за фактом.
    let (items_n, status): (i32, String) =
        sqlx::query_as("SELECT items, status FROM sync_batches WHERE id = $1")
            .bind(batch_id)
            .fetch_one(&pool)
            .await
            .expect("рядок sync_batches мусить існувати");
    assert_eq!(items_n, 3, "items батча");
    assert_eq!(
        status, "partial",
        "2 прийнято + 1 відкинуто → partial (фактичний результат, не 'accepted')"
    );

    // 2. Той самий batch_id у відповіді (клієнт штампує батч — §4.3 п.1).
    assert_eq!(
        resp_batch,
        batch_id.to_string(),
        "сервер мусить повернути ТОЙ САМИЙ batch_id заголовком"
    );

    // 3. sync_log: рівно 3 записи батча — і прийняті, і відкинутий — з одним
    //    batch_id і з машинним класом помилки.
    let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT status, error_class, batch_id::text FROM sync_log \
         WHERE batch_id = $1 ORDER BY id",
    )
    .bind(batch_id)
    .fetch_all(&pool)
    .await
    .expect("sync_log батча");
    assert_eq!(rows.len(), 3, "усі 3 агрегати мусять бути в журналі батча");
    for (st, class, b) in &rows {
        assert_eq!(
            b.as_deref(),
            Some(batch_id.to_string().as_str()),
            "кожен запис батча несе той самий batch_id"
        );
        match st.as_str() {
            "ok" => assert!(class.is_none(), "успішний агрегат без класу помилки"),
            "error" => assert_eq!(
                class.as_deref(),
                Some("VALIDATION"),
                "клас помилки в журналі = клас у відповіді"
            ),
            other => panic!("несподіваний статус журналу '{other}'"),
        }
    }
    assert_eq!(
        rows.iter().filter(|(s, _, _)| s == "ok").count(),
        2,
        "у журналі 2 прийняті агрегати"
    );
    assert_eq!(
        rows.iter().filter(|(s, _, _)| s == "error").count(),
        1,
        "у журналі 1 відкинутий агрегат"
    );

    eprintln!(
        "[e2e][evidence] батч {}: items=3, status=partial, sync_log.batch_id = {} (3 записи), \
         клас помилки невалідного = VALIDATION",
        batch_id, batch_id
    );

    // Прибирання: агрегати батча не мусять впливати на інші тести тієї БД
    // (спільна pos_system_fresh_test).
    sqlx::query("DELETE FROM debtors WHERE client_uuid = $1")
        .bind(debtor_cu)
        .execute(&pool)
        .await
        .expect("cleanup debtors");
    sqlx::query("DELETE FROM prro_shifts WHERE client_uuid = $1")
        .bind(shift_cu)
        .execute(&pool)
        .await
        .expect("cleanup prro_shifts");
    sqlx::query("DELETE FROM sync_log WHERE batch_id = $1")
        .bind(batch_id)
        .execute(&pool)
        .await
        .expect("cleanup sync_log");
    sqlx::query("DELETE FROM sync_batches WHERE id = $1")
        .bind(batch_id)
        .execute(&pool)
        .await
        .expect("cleanup sync_batches");
}
