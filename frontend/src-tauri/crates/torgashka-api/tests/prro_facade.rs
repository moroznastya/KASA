//! Інтеграційні тести Rust-фасаду ПРРО (етап 7.3 + per-store) — БД + хендлери.
//! Потребують доступної PostgreSQL (DATABASE_URL або DB_* у backend/.env).
//!
//! Per-store: репозиторій працює лише в межах StoreCtx — кожен тест виконує
//! операції під with_store_ctx з реальним рядком stores (store_id NOT NULL).

use std::sync::Arc;

use torgashka_api::prro::PrroFacade;
use torgashka_infrastructure::prro::SqlxPrroRepository;
use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};
use torgashka_prro::prro::PrroRepository;
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_readonly_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

static FACADE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Точка-фікстура + контекст: без stores FK store_id NOT NULL не дасть INSERT.
async fn with_store<T, Fut>(p: &sqlx::PgPool, f: impl FnOnce() -> Fut) -> T
where
    Fut: std::future::Future<Output = T>,
{
    let store_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stores (id, name, is_active, created_at, updated_at) \
         VALUES ($1, 'prro-facade-test', true, now(), now()) ON CONFLICT DO NOTHING",
    )
    .bind(store_id)
    .execute(p)
    .await
    .expect("seed store");
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

async fn facade(shadow: bool) -> (Arc<PrroFacade>, sqlx::PgPool) {
    let _guard = FACADE_TEST_LOCK.lock().await;
    let p = pool().await;
    sqlx::raw_sql("DELETE FROM prro_queue_items; DELETE FROM prro_shifts;")
        .execute(&p)
        .await
        .unwrap();
    let repo = SqlxPrroRepository::connect(StorePool::new(p.clone()))
        .await
        .expect("repo");
    (Arc::new(PrroFacade::new(repo, shadow)), p)
}

mod common;

#[tokio::test]
async fn status_reports_rust_gate() {
    common::force_test_db();
    let (f, p) = facade(false).await;
    with_store(&p, || async {
        let v = f.status().await.expect("status");
        assert_eq!(v["rust_gate"], true);
        assert_eq!(v["configured"], false);
        assert_eq!(v["queue_pending"], 0);
    })
    .await;
}

#[tokio::test]
async fn list_shifts_empty_then_with_shift() {
    common::force_test_db();
    let (f, p) = facade(false).await;
    with_store(&p, || async {
        let v = f.list_shifts(1, 20).await.expect("list");
        assert_eq!(v["total"], 0);

        let shift = torgashka_prro::prro::PrroShift::new(424242, chrono::Utc::now());
        f.repo().create_shift(shift).await.expect("create");
        let v2 = f.list_shifts(1, 20).await.expect("list2");
        assert_eq!(v2["total"], 1);
        assert_eq!(v2["items"][0]["shift_number"], 424242);
        assert_eq!(v2["items"][0]["status"], "open");
    })
    .await;
}

#[tokio::test]
async fn queue_empty_and_pending_visible() {
    common::force_test_db();
    let (f, p) = facade(false).await;
    with_store(&p, || async {
        let v = f.queue(100).await.expect("queue");
        assert_eq!(v["pending"], 0);

        let shift = torgashka_prro::prro::PrroShift::new(424243, chrono::Utc::now());
        let shift = f.repo().create_shift(shift).await.expect("shift");
        let xml = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><E N="1" SM="100" TX="0" TXPR="20.00" TXSM="17"></E></C><TS>20260807120000</TS></DAT>"#;
        torgashka_prro::prro::PrroOfflineQueue::add_document(
            f.repo(),
            None,
            Some(shift.id),
            1,
            torgashka_prro::prro::CHECK_TYPE_CHK,
            xml,
            None,
            None, // B2: check_sign
            None, // B4: id_offline
        )
        .await
        .expect("add");
        let v2 = f.queue(100).await.expect("queue2");
        assert_eq!(v2["pending"], 1);
        assert_eq!(v2["items"][0]["check_type"], "CHK");
    })
    .await;
}

#[tokio::test]
async fn open_shift_without_config_returns_config_error() {
    common::force_test_db();
    let (f, p) = facade(false).await;
    with_store(&p, || async {
        let err = f.open_shift().await.unwrap_err();
        assert!(
            err.to_string().contains("налаштуйте ПРРО") || err.to_string().contains("PRRO_KEY"),
            "{err}"
        );
    })
    .await;
}

#[tokio::test]
async fn shadow_sync_reports_python_handles() {
    common::force_test_db();
    let (f, p) = facade(true).await;
    with_store(&p, || async {
        let v = f.sync(100).await.expect("sync shadow");
        assert_eq!(v["shadow"], true);
        assert_eq!(v["pending"], 0);
    })
    .await;
}
