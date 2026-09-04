//! Інтеграційний тест: ізоляція налаштувань за торговельною точкою (Етап 3).
//!
//! Перевіряє:
//!   1. `create_store` копіює налаштування (system_settings) і шаблони друку
//!      (print_templates) з ПОТОЧНОЇ точки (ctx.store_id) у нову — в межах
//!      однієї транзакції.
//!   2. `fetch_settings` (settings_all) повертає ТІЛЬКИ налаштування поточної
//!      точки (явний фільтр `store_id = NULLIF(current_setting('app.store_id', true), '')::uuid`).
//!   3. `settings_update_key` оновлює запис ТІЛЬКИ поточної точки — значення
//!      в іншій точці не змінюється.
//!
//! Потрібна жива БД (як snapshot-тести); гейтиться на доступність.

use sqlx::PgPool;
use uuid::Uuid;

use torgashka_domain::{AuthService, StoreCreateInput, StoreService};
use torgashka_infrastructure::repositories::auth::SqlxAuth;
use torgashka_infrastructure::repositories::stores::SqlxStoreService;
use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};

/// «Білий магазин» — точка-донор (всі 30 налаштувань + шаблони друку).
const SOURCE_STORE: &str = "65d5db51-672f-4a38-9c1e-f36c5feb5374";

/// Пул до живої БД (як snapshot-тести). Тест гейтиться на доступність БД.
/// Пул лише до ТЕСТОВОЇ БД (TEST_DATABASE_URL або <dbname>_test) — ізоляція від робочої.
async fn pool() -> PgPool {
    torgashka_infrastructure::db::connect_test_pool(2)
        .await
        .expect("тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test")
}

/// Рахунок активних рядків (is_active = true) — той самий предикат, що й у
/// копіюванні create_store (неактивні шаблони/налаштування не копіюються).
async fn count_active_rows(pool: &PgPool, table: &str, store_id: Uuid) -> i64 {
    let sql = format!("SELECT count(*) FROM {table} WHERE store_id = $1 AND is_active = true");
    sqlx::query_scalar(&sql)
        .bind(store_id)
        .fetch_one(pool)
        .await
        .expect("count працює")
}

/// Видаляє всі тестові точки (залишки попередніх запусків) + конкретну нову.
/// FK каскадять: system_settings, print_templates, user_stores.
async fn cleanup_test_stores(pool: &PgPool, new_store: Option<Uuid>) {
    if let Some(id) = new_store {
        let _ = sqlx::query("DELETE FROM stores WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await;
    }
    let _ = sqlx::query("DELETE FROM stores WHERE name LIKE '\\__test\\_store\\_%' ESCAPE '\\'")
        .execute(pool)
        .await;
}

/// Кількість налаштувань у відповіді settings_all() (DTO, а не SQL).
fn dto_settings_count(dto: &torgashka_domain::SettingsModulesDto) -> usize {
    dto.modules
        .values()
        .map(|v| v.as_array().map(|a| a.len()).unwrap_or(0))
        .sum()
}

#[tokio::test]
async fn create_store_copies_settings_and_templates_per_store() {
    let pool = pool().await;
    let store_pool = StorePool::new(pool.clone());
    cleanup_test_stores(&pool, None).await;
    let source_store = Uuid::parse_str(SOURCE_STORE).expect("source uuid");

    // Owner «Білого магазину» — потрібен для StoreCtx (user_id) і user_stores.
    let owner_id: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM user_stores WHERE store_id = $1 AND role = 'owner' ORDER BY created_at LIMIT 1",
    )
    .bind(source_store)
    .fetch_one(&pool)
    .await
    .expect("owner Білого магазину існує");

    let source_ctx = StoreCtx {
        user_id: owner_id,
        store_id: source_store,
        role: "owner".to_string(),
    };

    // Еталонні кількості у точці-донорі.
    let src_settings = count_active_rows(&pool, "system_settings", source_store).await;
    let src_templates = count_active_rows(&pool, "print_templates", source_store).await;
    assert!(src_settings > 0, "точка-донор має налаштування");
    assert!(src_templates > 0, "точка-донор має шаблони друку");

    // 1) Створення точки через репозиторій (реальний код-шлях create_store).
    let name = format!(
        "__test_store_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp()
    );
    let input = StoreCreateInput {
        name,
        address: None,
        phone: None,
        legal_name: None,
        edrpou: None,
    };
    let svc = SqlxStoreService::new(store_pool.clone());
    let dto = with_store_ctx(source_ctx.clone(), async { svc.create_store(&input).await })
        .await
        .expect("create_store виконується");
    let new_store = dto.id;

    // 2) Копіювання: нова точка має СТІЛЬКИ ж налаштувань і шаблонів.
    let new_settings = count_active_rows(&pool, "system_settings", new_store).await;
    let new_templates = count_active_rows(&pool, "print_templates", new_store).await;
    assert_eq!(
        new_settings, src_settings,
        "у нову точку скопійовано всі налаштування донора"
    );
    assert_eq!(
        new_templates, src_templates,
        "у нову точку скопійовано всі шаблони друку донора"
    );

    // 3) fetch_settings (settings_all) у контексті НОВОЇ точки → свої 30.
    let auth = SqlxAuth::new(store_pool.clone());
    let new_ctx = StoreCtx {
        user_id: owner_id,
        store_id: new_store,
        role: "owner".to_string(),
    };
    let new_dto = with_store_ctx(new_ctx.clone(), async { auth.settings_all().await })
        .await
        .expect("settings_all для нової точки");
    assert_eq!(
        dto_settings_count(&new_dto),
        src_settings as usize,
        "нова точка бачить свої налаштування"
    );

    // 4) fetch_settings у контексті ДОНОРА → свої (не змінились після копіювання).
    let src_dto = with_store_ctx(source_ctx.clone(), async { auth.settings_all().await })
        .await
        .expect("settings_all для донора");
    assert_eq!(
        dto_settings_count(&src_dto),
        src_settings as usize,
        "донор бачить свої налаштування"
    );

    // 5) settings_update_key у НОВІЙ точці не зачіпає донора.
    let first_key = new_dto
        .modules
        .values()
        .find_map(|v| v.as_array().and_then(|a| a.first()))
        .and_then(|o| o.get("key"))
        .and_then(|k| k.as_str())
        .expect("є хоча б одне налаштування")
        .to_string();
    with_store_ctx(new_ctx.clone(), async {
        auth.settings_update_key(&first_key, Some("__test_store_value__".to_string()))
            .await
    })
    .await
    .expect("settings_update_key виконується");

    let new_val: Option<String> =
        sqlx::query_scalar("SELECT value FROM system_settings WHERE store_id = $1 AND key = $2")
            .bind(new_store)
            .bind(&first_key)
            .fetch_one(&pool)
            .await
            .expect("значення у новій точці");
    let src_val: Option<String> =
        sqlx::query_scalar("SELECT value FROM system_settings WHERE store_id = $1 AND key = $2")
            .bind(source_store)
            .bind(&first_key)
            .fetch_one(&pool)
            .await
            .expect("значення у донора");

    assert_eq!(
        new_val.as_deref(),
        Some("__test_store_value__"),
        "оновлено запис саме нової точки"
    );
    assert_ne!(
        src_val.as_deref(),
        Some("__test_store_value__"),
        "запис донора не змінений"
    );

    // Прибирання тестової точки (FK каскадять усе).
    cleanup_test_stores(&pool, Some(new_store)).await;
}
