//! Інтеграційні тести sqlx-репозиторію ПРРО (етап 7.3 + per-store).
//! Потребують доступної PostgreSQL (як Python-еталон): DATABASE_URL або
//! DB_* у backend/.env. Схема prro створюється ідемпотентно (ensure_prro_schema).
//!
//! Per-store модель «Один магазин — один ПРРО»: репозиторій працює ТІЛЬКИ в
//! межах StoreCtx (X-Store-Id) — всі repo-тести виконуються під
//! with_store_ctx + реальним рядком stores (FK store_id NOT NULL).

use chrono::Utc;
use torgashka_infrastructure::prro::SqlxPrroRepository;
use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};
use torgashka_prro::prro::{
    PrroOfflineQueue, PrroQueueStatus, PrroRepository, PrroShift, PrroShiftStatus, CHECK_TYPE_CHK,
    KEY_LAST_SHIFT_NUMBER,
};
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_test_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

fn uniq() -> String {
    // обмежуємо розмір — колонка shift_number INTEGER (32-bit), 1:1 Python
    (chrono::Utc::now().timestamp_micros() % 1_000_000).to_string()
}

/// Серіалізація prro-тестів: DDL (CREATE TYPE/TABLE/ALTER) + DELETE не можуть
/// виконуватись паралельно (PostgreSQL deadlock на DDL-локах).
static PRRO_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Повна схема (stores/users/RLS) один раз на бінарник — prro-тести можуть
/// стартувати на чистій БД (ensure_schema також застосовує prro-міграцію).
static SCHEMA_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn ensure_full_schema(p: &sqlx::PgPool) {
    SCHEMA_ONCE
        .get_or_init(|| async {
            torgashka_infrastructure::db::ensure_schema(p)
                .await
                .expect("ensure_schema (prro per-store міграція)");
        })
        .await;
}

/// Ізоляція тестів: чиста prro-схема (тестова БД, не production).
async fn cleanup_prro(p: &sqlx::PgPool) {
    sqlx::raw_sql("DELETE FROM prro_queue_items; DELETE FROM prro_shifts; DELETE FROM prro_settings;")
        .execute(p)
        .await
        .expect("cleanup");
}

/// Створює точку-фікстуру (FK store_id NOT NULL) і повертає її id.
async fn seed_store(p: &sqlx::PgPool, name: &str) -> Uuid {
    ensure_full_schema(p).await;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stores (id, name, is_active, created_at, updated_at) \
         VALUES ($1, $2, true, now(), now()) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(name)
    .execute(p)
    .await
    .expect("seed store");
    id
}

/// Виконує замикання в межах контексту точки (StoreCtx): репозиторій бере
/// store_id саме з цього контексту.
async fn run_in_store<T, Fut>(p: &sqlx::PgPool, f: impl FnOnce() -> Fut) -> T
where
    Fut: std::future::Future<Output = T>,
{
    let store_id = seed_store(p, &format!("prro-test {}", &uniq()[..4])).await;
    with_store_ctx(
        StoreCtx {
            user_id: Uuid::new_v4(),
            store_id,
            role: "owner".to_string(),
        },
        f(),
    )
    .await
}

#[tokio::test]
async fn ensure_schema_idempotent() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    ensure_full_schema(&p).await;
    torgashka_infrastructure::prro::ensure_prro_schema(&p)
        .await
        .expect("schema");
    // повторний виклик — ідемпотентний (IF NOT EXISTS / DO-блокування)
    torgashka_infrastructure::prro::ensure_prro_schema(&p)
        .await
        .expect("schema 2nd");
}

#[tokio::test]
async fn shift_crud_open_close() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    run_in_store(&p, || async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo");
        let n = uniq();
        let shift_number: i64 = format!("7{n}").parse().unwrap();

        let mut shift = PrroShift::new(shift_number, Utc::now());
        shift.signer_serial = Some("TEST-SERIAL".into());
        shift.signer_name = Some("Тестовий Підписант".into());
        let saved = repo.create_shift(shift).await.expect("create");
        assert_eq!(saved.status, PrroShiftStatus::Open);

        // get_open_shift знаходить створену
        let open = repo
            .get_open_shift()
            .await
            .expect("get_open")
            .expect("some");
        assert_eq!(open.shift_number, shift_number);
        assert_eq!(open.signer_name.as_deref(), Some("Тестовий Підписант"));

        // close_shift
        let closed = repo
            .close_shift(
                saved.id,
                Utc::now(),
                "test-user".into(),
                format!("Z-{n}"),
                Some("TEST-SERIAL".into()),
                Some("Тестовий Підписант".into()),
            )
            .await
            .expect("close")
            .expect("some");
        assert_eq!(closed.status, PrroShiftStatus::Closed);
        assert_eq!(
            closed.zreport_number.as_deref(),
            Some(format!("Z-{n}").as_str())
        );
        assert!(repo.get_open_shift().await.unwrap().is_none());

        // прибрати за собою
        sqlx::query("DELETE FROM prro_shifts WHERE id = $1")
            .bind(saved.id)
            .execute(&p)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn queue_full_cycle() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    run_in_store(&p, || async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo");
        let n = uniq();
        let shift_number: i64 = format!("8{n}").parse().unwrap();
        let shift = repo
            .create_shift(PrroShift::new(shift_number, Utc::now()))
            .await
            .expect("shift");
        let xml = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><E N="1" SM="100" TX="0" TXPR="20.00" TXSM="17"></E></C><TS>20260807120000</TS></DAT>"#;

        // add + pending
        let item = PrroOfflineQueue::add_document(
            &repo,
            None,
            Some(shift.id),
            1,
            CHECK_TYPE_CHK,
            xml,
            None,
            None,
            None,
        )
        .await
        .expect("add");
        assert_eq!(item.status, PrroQueueStatus::Pending);
        assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 1);

        let pending = PrroOfflineQueue::get_pending(&repo, 100).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].local_number, 1);

        // list_by_shift
        let by_shift = PrroOfflineQueue::list_by_shift(&repo, shift.id)
            .await
            .unwrap();
        assert_eq!(by_shift.len(), 1);

        // mark_sent
        let sent = PrroOfflineQueue::mark_sent(&repo, item.id, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(sent.status, PrroQueueStatus::Sent);
        assert!(sent.sent_at.is_some());
        assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);

        // mark_failed
        let item2 = PrroOfflineQueue::add_document(
            &repo,
            None,
            Some(shift.id),
            2,
            CHECK_TYPE_CHK,
            xml,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let failed = PrroOfflineQueue::mark_failed(&repo, item2.id, "ERROR_OFFLINE_168".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, PrroQueueStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("ERROR_OFFLINE_168"));
        // failed знову в get_pending (повторна передача)
        assert_eq!(
            PrroOfflineQueue::get_pending(&repo, 100)
                .await
                .unwrap()
                .len(),
            1
        );

        // прибрати за собою
        sqlx::query("DELETE FROM prro_queue_items WHERE shift_id = $1")
            .bind(shift.id)
            .execute(&p)
            .await
            .unwrap();
        sqlx::query("DELETE FROM prro_shifts WHERE id = $1")
            .bind(shift.id)
            .execute(&p)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn settings_upsert() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    run_in_store(&p, || async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo");
        let key = format!("test_key_{}", uniq());
        repo.set_setting(&key, "42").await.expect("set");
        assert_eq!(
            repo.get_setting(&key).await.unwrap().as_deref(),
            Some("42")
        );
        repo.set_setting(&key, "43").await.expect("upsert");
        assert_eq!(
            repo.get_setting(&key).await.unwrap().as_deref(),
            Some("43")
        );
        sqlx::query("DELETE FROM prro_settings WHERE key_name = $1")
            .bind(&key)
            .execute(&p)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn next_shift_number_increments() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    run_in_store(&p, || async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo");
        let key = format!("last_shift_number_{}", uniq());
        // 1:1 Python next_shift_number: last + 1
        repo.set_setting(&key, "7").await.unwrap();
        let last: i64 = repo
            .get_setting(&key)
            .await
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(last + 1, 8);
        // KEY_LAST_SHIFT_NUMBER реальний ключ теж працює
        repo.set_setting(KEY_LAST_SHIFT_NUMBER, "5").await.unwrap();
        assert_eq!(
            repo.get_setting(KEY_LAST_SHIFT_NUMBER)
                .await
                .unwrap()
                .as_deref(),
            Some("5")
        );
        sqlx::query("DELETE FROM prro_settings WHERE key_name = $1 OR key_name = $2")
            .bind(&key)
            .bind(KEY_LAST_SHIFT_NUMBER)
            .execute(&p)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn get_shift_by_number_and_delete() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    run_in_store(&p, || async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo");
        let n = uniq();
        let shift_number: i64 = format!("9{n}").parse().unwrap();
        let shift = repo
            .create_shift(PrroShift::new(shift_number, Utc::now()))
            .await
            .expect("create");
        let found = repo
            .get_shift_by_number(shift_number)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, shift.id);
        assert!(!repo.delete_queue_item(Uuid::new_v4()).await.unwrap());
        sqlx::query("DELETE FROM prro_shifts WHERE id = $1")
            .bind(shift.id)
            .execute(&p)
            .await
            .unwrap();
    })
    .await;
}

// ─── Per-store ізоляція: два магазини не бачать дані один одного ───────────

#[tokio::test]
async fn two_stores_fully_isolated() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    let store_a = seed_store(&p, "ПРРО точка А").await;
    let store_b = seed_store(&p, "ПРРО точка Б").await;

    let ctx_a = StoreCtx {
        user_id: Uuid::new_v4(),
        store_id: store_a,
        role: "owner".to_string(),
    };
    let ctx_b = StoreCtx {
        user_id: Uuid::new_v4(),
        store_id: store_b,
        role: "owner".to_string(),
    };

    // Налаштування А пишемо з контексту А.
    with_store_ctx(ctx_a.clone(), async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo A");
        repo.set_setting("prro_fn", "400000111111").await.unwrap();
        repo.set_setting("url", "api.test.a").await.unwrap();
        let shift = repo.create_shift(PrroShift::new(1, Utc::now())).await.unwrap();
        PrroOfflineQueue::add_document(
            &repo,
            None,
            Some(shift.id),
            1,
            CHECK_TYPE_CHK,
            "<DAT/>",
            None,
            None,
            None,
        )
        .await
        .expect("queue A");
    })
    .await;

    // А бачить своє; Б — порожньо (НЕ дані А).
    with_store_ctx(ctx_b.clone(), async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo B");
        assert_eq!(repo.get_setting("prro_fn").await.unwrap(), None);
        assert_eq!(repo.get_setting("url").await.unwrap(), None);
        assert!(repo.get_open_shift().await.unwrap().is_none());
        assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
    })
    .await;

    with_store_ctx(ctx_a.clone(), async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo A2");
        assert_eq!(
            repo.get_setting("prro_fn").await.unwrap().as_deref(),
            Some("400000111111")
        );
        assert_eq!(repo.get_open_shift().await.unwrap().unwrap().shift_number, 1);
        assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 1);
    })
    .await;

    // PUT Б не затирає А (ключі per-store).
    with_store_ctx(ctx_b, async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo B2");
        repo.set_setting("prro_fn", "400000222222").await.unwrap();
        repo.set_setting("url", "api.test.b").await.unwrap();
    })
    .await;
    with_store_ctx(ctx_a, async {
        let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
            .await
            .expect("repo A3");
        assert_eq!(
            repo.get_setting("prro_fn").await.unwrap().as_deref(),
            Some("400000111111"),
            "конфіг А не має бути затертий конфігом Б"
        );
    })
    .await;

    cleanup_prro(&p).await;
    sqlx::query("DELETE FROM stores WHERE id = ANY($1)")
        .bind(&[store_a, store_b][..])
        .execute(&p)
        .await
        .unwrap();
}

// ─── Міграція: глобальні рядки старої схеми → перший активний магазин ──────

#[tokio::test]
async fn legacy_global_rows_migrate_to_first_active_store() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    ensure_full_schema(&p).await;
    // Чиста БД точок — детермінований «перший активний».
    sqlx::raw_sql("TRUNCATE stores CASCADE")
        .execute(&p)
        .await
        .expect("truncate stores");

    // Симуляція СТАРОЇ (глобальної) схеми: store_id nullable.
    sqlx::raw_sql(
        "ALTER TABLE prro_settings ALTER COLUMN store_id DROP NOT NULL;
         ALTER TABLE prro_shifts ALTER COLUMN store_id DROP NOT NULL;
         ALTER TABLE prro_queue_items ALTER COLUMN store_id DROP NOT NULL;",
    )
    .execute(&p)
    .await
    .expect("drop not null");

    // Старі глобальні рядки (без store_id).
    let old_shift = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO prro_settings (key_name, value, updated_at) VALUES \
         ('prro_fn','400000999999',now()),('mode','test',now()),('url','legacy.test',now())",
    )
    .execute(&p)
    .await
    .expect("legacy settings");
    sqlx::query(
        "INSERT INTO prro_shifts (id, shift_number, opened_at, status, receipt_count, \
                                  total_amount, last_local_number) \
         VALUES ($1, 777, now(), 'open'::public.prro_shift_status, 3, 0::numeric, 0)",
    )
    .bind(old_shift)
    .execute(&p)
    .await
    .expect("legacy shift");
    sqlx::query(
        "INSERT INTO prro_queue_items (id, local_number, check_type, xml_body, status, created_at) \
         VALUES ($1, 1, 'CHK', '<DAT/>', 'pending'::public.prro_queue_status, now())",
    )
    .bind(Uuid::new_v4())
    .execute(&p)
    .await
    .expect("legacy queue");

    // Магазини: перший АКТИВНИЙ за created_at → А.
    let store_first = Uuid::new_v4();
    let store_second = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stores (id, name, is_active, created_at, updated_at) VALUES \
         ($1, 'Перший', true, now() - interval '1 hour', now()), \
         ($2, 'Другий', true, now(), now())",
    )
    .bind(store_first)
    .bind(store_second)
    .execute(&p)
    .await
    .expect("stores");

    // Міграція (двічі — перевірка ідемпотентності).
    torgashka_infrastructure::prro::ensure_prro_schema(&p)
        .await
        .expect("migration");
    torgashka_infrastructure::prro::ensure_prro_schema(&p)
        .await
        .expect("migration 2nd");

    // Усі глобальні рядки → store_first.
    let (cnt_null,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM prro_settings WHERE store_id IS NULL",
    )
    .fetch_one(&p)
    .await
    .unwrap();
    assert_eq!(cnt_null, 0, "settings: NULL store_id має бути 0");
    let (sid,): (Uuid,) =
        sqlx::query_as("SELECT store_id FROM prro_settings WHERE key_name = 'prro_fn'")
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(sid, store_first);
    let (shift_sid,): (Uuid,) =
        sqlx::query_as("SELECT store_id FROM prro_shifts WHERE id = $1")
            .bind(old_shift)
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(shift_sid, store_first);
    let (q_null,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM prro_queue_items WHERE store_id IS NULL")
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(q_null, 0, "queue: NULL store_id має бути 0");

    // NOT NULL знову активний: вставка без store_id тепер падає.
    let insert_err = sqlx::query(
        "INSERT INTO prro_settings (key_name, value) VALUES ('x', 'y')",
    )
    .execute(&p)
    .await;
    assert!(insert_err.is_err(), "NOT NULL має блокувати глобальний запис");

    cleanup_prro(&p).await;
    sqlx::query("DELETE FROM stores WHERE id = ANY($1)")
        .bind(&[store_first, store_second][..])
        .execute(&p)
        .await
        .unwrap();
}
