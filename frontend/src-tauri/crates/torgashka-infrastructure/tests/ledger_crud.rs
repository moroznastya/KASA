//! Інтеграційні тести LEDGER (етап 4): журнал взаєморозрахунків.
//!
//! Потребують доступної PostgreSQL (як Python-еталон): `DATABASE_URL` або
//! `DB_*` у backend/.env.
//!
//! Покривають:
//!   - v1 create/history/balance (сума amount, ORDER BY operation_date DESC)
//!   - v2 create/list/balance (останній balance_after), баланси всіх
//!   - валідації: 404 неіснуючий supplier, 400 невідомий тип
//!   - конкурентність: 2 паралельні create — обидва успішні, записи не
//!     втрачаються (count == 2)

use torgashka_domain::{LedgerEntriesQuery, LedgerEntryInput, LedgerError, LedgerService};
use torgashka_infrastructure::{db, repositories::ledger::SqlxLedger};
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    db::connect_test_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

fn repo(p: &sqlx::PgPool) -> SqlxLedger {
    SqlxLedger::new(torgashka_infrastructure::store_ctx::StorePool::new(p.clone()))
}

fn uniq() -> String {
    chrono::Utc::now().timestamp_micros().to_string()
}

async fn make_supplier(p: &sqlx::PgPool, name: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO suppliers (name, phone, address, created_at, updated_at) \
         VALUES ($1, '', '', now(), now()) RETURNING id",
    )
    .bind(name)
    .fetch_one(p)
    .await
    .expect("supplier insert")
}

async fn cleanup_supplier(p: &sqlx::PgPool, id: Uuid) {
    let _ = sqlx::query("DELETE FROM supplier_ledger WHERE supplier_id = $1")
        .bind(id)
        .execute(p)
        .await;
    let _ = sqlx::query("DELETE FROM suppliers WHERE id = $1")
        .bind(id)
        .execute(p)
        .await;
}

fn input_at(supplier_id: Uuid, amount: &str, op: &str, doc_num: &str, h: u32) -> LedgerEntryInput {
    LedgerEntryInput {
        supplier_id,
        amount: amount.to_string(),
        operation_type: op.to_string(),
        document_id: None,
        document_number: Some(doc_num.to_string()),
        operation_date: Some(
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7)
                .unwrap()
                .and_hms_opt(h, 0, 0)
                .unwrap(),
        ),
        notes: Some("test".to_string()),
    }
}

fn input(supplier_id: Uuid, amount: &str, op: &str, doc_num: &str) -> LedgerEntryInput {
    input_at(supplier_id, amount, op, doc_num, 10)
}

#[tokio::test]
async fn ledger_v1_create_history_balance() {
    let p = pool().await;
    let r = repo(&p);
    let sid = make_supplier(&p, &format!("E4T-V1-{}", uniq())).await;

    let e1 = r
        .create_entry_v1(&input_at(sid, "100.50", "invoice", "DOC-1", 10))
        .await
        .expect("v1 create 1");
    assert_eq!(e1.amount, "100.50"); // вхідна scale (identity map Python)
    assert_eq!(e1.balance_after, "100.50");
    assert_eq!(e1.operation_type, "invoice");
    let e2 = r
        .create_entry_v1(&input_at(sid, "50.25", "payment", "DOC-2", 11))
        .await
        .expect("v1 create 2");
    assert_eq!(e2.amount, "50.25");
    assert_eq!(e2.balance_after, "150.75"); // 100.50 + 50.25 (Decimal)

    let h = r.history_v1(sid, 1, 10).await.expect("history");
    assert_eq!(h.total, 2);
    assert_eq!(h.items.len(), 2);
    assert_eq!(h.items[0].operation_type, "payment"); // ORDER BY operation_date DESC

    let b = r.balance_v1(sid).await.expect("balance v1");
    assert_eq!(b.current_balance, "150.75"); // сума amount
    assert!(b.supplier_name.starts_with("E4T-V1-"));

    cleanup_supplier(&p, sid).await;
}

#[tokio::test]
async fn ledger_v2_list_balance_and_all() {
    let p = pool().await;
    let r = repo(&p);
    let sid = make_supplier(&p, &format!("E4T-V2-{}", uniq())).await;

    let mut inp = input(sid, "1.00", "invoice", "A");
    inp.operation_date = None; // v2: operation_date = now
    let _ = r.create_entry_v2(&inp).await.expect("v2 create 1");
    let _ = r.create_entry_v2(&inp).await.expect("v2 create 2");

    let q = LedgerEntriesQuery {
        page: 1,
        size: 10,
        supplier_id: Some(sid),
        operation_type: None,
        date_from: None,
        date_to: None,
    };
    let l = r.list_entries_v2(&q).await.expect("list v2");
    assert_eq!(l.total, 2);
    assert_eq!(l.items.len(), 2);
    // float amount (Pydantic float)
    assert_eq!(l.items[0].amount, 1.0);

    let b = r.balance_v2(sid).await.expect("balance v2");
    assert_eq!(b.balance, 2.0); // останній balance_after

    let all = r.all_balances_v2().await.expect("all balances");
    assert!(all.iter().any(|x| x.supplier_id == sid));

    cleanup_supplier(&p, sid).await;
}

#[tokio::test]
async fn ledger_validation_404_400() {
    let p = pool().await;
    let r = repo(&p);
    let sid = make_supplier(&p, &format!("E4T-VAL-{}", uniq())).await;
    let missing = Uuid::nil();

    // v1: неіснуючий supplier → 404 (всі три ендпойнти)
    assert!(matches!(
        r.create_entry_v1(&input(missing, "10.00", "invoice", "X"))
            .await,
        Err(LedgerError::NotFound(_))
    ));
    assert!(matches!(
        r.history_v1(missing, 1, 10).await,
        Err(LedgerError::NotFound(_))
    ));
    assert!(matches!(
        r.balance_v1(missing).await,
        Err(LedgerError::NotFound(_))
    ));

    // v1: невідомий тип → 400
    assert!(matches!(
        r.create_entry_v1(&input(sid, "10.00", "bad", "X")).await,
        Err(LedgerError::BadRequest(_))
    ));

    // v2: неіснуючий supplier → 400 (Python ValueError → 400)
    let mut inp = input(missing, "10.00", "invoice", "X");
    inp.operation_date = None;
    assert!(matches!(
        r.create_entry_v2(&inp).await,
        Err(LedgerError::BadRequest(_))
    ));
    // v2 balance → 404
    assert!(matches!(
        r.balance_v2(missing).await,
        Err(LedgerError::NotFound(_))
    ));
    // v2 entries невалідний operation_type → 500 (Python ValueError → 500)
    let q = LedgerEntriesQuery {
        page: 1,
        size: 10,
        supplier_id: None,
        operation_type: Some("bad".to_string()),
        date_from: None,
        date_to: None,
    };
    assert!(matches!(
        r.list_entries_v2(&q).await,
        Err(LedgerError::InvalidOperationType(_))
    ));

    cleanup_supplier(&p, sid).await;
}

#[tokio::test]
async fn ledger_concurrent_creates_no_loss() {
    let p = pool().await;
    let r = repo(&p);
    let sid = make_supplier(&p, &format!("E4T-CONC-{}", uniq())).await;

    let r1 = r.clone();
    let r2 = r.clone();
    let i1 = input(sid, "10.00", "invoice", "C1");
    let i2 = input(sid, "20.00", "invoice", "C2");
    let (a, b) = tokio::join!(async { r1.create_entry_v2(&i1).await }, async {
        r2.create_entry_v2(&i2).await
    },);
    assert!(
        a.is_ok() && b.is_ok(),
        "обидва create мають бути 201-еквівалент"
    );

    let cnt: i64 =
        sqlx::query_scalar("SELECT count(*) FROM supplier_ledger WHERE supplier_id = $1")
            .bind(sid)
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(cnt, 2, "жоден запис не втрачено");

    cleanup_supplier(&p, sid).await;
}
