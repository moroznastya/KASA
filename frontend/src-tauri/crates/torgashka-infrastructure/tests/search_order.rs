//! Пошук товару на касі: **наявність понад усе**, далі релевантність, далі назва.
//!
//! Критерій прийняття (задача «оптимізувати пошук у вікні каси»):
//!   1. Товари, що Є в наявності в поточній точці, стоять ПЕРЕД відсутніми —
//!      навіть якщо в відсутнього кращий клас релевантності.
//!   2. У межах однієї групи наявності — порядок релевантності 1:1 як
//!      Python-еталон `_relevance_sort_key` (starts_with → « q» → містить →
//!      штрих-код/артикул → інше), далі алфавіт за назвою.
//!   3. Сортування й сторінка виконуються в SQL: `page`/`size` дають
//!      конкатенацію, ідентичну повному списку (пагінація не губить і не дублює).

use torgashka_domain::{ProductCreateInput, ProductFilters, ReadDirectories, WriteDirectories};
use torgashka_infrastructure::repositories::directories::SqlxDirectories;
use torgashka_infrastructure::repositories::write::SqlxWriteDirectories;
use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};
use uuid::Uuid;

async fn pool() -> sqlx::PgPool {
    torgashka_infrastructure::db::connect_test_pool(5)
        .await
        .expect("тестова БД недоступна: задайте DATABASE_URL або DB_* у backend/.env")
}

fn uniq() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}{}",
        chrono::Utc::now().timestamp_micros(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

/// Будь-яка наявна точка + користувач: тест не залежить від конкретних UUID
/// (БД розробника, CI та `*_test` мають різні набори даних).
async fn any_ctx(p: &sqlx::PgPool) -> StoreCtx {
    let store_id: Uuid = sqlx::query_scalar("SELECT id FROM stores ORDER BY created_at LIMIT 1")
        .fetch_one(p)
        .await
        .expect("у БД має бути хоча б одна точка");
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users ORDER BY created_at LIMIT 1")
        .fetch_one(p)
        .await
        .expect("у БД має бути хоча б один користувач");
    StoreCtx {
        user_id,
        store_id,
        role: "owner".to_string(),
    }
}

fn input(title: String, ts: &str) -> ProductCreateInput {
    ProductCreateInput {
        barcode: Some(format!("{ts}b")),
        sku: Some(format!("SKU-{ts}")),
        title,
        description: None,
        price: Some("10.00".into()),
        cost_price: Some("5.00".into()),
        markup: None,
        stock: Some("5".into()),
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

/// Прямий запис залишку в точці (тестовий шорткат повз бізнес-правила).
async fn set_qty(p: &sqlx::PgPool, store_id: Uuid, product_id: Uuid, qty: &str) {
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, price)
         VALUES ($1, $2, $3::numeric, 0)
         ON CONFLICT (store_id, product_id) DO UPDATE SET quantity = EXCLUDED.quantity",
    )
    .bind(store_id)
    .bind(product_id)
    .bind(qty)
    .execute(p)
    .await
    .expect("set stock quantity");
}

async fn cleanup(p: &sqlx::PgPool, id: Uuid) {
    let _ = sqlx::query("DELETE FROM stock WHERE product_id = $1")
        .bind(id)
        .execute(p)
        .await;
    let _ = sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(id)
        .execute(p)
        .await;
}

async fn search(
    read: &SqlxDirectories,
    ctx: StoreCtx,
    query: &str,
    page: i64,
    size: i64,
) -> Vec<String> {
    let filters = ProductFilters {
        query: Some(query.to_string()),
        page,
        size,
        ..ProductFilters::default()
    };
    with_store_ctx(ctx, async { read.list_products(&filters).await })
        .await
        .expect("list_products")
        .items
        .into_iter()
        .map(|p| p.title)
        .collect()
}

#[tokio::test]
async fn available_products_come_first_then_relevance() {
    let p = pool().await;
    let store_pool = StorePool::new(p.clone());
    let write = SqlxWriteDirectories::new(store_pool.clone());
    let read = SqlxDirectories::new(store_pool);
    let ctx = any_ctx(&p).await;
    let ts = uniq();
    // Унікальний токен запиту: за назвами дає класи 0 / 1 / 2.
    let q = format!("тст{ts}");

    // «B» — єдиний бренд-токен зі пробілом перед ним → class 1.
    let t_zero = format!("{q} цвях гострий"); // starts_with(q)      → 0
    let t_one = format!("цвях {q} гострий"); // містить " {q}"      → 1
    let t_two = format!("цвяхт{q} гострий"); // містить q           → 2

    let mut ids = Vec::new();
    for title in [&t_zero, &t_one, &t_two] {
        let c = with_store_ctx(ctx.clone(), async {
            write.create_product(&input(title.clone(), &uniq())).await
        })
        .await
        .expect("create product");
        ids.push(c.id);
    }

    // Усі троє в наявності → чиста релевантність: 0, 1, 2.
    let got = search(&read, ctx.clone(), &q, 1, 10).await;
    assert_eq!(
        got,
        vec![t_zero.clone(), t_one.clone(), t_two.clone()],
        "наявні: порядок мусить бути за релевантністю (0 → 1 → 2)"
    );

    // Обнуляємо залишок товару з НАЙКРАЩИМ класом (0) — він мусить поїхати в кінець.
    set_qty(&p, ctx.store_id, ids[0], "0").await;
    let got = search(&read, ctx.clone(), &q, 1, 10).await;
    assert_eq!(
        got,
        vec![t_one.clone(), t_two.clone(), t_zero.clone()],
        "відсутній товар мусить бути ПІСЛЯ наявних, попри кращий клас релевантності"
    );

    // Пагінація по 2: конкатенація сторінок == повний список.
    let page1 = search(&read, ctx.clone(), &q, 1, 2).await;
    let page2 = search(&read, ctx.clone(), &q, 2, 2).await;
    assert_eq!(page1, vec![t_one.clone(), t_two.clone()], "сторінка 1");
    assert_eq!(page2, vec![t_zero.clone()], "сторінка 2");
    let mut joined = page1;
    joined.extend(page2);
    assert_eq!(
        joined,
        search(&read, ctx.clone(), &q, 1, 10).await,
        "сторінки мусять склеюватись у повний порядок"
    );

    // page/size поза межами — порожньо, без помилок.
    let offset_page = search(&read, ctx.clone(), &q, 99, 2).await;
    assert!(
        offset_page.is_empty(),
        "offset за межами → порожня сторінка"
    );

    for id in ids {
        cleanup(&p, id).await;
    }
}

/// Каталог **без** пошукового тексту: сортування за назвою (як Python-еталон),
/// але сторінка береться в SQL. Регресія, яку ловить тест: запит без тексту
/// матеріалізував УВЕСЬ каталог (4409 товарів → 1.2 с на касі).
#[tokio::test]
async fn catalog_without_query_is_ordered_and_paginated_in_sql() {
    let p = pool().await;
    let store_pool = StorePool::new(p.clone());
    let write = SqlxWriteDirectories::new(store_pool.clone());
    let read = SqlxDirectories::new(store_pool);
    let ctx = any_ctx(&p).await;
    let ts = uniq();

    // Назви з гарантованим алфавітним порядком: абрикос < банан < яблуко.
    let names = [
        format!("яблуко-{ts}"),
        format!("абрикос-{ts}"),
        format!("банан-{ts}"),
    ];
    let mut ids = Vec::new();
    for name in &names {
        let c = with_store_ctx(ctx.clone(), async {
            write.create_product(&input(name.clone(), &uniq())).await
        })
        .await
        .expect("create product");
        ids.push(c.id);
    }

    // Сторінка каталогу без пошуку: усі назви монотонні (code points).
    let page1 = search(&read, ctx.clone(), "", 1, 50).await;
    let lowered: Vec<String> = page1.iter().map(|t| t.to_lowercase()).collect();
    let mut sorted = lowered.clone();
    sorted.sort();
    assert_eq!(lowered, sorted, "каталог без пошуку — алфавітний порядок");

    // Наші три товари присутні саме в алфавітному порядку.
    let mine: Vec<String> = page1
        .iter()
        .filter(|t| t.ends_with(&ts))
        .map(|t| t.to_lowercase())
        .collect();
    assert_eq!(
        mine,
        vec![
            names[1].to_lowercase(),
            names[2].to_lowercase(),
            names[0].to_lowercase()
        ],
        "абрикос → банан → яблуко"
    );

    // Сторінки не перетинаються: склеювання теж монотонне.
    let page2 = search(&read, ctx.clone(), "", 2, 50).await;
    let joined: Vec<String> = page1
        .iter()
        .chain(page2.iter())
        .map(|t| t.to_lowercase())
        .collect();
    let mut joined_sorted = joined.clone();
    joined_sorted.sort();
    assert_eq!(joined, joined_sorted, "склеєні сторінки монотонні");

    // Далека сторінка за межами каталогу → порожньо, без помилок.
    assert!(search(&read, ctx.clone(), "", 100_000, 50).await.is_empty());

    for id in ids {
        cleanup(&p, id).await;
    }
}
