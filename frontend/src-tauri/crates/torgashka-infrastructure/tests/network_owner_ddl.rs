//! Інтеграційний тест: мережевий рівень власника (5 owner-таблиць + enum
//! device_status) створюється через `ensure_schema` ідемпотентно.
//!
//! Покриває обидва шляхи застосування:
//!   - fresh-БД (порожня, без `users`) → SCHEMA_SQL (включно з новими
//!     таблицями в schema.sql) + OWNERS/CASH_OPS/NETWORK DDL;
//!   - вже мігрована БД (з `users`) → тільки OWNERS/CASH_OPS/NETWORK DDL.
//!
//! Запуск (як інші integration-тести проєкту):
//!   TEST_DATABASE_URL=postgresql://…/порожня_або_мігрована_бд \
//!       cargo test -p torgashka-infrastructure --test network_owner_ddl

use torgashka_infrastructure::db::{connect_test_pool, ensure_schema};

/// 5 таблиць мережевого рівня + enum device_status.
const NETWORK_TABLES: [&str; 5] = [
    "public.devices",
    "public.store_activation_codes",
    "public.store_product_prices",
    "public.audit_log",
    "public.store_sync_state",
];

async fn assert_network_objects(p: &sqlx::PgPool) {
    for t in NETWORK_TABLES {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(t)
            .fetch_one(p)
            .await
            .expect("query to_regclass");
        assert!(exists, "таблиця {t} не створена");
    }
    let enum_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'device_status')")
            .fetch_one(p)
            .await
            .expect("query pg_type");
    assert!(enum_exists, "enum device_status не створено");
    // enum-значення у правильному порядку
    let vals: Vec<String> = sqlx::query_scalar(
        "SELECT enumlabel FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid \
         WHERE t.typname = 'device_status' ORDER BY e.enumsortorder",
    )
    .fetch_all(p)
    .await
    .expect("query pg_enum");
    assert_eq!(
        vals,
        vec!["pending", "active", "blocked", "deleted"],
        "значення device_status"
    );
}

/// ensure_schema() виконується двічі підряд без помилок; після цього всі
/// 5 таблиць та enum існують (перший виклик створює, другий — no-op).
#[tokio::test]
async fn ensure_schema_network_ddl_idempotent() {
    let p = connect_test_pool(5)
        .await
        .expect("тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test");
    ensure_schema(&p).await.expect("ensure_schema (1-й виклик)");
    ensure_schema(&p).await.expect("ensure_schema (2-й виклик)");
    assert_network_objects(&p).await;
}
