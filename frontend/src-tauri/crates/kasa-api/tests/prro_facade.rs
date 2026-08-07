//! Інтеграційні тести Rust-фасаду ПРРО (етап 7.3) — БД + хендлери.
//! Потребують доступної PostgreSQL (DATABASE_URL або DB_* у backend/.env).

use std::sync::Arc;

use kasa_api::prro::PrroFacade;
use kasa_infrastructure::prro::SqlxPrroRepository;
use kasa_prro::prro::PrroRepository;

async fn pool() -> sqlx::PgPool {
    kasa_infrastructure::db::connect_readonly_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

static FACADE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn facade(shadow: bool) -> Arc<PrroFacade> {
    let _guard = FACADE_TEST_LOCK.lock().await;
    let p = pool().await;
    sqlx::raw_sql("DELETE FROM prro_queue_items; DELETE FROM prro_shifts;")
        .execute(&p)
        .await
        .unwrap();
    let repo = SqlxPrroRepository::connect(p).await.expect("repo");
    Arc::new(PrroFacade::new(repo, shadow))
}

#[tokio::test]
async fn status_reports_rust_gate() {
    let f = facade(false).await;
    let v = f.status().await.expect("status");
    assert_eq!(v["rust_gate"], true);
    assert_eq!(v["configured"], false);
    assert_eq!(v["queue_pending"], 0);
}

#[tokio::test]
async fn list_shifts_empty_then_with_shift() {
    let f = facade(false).await;
    let v = f.list_shifts(1, 20).await.expect("list");
    assert_eq!(v["total"], 0);

    let shift = kasa_prro::prro::PrroShift::new(424242, chrono::Utc::now());
    f.repo().create_shift(shift).await.expect("create");
    let v2 = f.list_shifts(1, 20).await.expect("list2");
    assert_eq!(v2["total"], 1);
    assert_eq!(v2["items"][0]["shift_number"], 424242);
    assert_eq!(v2["items"][0]["status"], "open");
}

#[tokio::test]
async fn queue_empty_and_pending_visible() {
    let f = facade(false).await;
    let v = f.queue(100).await.expect("queue");
    assert_eq!(v["pending"], 0);

    let shift = kasa_prro::prro::PrroShift::new(424243, chrono::Utc::now());
    let shift = f.repo().create_shift(shift).await.expect("shift");
    let xml = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><E N="1" SM="100" TX="0" TXPR="20.00" TXSM="17"></E></C><TS>20260807120000</TS></DAT>"#;
    kasa_prro::prro::PrroOfflineQueue::add_document(
        f.repo(),
        None,
        Some(shift.id),
        1,
        kasa_prro::prro::CHECK_TYPE_CHK,
        xml,
        None,
    )
    .await
    .expect("add");
    let v2 = f.queue(100).await.expect("queue2");
    assert_eq!(v2["pending"], 1);
    assert_eq!(v2["items"][0]["check_type"], "CHK");
}

#[tokio::test]
async fn open_shift_without_config_returns_config_error() {
    let f = facade(false).await;
    let err = f.open_shift().await.unwrap_err();
    assert!(
        err.to_string().contains("налаштуйте ПРРО") || err.to_string().contains("PRRO_KEY"),
        "{err}"
    );
}

#[tokio::test]
async fn shadow_sync_reports_python_handles() {
    let f = facade(true).await;
    let v = f.sync(100).await.expect("sync shadow");
    assert_eq!(v["shadow"], true);
    assert_eq!(v["pending"], 0);
}
