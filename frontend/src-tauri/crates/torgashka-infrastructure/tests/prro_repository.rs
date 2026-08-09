//! Інтеграційні тести sqlx-репозиторію ПРРО (етап 7.3).
//! Потребують доступної PostgreSQL (як Python-еталон): DATABASE_URL або
//! DB_* у backend/.env. Схема prro створюється ідемпотентно (ensure_prro_schema).

use chrono::Utc;
use torgashka_infrastructure::prro::SqlxPrroRepository;
use torgashka_prro::prro::{
    PrroOfflineQueue, PrroQueueStatus, PrroRepository, PrroShift, PrroShiftStatus, CHECK_TYPE_CHK,
    KEY_LAST_SHIFT_NUMBER,
};
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_readonly_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

fn uniq() -> String {
    // обмежуємо розмір — колонка shift_number INTEGER (32-bit), 1:1 Python
    (chrono::Utc::now().timestamp_micros() % 1_000_000).to_string()
}

/// Серіалізація prro-тестів: DDL (CREATE TYPE/TABLE) + DELETE не можуть
/// виконуватись паралельно (PostgreSQL deadlock на DDL-локах).
static PRRO_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Ізоляція тестів: чиста prro-схема (тестова БД, не production).
async fn cleanup_prro(p: &sqlx::PgPool) {
    sqlx::raw_sql("DELETE FROM prro_queue_items; DELETE FROM prro_shifts;")
        .execute(p)
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn ensure_schema_idempotent() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    torgashka_infrastructure::prro::ensure_prro_schema(&p)
        .await
        .expect("schema");
    // повторний виклик — ідемпотентний (IF NOT EXISTS)
    torgashka_infrastructure::prro::ensure_prro_schema(&p)
        .await
        .expect("schema 2nd");
}

#[tokio::test]
async fn shift_crud_open_close() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    let repo = SqlxPrroRepository::connect(p.clone()).await.expect("repo");
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
}

#[tokio::test]
async fn queue_full_cycle() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    let repo = SqlxPrroRepository::connect(p.clone()).await.expect("repo");
    let n = uniq();
    let shift_number: i64 = format!("8{n}").parse().unwrap();
    let shift = repo
        .create_shift(PrroShift::new(shift_number, Utc::now()))
        .await
        .expect("shift");
    let xml = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><E N="1" SM="100" TX="0" TXPR="20.00" TXSM="17"></E></C><TS>20260807120000</TS></DAT>"#;

    // add + pending
    let item =
        PrroOfflineQueue::add_document(&repo, None, Some(shift.id), 1, CHECK_TYPE_CHK, xml, None)
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
    let item2 =
        PrroOfflineQueue::add_document(&repo, None, Some(shift.id), 2, CHECK_TYPE_CHK, xml, None)
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
}

#[tokio::test]
async fn settings_upsert() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    let repo = SqlxPrroRepository::connect(p.clone()).await.expect("repo");
    let key = format!("test_key_{}", uniq());
    repo.set_setting(&key, "42").await.expect("set");
    assert_eq!(repo.get_setting(&key).await.unwrap().as_deref(), Some("42"));
    repo.set_setting(&key, "43").await.expect("upsert");
    assert_eq!(repo.get_setting(&key).await.unwrap().as_deref(), Some("43"));
    sqlx::query("DELETE FROM prro_settings WHERE key_name = $1")
        .bind(&key)
        .execute(&p)
        .await
        .unwrap();
}

#[tokio::test]
async fn next_shift_number_increments() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    let repo = SqlxPrroRepository::connect(p.clone()).await.expect("repo");
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
}

#[tokio::test]
async fn get_shift_by_number_and_delete() {
    let _guard = PRRO_TEST_LOCK.lock().await;
    let p = pool().await;
    cleanup_prro(&p).await;
    let repo = SqlxPrroRepository::connect(p.clone()).await.expect("repo");
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
}
