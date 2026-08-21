//! Інтеграційний тест: готівкові операції «Внесення»/«Інкасація» (Етап 3).
//!
//! Перевіряє на живій БД:
//!   1. `create_cash_operation` (deposit/collection) — INSERT + RETURNING,
//!      user_name підтягується з users (JOIN), amount зберігає scale колонки.
//!   2. `list_cash_operations` — список від найновіших + balance =
//!      SUM(deposit) − SUM(collection) для точки.
//!
//! Потрібна жива БД (як snapshot-тести); тест прибирає за собою записи.

use sqlx::PgPool;
use uuid::Uuid;

use torgashka_domain::{
    CashOperationCreateInput, CashOperationType, CashType, PosService,
};
use torgashka_infrastructure::repositories::pos::SqlxPos;
use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};

/// «Білий магазин» + власник (ФОП Мельничук) — контекст запиту.
const STORE_ID: &str = "65d5db51-672f-4a38-9c1e-f36c5feb5374";
const OWNER_ID: &str = "e30d480c-ef3b-4d0e-8808-0c745196d3d8";
const TEST_MARK: &str = "__test_cash_op__";

/// Пул лише до ТЕСТОВОЇ БД (TEST_DATABASE_URL або <dbname>_test) — ізоляція від робочої.
async fn pool() -> PgPool {
    torgashka_infrastructure::db::connect_test_pool(2)
        .await
        .expect("тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test")
}

async fn cleanup(pool: &PgPool) {
    // Тестова БД повністю ізольована (pos_system_*_test) — видаляємо ВСІ
    // операції, щоб тест був детермінованим незалежно від клонованих даних.
    let _ = sqlx::query("DELETE FROM cash_operations")
        .execute(pool)
        .await;
}

#[tokio::test]
async fn deposit_and_collection_roundtrip_with_balance() {
    let pool = pool().await;
    cleanup(&pool).await;
    let store_id = Uuid::parse_str(STORE_ID).unwrap();
    let user_id = Uuid::parse_str(OWNER_ID).unwrap();
    let ctx = StoreCtx {
        user_id,
        store_id,
        role: "owner".to_string(),
    };
    let svc = SqlxPos::new(StorePool::new(pool.clone()));

    with_store_ctx(ctx, async {
        // Внесення 500.00 «Розмін».
        let deposit = svc
            .create_cash_operation(
                store_id,
                user_id,
                &CashOperationCreateInput {
                    operation_type: CashOperationType::Deposit,
                    cash_type: CashType::Cash,
                    amount: "500.00".parse().unwrap(),
                    comment: Some(format!("{TEST_MARK} розмін")),
                },
            )
            .await
            .expect("deposit створюється");
        assert_eq!(deposit.operation_type, CashOperationType::Deposit);
        assert_eq!(deposit.cash_type, CashType::Cash);
        assert_eq!(deposit.amount.to_string(), "500.00");
        assert_eq!(deposit.user_name, "ФОП Мельничук");
        assert_eq!(deposit.store_id, store_id);
        assert_eq!(deposit.user_id, user_id);

        // Інкасація 200.00.
        let collection = svc
            .create_cash_operation(
                store_id,
                user_id,
                &CashOperationCreateInput {
                    operation_type: CashOperationType::Collection,
                    cash_type: CashType::Card,
                    amount: "200.00".parse().unwrap(),
                    comment: Some(format!("{TEST_MARK} інкасація")),
                },
            )
            .await
            .expect("collection створюється");
        assert_eq!(collection.operation_type, CashOperationType::Collection);
        assert_eq!(collection.cash_type, CashType::Card);
        assert_eq!(collection.amount.to_string(), "200.00");

        // Список + баланси: cash 500.00 (лише deposit), card −200.00 (лише collection).
        let list = svc
            .list_cash_operations(store_id)
            .await
            .expect("список читається");
        assert_eq!(list.operations.len(), 2);
        assert_eq!(list.balances.cash.to_string(), "500.00");
        assert_eq!(list.balances.card.to_string(), "-200.00");
        // Найновіша операція перша (ORDER BY created_at DESC).
        assert_eq!(list.operations[0].operation_type, CashOperationType::Collection);
        assert_eq!(list.operations[1].operation_type, CashOperationType::Deposit);
        // user_name присутній у кожному рядку списку.
        assert!(list
            .operations
            .iter()
            .all(|o| o.user_name == "ФОП Мельничук"));

        // Порожня точка іншого магазину → порожній список, баланс 0.
        let other = Uuid::parse_str("5e840d11-6b9b-4f6f-a6e4-000d1bb0a307").unwrap();
        let empty = svc.list_cash_operations(other).await.expect("інша точка");
        assert!(empty.operations.is_empty());
        assert_eq!(empty.balances.cash.to_string(), "0");
        assert_eq!(empty.balances.card.to_string(), "0");
    })
    .await;

    cleanup(&pool).await;
}
