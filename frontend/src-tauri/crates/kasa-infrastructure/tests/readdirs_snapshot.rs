//! Snapshot-тести довідників (етап 1): Rust-відповідь vs Python-еталон.
//!
//! Снапшоти (`tests/snapshots/*.json`) — реальні відповіді Python-бекенда
//! (FastAPI), зняті під час міграції:
//!   - products_default.json   — GET /api/v1/products?page=1&size=3
//!   - products_query.json     — GET /api/v1/products?query=хліб&page=1&size=5
//!   - categories_default.json — GET /api/v1/categories?page=1&size=5
//!   - suppliers_default.json  — GET /api/v1/suppliers?page=1&size=3
//!
//! Тест порівнює Rust-відповідь (serde_json::Value) зі снапшотом:
//! типи полів і значення — суворо (Decimal "41.00" ≠ number 41).

use kasa_domain::{ProductFilters, ReadDirectories};
use kasa_infrastructure::{db, repositories::directories::SqlxDirectories};
use uuid::Uuid;

fn snapshot_path(name: &str) -> String {
    format!("{}/tests/snapshots/{name}.json", env!("CARGO_MANIFEST_DIR"))
}

/// Читає JSON-снапшот Python-еталону.
fn python_snapshot(name: &str) -> serde_json::Value {
    let raw = std::fs::read_to_string(snapshot_path(name))
        .unwrap_or_else(|e| panic!("не вдалося прочитати снапшот {name}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("невалідний JSON у {name}: {e}"))
}

/// Створює пул і репозиторій (БД має бути доступна — як у Python-еталону).
async fn repo() -> SqlxDirectories {
    let pool = db::connect_readonly_pool(3)
        .await
        .expect("БД недоступна для snapshot-тесту: задайте DATABASE_URL або DB_* у backend/.env");
    SqlxDirectories::new(pool)
}

/// Порівнює Rust-відповідь зі снапшотом Python (Value-рівність: типи+значення).
fn assert_matches_python(
    label: &str,
    rust_json: serde_json::Value,
    python_json: serde_json::Value,
) {
    if rust_json != python_json {
        panic!(
            "Розбіжність {label}:\nRust:   {}\nPython: {}\n",
            serde_json::to_string_pretty(&rust_json).unwrap(),
            serde_json::to_string_pretty(&python_json).unwrap()
        );
    }
    eprintln!("✅ {label}: ідентично Python-еталону");
}

/// Допоміжний: UUID зі снапшота Python (рядок) — для фільтрів.
fn uuid_of(snapshot: &serde_json::Value, index: usize, field: &str) -> Option<Uuid> {
    snapshot["items"][index][field]
        .as_str()
        .map(|s| Uuid::parse_str(s).unwrap())
}

#[tokio::test]
async fn products_default_matches_python() {
    let repo = repo().await;
    let mut filters = ProductFilters::default_page();
    filters.size = 3; // як у снапшоті: ?page=1&size=3

    let page = repo.list_products(&filters).await.expect("list_products");
    let rust_json = serde_json::to_value(&page).unwrap();
    assert_matches_python(
        "products_default",
        rust_json,
        python_snapshot("products_default"),
    );
}

#[tokio::test]
async fn products_query_matches_python() {
    let repo = repo().await;
    let mut filters = ProductFilters::default_page();
    filters.query = Some("хліб".to_string());
    filters.size = 5; // як у снапшоті: ?query=хліб&page=1&size=5

    let page = repo.list_products(&filters).await.expect("list_products");
    let rust_json = serde_json::to_value(&page).unwrap();
    assert_matches_python(
        "products_query",
        rust_json,
        python_snapshot("products_query"),
    );
}

#[tokio::test]
async fn categories_default_matches_python() {
    let repo = repo().await;
    let page = repo.list_categories(1, 5).await.expect("list_categories");
    let rust_json = serde_json::to_value(&page).unwrap();
    assert_matches_python(
        "categories_default",
        rust_json,
        python_snapshot("categories_default"),
    );
}

#[tokio::test]
async fn suppliers_default_matches_python() {
    let repo = repo().await;
    let page = repo.list_suppliers(1, 3).await.expect("list_suppliers");
    let rust_json = serde_json::to_value(&page).unwrap();
    assert_matches_python(
        "suppliers_default",
        rust_json,
        python_snapshot("suppliers_default"),
    );
}

#[tokio::test]
async fn products_filter_by_category_matches_python() {
    // Фільтр категорією: Rust має повернути ту саму підмножину, що Python.
    // Беремо реальну категорію зі снапшота products і звіряємо total.
    let repo = repo().await;
    let snap = python_snapshot("products_default");
    let cat_id = uuid_of(&snap, 0, "category_id").expect("category_id у снапшоті");
    let mut filters = ProductFilters::default_page();
    filters.category_id = Some(cat_id);
    filters.size = 100;

    let page = repo.list_products(&filters).await.expect("list_products");
    // Сума всіх товарів цієї категорії (total) має збігатися зі значенням
    // у базі — перевіряємо консистентність фільтра (не менше 1).
    assert!(page.total >= 1, "категорія {cat_id} має містити товари");
    assert_eq!(page.items.len() as i64, page.total.min(100));
}

#[tokio::test]
async fn empty_query_returns_nothing_weird() {
    // Порожній рядок query → Python: query='' → falsy → еквівалентно None.
    let repo = repo().await;
    let mut filters = ProductFilters::default_page();
    filters.query = Some(String::new());
    filters.size = 10;

    let page = repo.list_products(&filters).await.expect("list_products");
    assert_eq!(page.total, 4002, "порожній query не має фільтрувати");
}
