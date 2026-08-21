//! Етап 5 — store_id в офлайн-схемі (SQLite) та черзі синхронізації.
//!
//! Покриває:
//!   1. Ідемпотентна міграція: існуюча БД (СТАРА схема без store_id)
//!      відкривається OfflineDatabase::new() без помилок, колонки додаються,
//!      повторний запуск безпечний (PRAGMA table_info → ALTER не повторюється).
//!   2. store_id зберігається в черзі чеків і повертається get_unsynced_receipts.
//!   3. store_id витягується з JSON-пейлоада чека (якщо не передано явно).
//!   4. Кеш товарів фільтрується поточним store_id; legacy-рядки (NULL)
//!      лишаються видимими (зворотна сумісність).

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Глобальний м'ютекс: усі тести серіалізують доступ до XDG_DATA_HOME
/// (env-змінна — глобальна для процесу, паралельні тести б конкурували).
fn xdg_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Ізольована XDG_DATA_HOME — реальні дані користувача не чіпаються.
fn temp_data_home(tag: &str) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "torgashka-offline-store-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).expect("тимчасова XDG_DATA_HOME");
    tmp
}

/// Створити SQLite-файл зі СТАРОЮ схемою (до Етапу 5 — без store_id).
fn create_legacy_db(db_file: &PathBuf) {
    if let Some(parent) = db_file.parent() {
        std::fs::create_dir_all(parent).expect("директорія legacy БД створена");
    }
    let conn = Connection::open(db_file).expect("legacy SQLite створено");
    conn.execute_batch(
        "
        CREATE TABLE products (
            id TEXT PRIMARY KEY,
            data TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE receipts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            data TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            synced INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_receipts_synced ON receipts(synced);
        INSERT INTO products (id, data) VALUES ('legacy-1', '{\"id\":\"legacy-1\"}');
        INSERT INTO receipts (data, synced) VALUES ('{\"receipt_type\":\"sale\",\"total_amount\":\"1.00\"}', 0);
        ",
    )
    .expect("legacy schema створена");
}

/// Чи має таблиця колонку (PRAGMA table_info).
fn has_column(db_file: &PathBuf, table: &str, column: &str) -> bool {
    let conn = Connection::open(db_file).expect("БД відкрита для перевірки");
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("PRAGMA підготовлено");
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("PRAGMA виконано")
        .collect::<Result<_, _>>()
        .expect("колонки зібрано");
    cols.iter().any(|c| c == column)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Ідемпотентна міграція старої БД
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn legacy_db_upgrades_idempotently() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("migration");
    std::env::set_var("XDG_DATA_HOME", &tmp);
    let db_file = tmp.join("torgashka").join("offline.db");
    create_legacy_db(&db_file);

    // Перший відкритий: міграція додає store_id.
    let db = torgashka_infrastructure::offline::db::OfflineDatabase::new()
        .expect("legacy БД відкрилась без помилок (критерій 3)");
    assert!(has_column(&db_file, "products", "store_id"), "products.store_id додано");
    assert!(has_column(&db_file, "receipts", "store_id"), "receipts.store_id додано");
    drop(db);

    // Повторний запуск — ідемпотентність (не падає, колонки на місці).
    let db2 = torgashka_infrastructure::offline::db::OfflineDatabase::new()
        .expect("повторне відкриття безпечне (ідемпотентність)");
    assert!(has_column(&db_file, "products", "store_id"));
    assert!(has_column(&db_file, "receipts", "store_id"));
    drop(db2);

    // Legacy-дані збереглись; legacy-чек у черзі лишився.
    let db3 = torgashka_infrastructure::offline::db::OfflineDatabase::new().unwrap();
    assert_eq!(
        db3.count_unsynced_receipts().expect("count"),
        1,
        "legacy-чек пережив міграцію"
    );
    let unsynced = db3.get_unsynced_receipts().expect("get_unsynced");
    assert_eq!(
        unsynced[0]["store_id"], serde_json::Value::Null,
        "legacy-чек без store_id → NULL у черзі"
    );

    eprintln!("[offline_store_id] ✅ міграція: legacy БД → store_id додано ідемпотентно, дані збережено");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. store_id зберігається в черзі та повертається при синхронізації
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn receipt_queue_preserves_store_id() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("queue");
    std::env::set_var("XDG_DATA_HOME", &tmp);

    let db = torgashka_infrastructure::offline::db::OfflineDatabase::new()
        .expect("offline.db створена");
    let store_a = "d9be9608-c011-49be-b776-3317ca5e9af6";
    let store_b = "22222222-2222-4222-8222-222222222222";

    let receipt = serde_json::json!({
        "receipt_type": "sale",
        "items": [{"product_id": "t-1", "quantity": 1, "price": "50.00"}],
        "total_amount": "50.00",
        "payment_method": "cash",
    });

    // Два чеки — дві різні точки.
    let id_a = db
        .save_receipt_offline_for_store(&receipt.to_string(), Some(store_a))
        .expect("чек A збережено");
    let id_b = db
        .save_receipt_offline_for_store(&receipt.to_string(), Some(store_b))
        .expect("чек B збережено");

    // Черга повертає store_id кожного чека — синхронізація знає, куди слати.
    let unsynced = db.get_unsynced_receipts().expect("get_unsynced");
    assert_eq!(unsynced.len(), 2);
    let a = unsynced.iter().find(|r| r["id"] == serde_json::json!(id_a)).expect("чек A у черзі");
    let b = unsynced.iter().find(|r| r["id"] == serde_json::json!(id_b)).expect("чек B у черзі");
    assert_eq!(a["store_id"].as_str(), Some(store_a), "store_id A збережено в черзі");
    assert_eq!(b["store_id"].as_str(), Some(store_b), "store_id B збережено в черзі");

    // Після mark_synced черга порожня.
    db.mark_receipt_synced(id_a).expect("mark A");
    db.mark_receipt_synced(id_b).expect("mark B");
    assert_eq!(db.count_unsynced_receipts().expect("count"), 0);

    eprintln!("[offline_store_id] ✅ черга: store_id збережено для 2 чеків, синхронізовано");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. store_id витягується з JSON-пейлоада чека
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn receipt_store_id_extracted_from_json_payload() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("json-extract");
    std::env::set_var("XDG_DATA_HOME", &tmp);

    let db = torgashka_infrastructure::offline::db::OfflineDatabase::new()
        .expect("offline.db створена");
    let store = "33333333-3333-4333-8333-333333333333";

    // Пейлоад містить store_id — save_receipt (без явного аргументу) має його підхопити.
    let receipt = serde_json::json!({
        "receipt_type": "return",
        "store_id": store,
        "total_amount": "10.00",
    });
    let id = db
        .save_receipt_offline(&receipt.to_string())
        .expect("чек збережено");

    let unsynced = db.get_unsynced_receipts().expect("get_unsynced");
    let r = unsynced.iter().find(|r| r["id"] == serde_json::json!(id)).expect("чек у черзі");
    assert_eq!(
        r["store_id"].as_str(),
        Some(store),
        "store_id витягнуто з JSON-пейлоада"
    );

    eprintln!("[offline_store_id] ✅ JSON: store_id витягнуто з пейлоада чека");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Кеш товарів фільтрується поточним store_id
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn products_cache_scoped_by_store() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("cache-scope");
    std::env::set_var("XDG_DATA_HOME", &tmp);

    let db = torgashka_infrastructure::offline::db::OfflineDatabase::new()
        .expect("offline.db створена");
    let store_a = "d9be9608-c011-49be-b776-3317ca5e9af6";
    let store_b = "44444444-4444-4444-8444-444444444444";

    let products_a = serde_json::json!([
        {"id": "p-a1", "title": "Товар точки A"},
        {"id": "p-a2", "title": "Ще A"},
    ]);
    let products_b = serde_json::json!([
        {"id": "p-b1", "title": "Товар точки B"},
    ]);

    db.cache_products_for_store(&products_a.to_string(), Some(store_a))
        .expect("кеш A");
    db.cache_products_for_store(&products_b.to_string(), Some(store_b))
        .expect("кеш B");

    // Пошук у точці A бачить тільки товари A.
    let cached_a: serde_json::Value = serde_json::from_str(
        &db.get_cached_products_for_store(None, 100, Some(store_a))
            .expect("читання кешу A"),
    )
    .expect("JSON A");
    assert_eq!(cached_a.as_array().unwrap().len(), 2, "точка A: 2 товари");

    // Пошук у точці B бачить тільки товари B.
    let cached_b: serde_json::Value = serde_json::from_str(
        &db.get_cached_products_for_store(None, 100, Some(store_b))
            .expect("читання кешу B"),
    )
    .expect("JSON B");
    assert_eq!(cached_b.as_array().unwrap().len(), 1, "точка B: 1 товар");

    // Пошук за назвою у точці A.
    let search_a: serde_json::Value = serde_json::from_str(
        &db.get_cached_products_for_store(Some("Товар точки A"), 100, Some(store_a))
            .expect("пошук A"),
    )
    .expect("JSON search A");
    assert_eq!(search_a.as_array().unwrap().len(), 1, "пошук у A знайшов 1");

    // Без фільтра (legacy-виклик) — видно все.
    let all: serde_json::Value = serde_json::from_str(
        &db.get_cached_products(None, 100).expect("кеш без фільтра"),
    )
    .expect("JSON all");
    assert_eq!(all.as_array().unwrap().len(), 3, "без фільтра: всі 3 товари");

    eprintln!("[offline_store_id] ✅ кеш: товари відфільтровані за store_id (A=2, B=1)");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Legacy-рядки кешу (store_id IS NULL) видимі у будь-якій точці
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn legacy_products_visible_after_upgrade() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("legacy-visible");
    std::env::set_var("XDG_DATA_HOME", &tmp);
    let db_file = tmp.join("torgashka").join("offline.db");
    create_legacy_db(&db_file);

    // Відкриваємо через міграцію — legacy-товар лишився.
    let db = torgashka_infrastructure::offline::db::OfflineDatabase::new()
        .expect("БД відкрита");
    let store = "55555555-5555-4555-8555-555555555555";
    let cached: serde_json::Value = serde_json::from_str(
        &db.get_cached_products_for_store(None, 100, Some(store))
            .expect("читання кешу"),
    )
    .expect("JSON");
    assert_eq!(
        cached.as_array().unwrap().len(),
        1,
        "legacy-товар (store_id IS NULL) видимий у точці"
    );

    eprintln!("[offline_store_id] ✅ legacy: товари без store_id видимі після апгрейду");
}
