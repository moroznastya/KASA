//! Інтеграційні тести write-операцій (етап 2): CRUD довідників,
//! інвентаризація та КОНКУРЕНТНІСТЬ проведення.
//!
//! Потребують доступної PostgreSQL (як Python-еталон):
//! `DATABASE_URL` або `DB_*` у backend/.env.
//!
//! Конкурентність: два паралельні `confirm_inventory` на одному товарі —
//! `SELECT ... FOR UPDATE` серіалізує оновлення, кінцевий залишок = сума
//! різниць, нуль втрат.

use torgashka_domain::{
    CategoryCreateInput, InventoryCreateInput, InventoryItemInput, ProductCreateInput,
    ProductUpdateInput, SupplierCreateInput, WriteDirectories, WriteError,
};
use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};
use torgashka_infrastructure::{db, repositories::write::SqlxWriteDirectories};
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    db::connect_test_pool(5)
        .await
        .expect("БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

fn repo(p: &sqlx::PgPool) -> SqlxWriteDirectories {
    SqlxWriteDirectories::new(torgashka_infrastructure::store_ctx::StorePool::new(
        p.clone(),
    ))
}

fn uniq() -> String {
    chrono::Utc::now().timestamp_micros().to_string()
}

/// Перший валідний user id (created_by для інвентаризацій).
async fn any_user_id(p: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users ORDER BY created_at LIMIT 1")
        .fetch_one(p)
        .await
        .expect("у БД має бути хоча б один користувач")
}

/// «Білий магазин» + власник (ФОП Мельничук) — контекст точки для write-операцій.
const STORE_ID: &str = "65d5db51-672f-4a38-9c1e-f36c5feb5374";
const OWNER_ID: &str = "e30d480c-ef3b-4d0e-8808-0c745196d3d8";

/// Пряме видалення (cleanup) — обходить бізнес-правила.
async fn cleanup_product(p: &sqlx::PgPool, id: Uuid) {
    let _ = sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(id)
        .execute(p)
        .await;
}

#[tokio::test]
async fn product_crud_flow() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let store_id = Uuid::parse_str(STORE_ID).unwrap();
    let owner_id = Uuid::parse_str(OWNER_ID).unwrap();
    let ctx = StoreCtx {
        user_id: owner_id,
        store_id,
        role: "owner".to_string(),
    };
    with_store_ctx(ctx, async {
        let r = repo(&p);
        let ts = uniq();

        // Create → 201-еквівалент: вхідні значення збережено.
        let input = ProductCreateInput {
            barcode: Some(format!("{ts}01")),
            sku: Some(format!("SKU-{ts}")),
            title: format!("ТЕСТ-ТОВАР-{ts}"),
            description: Some("тест".to_string()),
            price: Some("142.7".into()),
            cost_price: Some("87.23".into()),
            markup: None,
            stock: Some("16".into()),
            recommended_qty: Some("5".into()),
            uktzed: Some("4820".into()),
            scan_excise: false,
            tax_rate: Some("20".into()),
            tax_group: Some("А".into()),
            is_weight: false,
            unit: Some("шт".into()),
            category_id: None,
            supplier_id: None,
        };
        let created = r.create_product(&input).await.expect("create product");
        assert_eq!(created.title, format!("ТЕСТ-ТОВАР-{ts}"));
        // Вхідна scale збережена (як Python identity map).
        assert_eq!(created.price.as_deref(), Some("142.7"));
        assert_eq!(created.stock.as_deref(), Some("16"));
        // markup розраховано (HALF_EVEN): (142.7-87.23)/87.23*100 = 63.59.
        assert_eq!(created.markup.as_deref(), Some("63.59"));
        // tax_rate default scale 0 (вхідне "20").
        assert_eq!(created.tax_rate.as_deref(), Some("20"));

        // Конфлікт barcode → 409.
        let dup = ProductCreateInput {
            title: format!("ДУБЛЬ-{ts}"),
            ..input.clone()
        };
        let err = r.create_product(&dup).await.unwrap_err();
        assert!(
            matches!(&err, WriteError::Conflict(msg) if msg.contains("вже існує")),
            "очікувався Conflict, отримано: {err:?}"
        );

        // Update: тільки title + price → відповідь: title новий, price вхідний,
        // markup перераховано з (нового price, старого cost_price з БД).
        let upd = ProductUpdateInput {
            title: Some(format!("ТЕСТ-ТОВАР-{ts}-NEW")),
            price: Some(Some("150.00".into())),
            ..ProductUpdateInput::default()
        };
        let updated = r
            .update_product(created.id, &upd)
            .await
            .expect("update product");
        assert_eq!(updated.title, format!("ТЕСТ-ТОВАР-{ts}-NEW"));
        assert_eq!(updated.price.as_deref(), Some("150.00"));
        // (150-87.23)/87.23*100 = 71.96 (HALF_EVEN).
        assert_eq!(updated.markup.as_deref(), Some("71.96"));

        // 404 на неіснуючий ID.
        let missing = r.update_product(Uuid::new_v4(), &upd).await.unwrap_err();
        assert!(matches!(missing, WriteError::NotFound(_)));

        // Delete з ненульовим залишком (stock 16) → 400.
        let err = r.delete_product(created.id).await.unwrap_err();
        assert!(
            matches!(&err, WriteError::BadRequest(msg) if msg.contains("залишок на складі")),
            "очікувався BadRequest, отримано: {err:?}"
        );

        // Обнуляємо stock → delete успішний.
        let upd2 = ProductUpdateInput {
            stock: Some(Some("0".into())),
            ..ProductUpdateInput::default()
        };
        r.update_product(created.id, &upd2)
            .await
            .expect("zero stock");
        r.delete_product(created.id).await.expect("delete product");

        // 404 після видалення.
        let err = r.delete_product(created.id).await.unwrap_err();
        assert!(matches!(&err, WriteError::NotFound(_)));

        cleanup_product(&p, created.id).await;
    })
    .await;
}

#[tokio::test]
async fn category_supplier_crud_flow() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();

    // Category create.
    let cat = r
        .create_category(&CategoryCreateInput {
            name: format!("ТЕСТ-КАТ-{ts}"),
            description: Some("тест".into()),
            parent_id: None,
        })
        .await
        .expect("create category");
    assert_eq!(cat.name, format!("ТЕСТ-КАТ-{ts}"));

    // Category update: зміна назви.
    let upd = torgashka_domain::CategoryUpdateInput {
        name: Some(format!("ТЕСТ-КАТ-{ts}-NEW")),
        ..Default::default()
    };
    let cat2 = r
        .update_category(cat.id, &upd)
        .await
        .expect("update category");
    assert_eq!(cat2.name, format!("ТЕСТ-КАТ-{ts}-NEW"));

    // Category: власна батьківська → 400.
    let self_parent = torgashka_domain::CategoryUpdateInput {
        parent_id: Some(Some(cat.id)),
        ..Default::default()
    };
    let err = r.update_category(cat.id, &self_parent).await.unwrap_err();
    assert!(matches!(&err, WriteError::BadRequest(msg) if msg.contains("власною батьківською")));

    // Category delete.
    r.delete_category(cat.id).await.expect("delete category");
    let err = r.delete_category(cat.id).await.unwrap_err();
    assert!(matches!(&err, WriteError::NotFound(_)));

    // Supplier create/update/delete.
    let sup = r
        .create_supplier(&SupplierCreateInput {
            name: format!("ТЕСТ-ПОСТАЧ-{ts}"),
            edrpou: Some("12345678".into()),
            phone: Some("+380501234567".into()),
            email: Some("t@t.ua".into()),
            address: Some("Київ".into()),
            notes: Some("тест".into()),
        })
        .await
        .expect("create supplier");
    assert_eq!(sup.current_balance, "0.00");
    let sup_upd = torgashka_domain::SupplierUpdateInput {
        name: Some(format!("ТЕСТ-ПОСТАЧ-{ts}-NEW")),
        ..Default::default()
    };
    let sup2 = r
        .update_supplier(sup.id, &sup_upd)
        .await
        .expect("update supplier");
    assert_eq!(sup2.name, format!("ТЕСТ-ПОСТАЧ-{ts}-NEW"));
    r.delete_supplier(sup.id).await.expect("delete supplier");
}

#[tokio::test]
async fn inventory_confirm_cancel_flow() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let user = any_user_id(&p).await;
    let store_id = Uuid::parse_str(STORE_ID).unwrap();
    let owner_id = Uuid::parse_str(OWNER_ID).unwrap();
    let ctx = StoreCtx {
        user_id: owner_id,
        store_id,
        role: "owner".to_string(),
    };
    with_store_ctx(ctx, async {
        let r = repo(&p);
        let ts = uniq();
        let user = any_user_id(&p).await;

    // Створюємо товар зі stock 10.000.
    let prod = r
        .create_product(&ProductCreateInput {
            barcode: Some(format!("{ts}inv")),
            sku: None,
            title: format!("ТЕСТ-ІНВ-{ts}"),
            description: None,
            price: Some("100.00".into()),
            cost_price: Some("50.00".into()),
            markup: None,
            stock: Some("10.000".into()),
            recommended_qty: None,
            uktzed: None,
            scan_excise: false,
            tax_rate: Some("20.00".into()),
            tax_group: Some("А".into()),
            is_weight: false,
            unit: Some("шт".into()),
            category_id: None,
            supplier_id: None,
        })
        .await
        .expect("create product");

    // Інвентаризація: фактично 12.5, обліково 10 → difference +2.5.
    let inv = r
        .create_inventory(&InventoryCreateInput {
            number: None,
            location: Some("ТЕСТ-СКЛАД".into()),
            inventory_date: chrono::Utc::now().naive_utc(),
            notes: Some("тест".into()),
            items: vec![InventoryItemInput {
                product_id: prod.id,
                actual_quantity: "12.5".into(),
                accounting_quantity: "10".into(),
                difference: "2.5".into(),
                cost_price: "50.00".into(),
                price: "100.00".into(),
            }],
            created_by: user,
        })
        .await
        .expect("create inventory");
    assert_eq!(inv.status, "draft");
    // Відповідь POST: вхідна scale.
    assert_eq!(inv.items[0].actual_quantity, "12.5");
    assert_eq!(inv.items[0].difference, "2.5");
    assert_eq!(
        inv.items[0].product.as_ref().unwrap().title,
        format!("ТЕСТ-ІНВ-{ts}")
    );
    // items[].total_cost — число 0 (як Python).
    assert_eq!(inv.items[0].total_cost, 0);
    // summary: 12.5*50 = 625.000 (scale 3).
    assert_eq!(inv.summary.total_cost, "625.000");

    // Confirm → stock 10 + 2.5 = 12.500.
    let confirmed = r.confirm_inventory(inv.id).await.expect("confirm");
    assert_eq!(confirmed.status, "confirmed");
    let stock: String = sqlx::query_scalar(
        "SELECT quantity::text FROM stock WHERE product_id = $1 AND store_id = $2",
    )
    .bind(prod.id)
    .bind(store_id)
    .fetch_one(&p)
    .await
    .expect("stock");
    assert_eq!(stock, "12.500");

    // Повторний confirm → 400 (статус вже confirmed).
    let err = r.confirm_inventory(inv.id).await.unwrap_err();
    assert!(
        matches!(&err, WriteError::BadRequest(msg) if msg.contains("вже має статус 'confirmed'")),
        "очікувався BadRequest, отримано: {err:?}"
    );

    // Cancel → відкат 12.5 - 2.5 = 10.000.
    let cancelled = r.cancel_inventory(inv.id).await.expect("cancel");
    assert_eq!(cancelled.status, "cancelled");
    let stock: String = sqlx::query_scalar(
        "SELECT quantity::text FROM stock WHERE product_id = $1 AND store_id = $2",
    )
    .bind(prod.id)
    .bind(store_id)
    .fetch_one(&p)
    .await
    .expect("stock");
    assert_eq!(stock, "10.000");

    // Недостатньо товару: інвентаризація з difference -100 → 400.
    let bad_inv = r
        .create_inventory(&InventoryCreateInput {
            number: None,
            location: Some("ТЕСТ".into()),
            inventory_date: chrono::Utc::now().naive_utc(),
            notes: None,
            items: vec![InventoryItemInput {
                product_id: prod.id,
                actual_quantity: "0".into(),
                accounting_quantity: "10".into(),
                difference: "-100".into(),
                cost_price: "50.00".into(),
                price: "100.00".into(),
            }],
            created_by: user,
        })
        .await
        .expect("create bad inventory");
    let err = r.confirm_inventory(bad_inv.id).await.unwrap_err();
    assert!(
        matches!(&err, WriteError::BadRequest(msg) if msg.contains("Недостатньо товару")),
        "очікувався BadRequest (недостатньо), отримано: {err:?}"
    );

    // Cleanup.
    let _ = sqlx::query("DELETE FROM inventories WHERE id = ANY($1)")
        .bind(vec![inv.id, bad_inv.id])
        .execute(&p)
        .await;
    cleanup_product(&p, prod.id).await;
    })
    .await;
}

#[tokio::test]
async fn concurrent_inventory_confirms_no_data_loss() {
    let p = pool().await;
    let r = repo(&p);
    let ts = uniq();
    let user = any_user_id(&p).await;
    let store_id = Uuid::parse_str(STORE_ID).unwrap();
    let owner_id = Uuid::parse_str(OWNER_ID).unwrap();
    let ctx = StoreCtx {
        user_id: owner_id,
        store_id,
        role: "owner".to_string(),
    };
    with_store_ctx(ctx, async {
        let r = repo(&p);
        let ts = uniq();
        let user = any_user_id(&p).await;

        // Товар зі stock 100.000.
        let prod = r
            .create_product(&ProductCreateInput {
                barcode: Some(format!("{ts}conc")),
                sku: None,
                title: format!("ТЕСТ-КОНКУР-{ts}"),
                description: None,
                price: Some("10.00".into()),
                cost_price: Some("5.00".into()),
                markup: None,
                stock: Some("100.000".into()),
                recommended_qty: None,
                uktzed: None,
                scan_excise: false,
                tax_rate: Some("20.00".into()),
                tax_group: Some("А".into()),
                is_weight: false,
                unit: Some("шт".into()),
                category_id: None,
                supplier_id: None,
            })
            .await
            .expect("create product");

        // Дві інвентаризації на ОДИН товар: +7 і -3.
        let mk_inv = |diff: &str| InventoryCreateInput {
            number: None,
            location: Some("КОНКУРЕНЦІЯ".into()),
            inventory_date: chrono::Utc::now().naive_utc(),
            notes: None,
            items: vec![InventoryItemInput {
                product_id: prod.id,
                actual_quantity: "0".into(),
                accounting_quantity: "0".into(),
                difference: diff.into(),
                cost_price: "5.00".into(),
                price: "10.00".into(),
            }],
            created_by: user,
        };
        let inv_a = r.create_inventory(&mk_inv("7")).await.expect("inv A");
        let inv_b = r.create_inventory(&mk_inv("-3")).await.expect("inv B");

        // Паралельне проведення обох (два "термінали").
        let r2 = repo(&p);
        let (res_a, res_b) = tokio::join!(
            r.confirm_inventory(inv_a.id),
            r2.confirm_inventory(inv_b.id),
        );
        res_a.expect("confirm A");
        res_b.expect("confirm B");

        // Кінцевий залишок: 100 + 7 - 3 = 104.000 — нуль втрат.
        let stock: String = sqlx::query_scalar(
            "SELECT quantity::text FROM stock WHERE product_id = $1 AND store_id = $2",
        )
        .bind(prod.id)
        .bind(store_id)
        .fetch_one(&p)
        .await
        .expect("stock");
        assert_eq!(stock, "104.000", "паралельні проведення втратили дані");

        // Cleanup.
        let _ = sqlx::query("DELETE FROM inventories WHERE id = ANY($1)")
            .bind(vec![inv_a.id, inv_b.id])
            .execute(&p)
            .await;
        cleanup_product(&p, prod.id).await;
    })
    .await;
}
