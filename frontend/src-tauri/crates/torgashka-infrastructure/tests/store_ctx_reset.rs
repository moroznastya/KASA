//! Регресійний тест: reset_config у StorePool не має ламати наступний логін.
//!
//! Відтворений runtime-баг (Етап 5):
//!   1. Запит з X-Store-Id проставляє `app.store_id` на з'єднанні пула;
//!   2. reset_config скидає контекст — але для custom-параметрів PostgreSQL
//!      (з крапкою) reset лишає `current_setting(..., true) = ''` (НЕ NULL);
//!   3. Наступний логін на цьому з'єднанні виконує
//!      `COALESCE(current_setting('app.store_id', true)::uuid, NULL)` —
//!      `''::uuid` кидає `invalid input syntax for type uuid` → 500.
//!
//! PostgreSQL-квірк (перевірено емпірично): `set_config(..., NULL)`, `RESET`
//! і навіть `DISCARD ALL` НЕ повертають NULL для вже встановленого
//! custom-параметра — лишають `''`. Єдиний безпечний стан — свіже з'єднання.
//!
//! Фікс: кожен каст `current_setting('app.store_id', true)::uuid` обгорнуто
//! `NULLIF(..., '')` — '' трактується як NULL (рівно як свіже з'єднання).
//! Цей тест доводить, що після set+reset нова форма касту дає NULL без
//! помилки, а стара — падає (документація необхідності NULLIF).

use sqlx::PgPool;

/// Пул до живої БД (як snapshot-тести). Тест гейтиться на доступність БД.
/// Пул лише до ТЕСТОВОЇ БД (TEST_DATABASE_URL або <dbname>_test) — ізоляція від робочої.
async fn pool() -> PgPool {
    torgashka_infrastructure::db::connect_test_pool(2)
        .await
        .expect("тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test")
}

const STORE_UUID: &str = "d9be9608-c011-49be-b776-3317ca5e9af6";

/// Симуляція reset_config: точний SQL з store_ctx.rs (set_config NULL ≡ RESET).
async fn reset_config(conn: &mut sqlx::PgConnection) {
    sqlx::query(
        "SELECT set_config('app.user_id', $1, false), set_config('app.store_id', $2, false)",
    )
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .execute(&mut *conn)
    .await
    .expect("reset_config SQL виконується");
}

/// Симуляція set_config зі store_ctx.rs (is_local=false — session-level).
async fn set_config(conn: &mut sqlx::PgConnection, user_id: &str, store_id: &str) {
    sqlx::query(
        "SELECT set_config('app.user_id', $1, false), set_config('app.store_id', $2, false)",
    )
    .bind(user_id)
    .bind(store_id)
    .execute(&mut *conn)
    .await
    .expect("set_config SQL виконується");
}

/// Після set+reset current_setting має трактуватись як NULL (не '' і не UUID).
/// Стара форма касту падає — саме тому потрібен NULLIF.
#[tokio::test]
async fn reset_leaves_nullif_safe_state_not_empty_string() {
    let pool = pool().await;
    let mut conn = pool.acquire().await.expect("acquire");

    // 1. Store-запит: контекст точки проставлено на з'єднанні.
    set_config(
        &mut conn,
        "00000000-0000-0000-0000-000000000001",
        STORE_UUID,
    )
    .await;
    let before: String = sqlx::query_scalar("SELECT current_setting('app.store_id', true)")
        .fetch_one(&mut *conn)
        .await
        .expect("store_id встановлено");
    assert_eq!(before, STORE_UUID, "контекст проставлено до reset");

    // 2. reset_config — з'єднання повертається в пул.
    reset_config(&mut conn).await;

    // 3. Квірк: current_setting лишає '' (не NULL) — документуємо факт.
    let after: String = sqlx::query_scalar("SELECT current_setting('app.store_id', true)")
        .fetch_one(&mut *conn)
        .await
        .expect("current_setting після reset");
    assert_eq!(
        after, "",
        "PostgreSQL-квірк: reset custom-параметра лишає ''"
    );

    // 4. СТАРА форма (баг): COALESCE(...)::uuid на '' → помилка касту.
    let old_crash = sqlx::query_scalar::<_, Option<uuid::Uuid>>(
        "SELECT COALESCE(current_setting('app.store_id', true)::uuid, NULL)",
    )
    .fetch_one(&mut *conn)
    .await;
    assert!(
        old_crash.is_err(),
        "стара форма касту мала б впасти на '' (відтворення бага)"
    );

    // 5. НОВА форма (фікс): NULLIF(..., '') → NULL, без помилки.
    let store_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(current_setting('app.store_id', true), '')::uuid, NULL)",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("нова форма касту не падає");
    assert_eq!(
        store_id, None,
        "'' трактується як NULL (рівно як свіже з'єднання)"
    );

    let user_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(current_setting('app.user_id', true), '')::uuid, NULL)",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("user_id каст не падає");
    assert_eq!(user_id, None, "'' для app.user_id теж трактується як NULL");
}

/// Повний runtime-сценарій (критерій 2 на рівні БД):
/// логін → /products з X-Store-Id → 200 → НАСТУПНИЙ логін → 200.
/// Тут: set → запит → reset → «наступний логін» (INSERT-патерн з auth.rs)
/// виконується без помилки на ТОМУ Ж з'єднанні.
#[tokio::test]
async fn login_after_store_request_does_not_crash() {
    let pool = pool().await;
    let mut conn = pool.acquire().await.expect("acquire");

    // Store-запит /products з X-Store-Id: контекст + запит + reset.
    set_config(
        &mut conn,
        "00000000-0000-0000-0000-000000000001",
        STORE_UUID,
    )
    .await;
    let _products_ok: i32 = sqlx::query_scalar(
        "SELECT 1 WHERE current_setting('app.store_id', true)::uuid IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("store-запит виконується з валідним UUID");
    reset_config(&mut conn).await;

    // Наступний логін на тому ж з'єднанні: INSERT-патерн create_work_session
    // з auth.rs — значення store_id через NULLIF-каст ('' → NULL), не падає.
    let store_id_for_session: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(current_setting('app.store_id', true), '')::uuid, NULL)",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("логін-патерн не падає після store-запиту");
    assert_eq!(
        store_id_for_session, None,
        "після reset логін бачить store_id = NULL (точка не визначена)"
    );
}
