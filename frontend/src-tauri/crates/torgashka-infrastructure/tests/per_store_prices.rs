//! Інтеграційний тест: per-store ціни товарів (Етап 3.1).
//!
//! Схема: `stock(store_id, product_id, quantity, price)` — ціна товару
//! ПЕРЕВИЗНАЧУЄТЬСЯ на рівні точки; `products.price` — глобальний дефолт.
//! Семантика: `stock.price = 0` = «без перевизначення» (SELECT читає
//! `COALESCE(NULLIF(st.price, 0), p.price)`).
//!
//! Перевіряє (критерій прийняття задачі):
//!   1. Створення товару в точці A з ціною 100 → точка B бачить 100 (глобальна).
//!   2. Зміна ціни в точці A на 150 → точка A бачить 150, точка B — ДОСІ 100
//!      (зміна ціни в одній точці НЕ впливає на іншу).
//!   3. Зміна ЛИШЕ ціни → quantity точки A не загубилась.
//!   4. v2-шлях (products_v2): та сама ізоляція update/get.

use torgashka_domain::{
    ProductCreateInput, ProductUpdateInput, ProductsV2Service, ReadDirectories, WriteDirectories,
};
use torgashka_infrastructure::repositories::directories::SqlxDirectories;
use torgashka_infrastructure::repositories::products_v2::SqlxProductsV2;
use torgashka_infrastructure::repositories::write::SqlxWriteDirectories;
use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};
use uuid::Uuid;

/// Пул до живої тестової БД (як інші integration-тести проєкту).
async fn pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_test_pool(5)
        .await
        .expect("тестова БД недоступна: задайте TEST_DATABASE_URL або створіть <dbname>_test")
}

/// Унікальний суфікс для barcode/sku/title.
fn uniq() -> String {
    chrono::Utc::now().timestamp_micros().to_string()
}

/// Точка A — «Білий магазин», точка B — «Жовтий магазин»; власник має доступ до обох.
const STORE_A: &str = "65d5db51-672f-4a38-9c1e-f36c5feb5374";
const STORE_B: &str = "5e840d11-6b9b-4f6f-a6e4-000d1bb0a307";
const OWNER_ID: &str = "e30d480c-ef3b-4d0e-8808-0c745196d3d8";

fn ctx(store_id: Uuid) -> StoreCtx {
    StoreCtx {
        user_id: Uuid::parse_str(OWNER_ID).unwrap(),
        store_id,
        role: "owner".to_string(),
    }
}

/// Пряме видалення (cleanup) — обходить бізнес-правила.
async fn cleanup_product(p: &sqlx::PgPool, id: Uuid) {
    let _ = sqlx::query("DELETE FROM stock WHERE product_id = $1")
        .bind(id)
        .execute(p)
        .await;
    let _ = sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(id)
        .execute(p)
        .await;
}

fn create_input(ts: &str) -> ProductCreateInput {
    ProductCreateInput {
        barcode: Some(format!("{ts}p")),
        sku: Some(format!("SKU-{ts}")),
        title: format!("ТЕСТ-PERSTORE-{ts}"),
        description: None,
        price: Some("100.00".into()),
        cost_price: Some("50.00".into()),
        markup: None,
        stock: Some("16".into()),
        recommended_qty: None,
        uktzed: None,
        scan_excise: false,
        tax_rate: Some("20.00".into()),
        tax_group: Some("А".into()),
        is_weight: false,
        unit: Some("шт".into()),
        category_id: None,
        supplier_id: None,
    }
}

#[tokio::test]
async fn per_store_price_isolation_v1() {
    let p = pool().await;
    let store_pool = StorePool::new(p.clone());
    let write = SqlxWriteDirectories::new(store_pool.clone());
    let read = SqlxDirectories::new(store_pool.clone());
    let ts = uniq();
    let a = Uuid::parse_str(STORE_A).unwrap();
    let b = Uuid::parse_str(STORE_B).unwrap();
    let ctx_a = ctx(a);
    let ctx_b = ctx(b);

    // 1) Створення в точці A: price 100.00, stock 16.
    let created = with_store_ctx(ctx_a.clone(), async {
        write.create_product(&create_input(&ts)).await
    })
    .await
    .expect("create product in A");

    // У точці A ціна 100.00 (stock A записано з price 100.00).
    let in_a = with_store_ctx(ctx_a.clone(), async { read.get_product(created.id).await })
        .await
        .expect("get in A");
    assert_eq!(in_a.price.as_deref(), Some("100.00"), "точка A: ціна 100");

    // У точці B ціна 100.00 (глобальний дефолт; stock B відсутній).
    let in_b = with_store_ctx(ctx_b.clone(), async { read.get_product(created.id).await })
        .await
        .expect("get in B");
    assert_eq!(
        in_b.price.as_deref(),
        Some("100.00"),
        "точка B: глобальна 100"
    );

    // 2) Зміна ціни ЛИШЕ в точці A: 100 → 150.
    let upd = ProductUpdateInput {
        price: Some(Some("150.00".into())),
        ..Default::default()
    };
    let updated = with_store_ctx(ctx_a.clone(), async {
        write.update_product(created.id, &upd).await
    })
    .await
    .expect("update price in A");
    assert_eq!(
        updated.price.as_deref(),
        Some("150.00"),
        "PUT-відповідь: нова ціна точки A"
    );

    // Точка A тепер бачить 150.
    let in_a2 = with_store_ctx(ctx_a.clone(), async { read.get_product(created.id).await })
        .await
        .expect("get in A after update");
    assert_eq!(in_a2.price.as_deref(), Some("150.00"), "точка A: ціна 150");

    // Точка B ДОСІ бачить 100 — зміна ціни в A не глобальна.
    let in_b2 = with_store_ctx(ctx_b.clone(), async { read.get_product(created.id).await })
        .await
        .expect("get in B after update");
    assert_eq!(
        in_b2.price.as_deref(),
        Some("100.00"),
        "точка B: ціна НЕ змінилась (per-store ізоляція)"
    );

    // 3) quantity точки A збережено (16.000) при зміні лише ціни.
    let qty: String = sqlx::query_scalar(
        "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(a)
    .bind(created.id)
    .fetch_one(&p)
    .await
    .expect("stock quantity A");
    assert_eq!(qty, "16.000", "зміна лише ціни не загубила quantity");

    // Глобальна products.price НЕ змінилась (150 записано лише в stock A).
    let global_price: String = sqlx::query_scalar("SELECT price::text FROM products WHERE id = $1")
        .bind(created.id)
        .fetch_one(&p)
        .await
        .expect("products.price");
    assert_eq!(
        global_price, "100.00",
        "products.price залишається глобальним дефолтом"
    );

    cleanup_product(&p, created.id).await;
}

#[tokio::test]
async fn per_store_price_isolation_v2() {
    let p = pool().await;
    let store_pool = StorePool::new(p.clone());
    let svc = SqlxProductsV2::new(store_pool.clone());
    let ts = uniq();
    let a = Uuid::parse_str(STORE_A).unwrap();
    let b = Uuid::parse_str(STORE_B).unwrap();
    let ctx_a = ctx(a);
    let ctx_b = ctx(b);

    // Створення в точці A: price 100.0, quantity 5.
    let create = torgashka_domain::ProductCreateV2Input {
        name: Some(format!("ТЕСТ-V2-PERSTORE-{ts}")),
        barcode: Some(format!("{ts}v2")),
        price: Some(100.0),
        cost_price: Some(50.0),
        quantity: Some(5.0),
        unit: Some("шт".into()),
        category_id: None,
        supplier_id: None,
        sku: None,
        description: None,
    };
    let created = with_store_ctx(ctx_a.clone(), async { svc.create(&create).await })
        .await
        .expect("v2 create in A");
    assert_eq!(created.price, Some(100.0));

    // Точка B бачить глобальну 100.0.
    let in_b = with_store_ctx(ctx_b.clone(), async { svc.get(created.id).await })
        .await
        .expect("v2 get in B");
    assert_eq!(in_b.price, Some(100.0), "v2 точка B: глобальна 100");

    // Зміна ціни лише в A: 100 → 150.
    let upd = torgashka_domain::ProductUpdateV2Input {
        price: Some(150.0),
        ..Default::default()
    };
    with_store_ctx(ctx_a.clone(), async { svc.update(created.id, &upd).await })
        .await
        .expect("v2 update price in A");

    let in_a2 = with_store_ctx(ctx_a.clone(), async { svc.get(created.id).await })
        .await
        .expect("v2 get in A after update");
    assert_eq!(in_a2.price, Some(150.0), "v2 точка A: ціна 150");

    let in_b2 = with_store_ctx(ctx_b.clone(), async { svc.get(created.id).await })
        .await
        .expect("v2 get in B after update");
    assert_eq!(in_b2.price, Some(100.0), "v2 точка B: ціна НЕ змінилась");

    // quantity точки A збережено при зміні лише ціни.
    let qty: String = sqlx::query_scalar(
        "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(a)
    .bind(created.id)
    .fetch_one(&p)
    .await
    .expect("v2 stock quantity A");
    assert_eq!(qty, "5.000", "v2 зміна лише ціни не загубила quantity");

    cleanup_product(&p, created.id).await;
}
