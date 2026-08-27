//! Unit-тести офлайн-черги ПРРО (етап 7.3) — 1:1 Python offline_queue.py.

mod common;

use chrono::{Duration, Utc};
use torgashka_prro::prro::{
    InMemoryPrroRepository, PrroOfflineQueue, PrroQueueStatus, CHECK_TYPE_CHK,
    CHECK_TYPE_SERVICECHK, CHECK_TYPE_ZREPORT, PRRO_OFFLINE_LIMIT_HOURS,
};
use torgashka_prro::xml::compute_mac;

const XML: &str = r#"<DAT FN="400000000000" TN="400000000000" ZN="400000000000" DI="1" V="2.1.7"><C T="0"><P C="120" NM="Товар" PRC="100" Q="1" SM="100" TX="0"></P><E N="1" SM="100" TX="0" TXPR="20.00" TXSM="16.67"></E></C><TS>20260807112601</TS></DAT>"#;

#[tokio::test]
async fn add_document_ok() {
    let repo = InMemoryPrroRepository::new();
    let mac = compute_mac(XML, None);
    let item = PrroOfflineQueue::add_document(
        &repo,
        None,
        None,
        5,
        CHECK_TYPE_CHK,
        XML,
        Some(mac.clone()),
        None, // B2: check_sign (не заповнено у тесті)
        None, // B4: id_offline
    )
    .await
    .expect("add");
    assert_eq!(item.local_number, 5);
    assert_eq!(item.check_type, CHECK_TYPE_CHK);
    assert_eq!(item.status, PrroQueueStatus::Pending);
    assert_eq!(item.mac.as_deref(), Some(mac.as_str()));
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 1);
}

#[tokio::test]
async fn add_document_validations() {
    let repo = InMemoryPrroRepository::new();
    let err = PrroOfflineQueue::add_document(&repo, None, None, -1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("від'ємним"), "{err}");
    let err = PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, "   ", None, None, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("порожнім"), "{err}");
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
}

#[tokio::test]
async fn mark_sent_and_failed() {
    let repo = InMemoryPrroRepository::new();
    let item = PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    let sent = PrroOfflineQueue::mark_sent(&repo, item.id, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sent.status, PrroQueueStatus::Sent);
    assert!(sent.sent_at.is_some());
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);

    let item2 = PrroOfflineQueue::add_document(&repo, None, None, 2, CHECK_TYPE_ZREPORT, XML, None, None, None)
        .await
        .unwrap();
    let failed = PrroOfflineQueue::mark_failed(&repo, item2.id, "grpc timeout".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, PrroQueueStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("grpc timeout"));
    // failed НЕ рахується в count_pending (1:1 Python)
    assert_eq!(PrroOfflineQueue::count_pending(&repo).await.unwrap(), 0);
}

#[tokio::test]
async fn get_pending_order_pending_first_then_failed() {
    let repo = InMemoryPrroRepository::new();
    // failed (старіший за часом)
    let f1 = PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    PrroOfflineQueue::mark_failed(&repo, f1.id, "err".into())
        .await
        .unwrap();
    // pending (новіший)
    PrroOfflineQueue::add_document(&repo, None, None, 2, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();

    let pending = PrroOfflineQueue::get_pending(&repo, 100).await.unwrap();
    assert_eq!(pending.len(), 2);
    // спершу pending, потім failed (1:1 Python order_by(status.asc(), created_at))
    assert_eq!(pending[0].status, PrroQueueStatus::Pending);
    assert_eq!(pending[1].status, PrroQueueStatus::Failed);
    assert_eq!(pending[1].local_number, 1);
}

#[tokio::test]
async fn limit_applied() {
    let repo = InMemoryPrroRepository::new();
    for i in 0..5 {
        PrroOfflineQueue::add_document(&repo, None, None, i, CHECK_TYPE_CHK, XML, None, None, None)
            .await
            .unwrap();
    }
    let pending = PrroOfflineQueue::get_pending(&repo, 3).await.unwrap();
    assert_eq!(pending.len(), 3);
}

#[test]
fn is_expired_168h_boundary() {
    let now = Utc::now();
    // 167 годин — ще не прострочено
    let fresh = now - Duration::hours(PRRO_OFFLINE_LIMIT_HOURS - 1);
    assert!(!PrroOfflineQueue::is_expired(fresh, Some(now)));
    // рівно 168 годин — НЕ прострочено (Python: > 168, не >=)
    let boundary = now - Duration::hours(PRRO_OFFLINE_LIMIT_HOURS);
    assert!(!PrroOfflineQueue::is_expired(boundary, Some(now)));
    // 169 годин — прострочено
    let old = now - Duration::hours(PRRO_OFFLINE_LIMIT_HOURS + 1);
    assert!(PrroOfflineQueue::is_expired(old, Some(now)));
}

#[tokio::test]
async fn get_expired_filters_only_old() {
    let repo = InMemoryPrroRepository::new();
    let now = Utc::now();
    let old = PrroOfflineQueue::add_document(&repo, None, None, 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    let fresh = PrroOfflineQueue::add_document(&repo, None, None, 2, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    // штучно зістарити old (через тестовий сетер)
    repo.set_queue_created_at(old.id, now - Duration::hours(200));
    repo.set_queue_created_at(fresh.id, now - Duration::hours(1));

    let expired = PrroOfflineQueue::get_expired(&repo, 100).await.unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].id, old.id);
}

#[tokio::test]
async fn list_by_shift_ordered_by_local_number() {
    let repo = InMemoryPrroRepository::new();
    let shift_id = uuid::Uuid::new_v4();
    PrroOfflineQueue::add_document(&repo, None, Some(shift_id), 3, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    PrroOfflineQueue::add_document(&repo, None, Some(shift_id), 1, CHECK_TYPE_CHK, XML, None, None, None)
        .await
        .unwrap();
    PrroOfflineQueue::add_document(&repo, None, None, 9, CHECK_TYPE_SERVICECHK, XML, None, None, None)
        .await
        .unwrap();
    let items = PrroOfflineQueue::list_by_shift(&repo, shift_id)
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].local_number, 1);
    assert_eq!(items[1].local_number, 3);
}
