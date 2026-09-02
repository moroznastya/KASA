//! ЕТАП 2 — міграційна база SQLite (PRAGMA user_version + двигун міграцій).
//!
//! Покриває критерії прийняття:
//!   1. Нова БД → мігрується до актуальної версії: sync_meta/outbox існують.
//!   2. Стара схема (створена старим db.rs, user_version = 0) → мігрується
//!      до актуальної версії БЕЗ втрати даних.
//!   3. Повторний запуск — ідемпотентний (не падає, версія не змінюється).
//!   4. Міграції йдуть ТІЛЬКИ через двигун user_version (SQL-файли),
//!      ensure_column-хак не використовується.

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use torgashka_infrastructure::offline::db::OfflineDatabase;
use torgashka_infrastructure::offline::migrations;

/// Глобальний м'ютекс: усі тести серіалізують доступ до XDG_DATA_HOME
/// (env-змінна — глобальна для процесу, паралельні тести б конкурували).
fn xdg_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Ізольована XDG_DATA_HOME — реальні дані користувача не чіпаються.
fn temp_data_home(tag: &str) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "torgashka-offline-migr-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).expect("тимчасова XDG_DATA_HOME");
    tmp
}

/// Створити SQLite-файл зі СТАРОЮ схемою (старий db.rs, до Етапу 5 —
/// без store_id, без sync_meta/outbox, user_version = 0) + дані.
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
        CREATE INDEX idx_receipts_synced ON receipts(synced);
        INSERT INTO products (id, data) VALUES ('legacy-1', '{\"id\":\"legacy-1\",\"name\":\"Тест\"}');
        INSERT INTO receipts (data, synced) VALUES ('{\"type\":\"sale\",\"total\":\"1.00\"}', 0);
        INSERT INTO settings (key, value) VALUES ('shop_name', 'Магазин №1');
        ",
    )
    .expect("legacy schema створена");
    // user_version лишається 0 — як у старих БД.
}

/// Таблиця існує?
fn table_exists(db_file: &PathBuf, table: &str) -> bool {
    let conn = Connection::open(db_file).expect("БД відкрита");
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .expect("sqlite_master");
    n > 0
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Нова БД → мігрується до актуальної версії
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn fresh_db_migrates_to_v2_with_sync_tables() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("fresh");
    std::env::set_var("XDG_DATA_HOME", &tmp);
    let db_file = tmp.join("torgashka").join("offline.db");

    let db = OfflineDatabase::new().expect("нова БД відкрита");
    drop(db);

    let conn = Connection::open(&db_file).expect("БД відкрита для перевірки");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(v, migrations::SCHEMA_VERSION as i64, "нова БД мігрується до актуальної версії");
    assert!(table_exists(&db_file, "sync_meta"), "sync_meta існує");
    assert!(table_exists(&db_file, "outbox"), "outbox існує");
    assert!(table_exists(&db_file, "products"), "products існує");
    assert!(table_exists(&db_file, "receipts"), "receipts існує");
    assert!(table_exists(&db_file, "settings"), "settings існує");

    // outbox готовий до використання (вставка pending — ок).
    conn.execute(
        "INSERT INTO outbox (type, client_uuid, payload) VALUES ('receipt', 'u-1', '{}')",
        [],
    )
    .expect("outbox приймає рядок");
    // sync_meta готовий.
    conn.execute(
        "INSERT INTO sync_meta (entity, version) VALUES ('products', 0)",
        [],
    )
    .expect("sync_meta приймає рядок");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Стара БД → мігрується без втрати даних
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn legacy_db_migrates_without_data_loss() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("legacy");
    std::env::set_var("XDG_DATA_HOME", &tmp);
    let db_file = tmp.join("torgashka").join("offline.db");
    create_legacy_db(&db_file);

    // Відкриття = міграція через двигун user_version.
    let db = OfflineDatabase::new().expect("legacy БД мігрується без помилок");
    drop(db);

    let conn = Connection::open(&db_file).expect("БД відкрита");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(v, migrations::SCHEMA_VERSION as i64, "legacy БД → актуальна версія");

    // Дані не втрачені.
    let pid: String = conn
        .query_row("SELECT id FROM products WHERE id='legacy-1'", [], |row| row.get(0))
        .expect("продукт збережено");
    assert_eq!(pid, "legacy-1");
    let rn: i64 = conn
        .query_row("SELECT COUNT(*) FROM receipts", [], |row| row.get(0))
        .expect("чеки");
    assert_eq!(rn, 1);
    let sv: String = conn
        .query_row("SELECT value FROM settings WHERE key='shop_name'", [], |row| row.get(0))
        .expect("налаштування");
    assert_eq!(sv, "Магазин №1");

    // Нові таблиці на місці.
    assert!(table_exists(&db_file, "sync_meta"));
    assert!(table_exists(&db_file, "outbox"));

    // Дані доступні через API OfflineDatabase (не лише сирий SQL).
    let count = db_count_products();
    assert!(count >= 1, "кеш товарів читається: {count}");
}

/// Через OfflineDatabase API (ізоляція XDG вже встановлена викликачем).
fn db_count_products() -> usize {
    OfflineDatabase::new()
        .expect("БД відкрита")
        .get_product_count()
        .expect("count продуктів")
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Повторний запуск — ідемпотентний
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn rerun_is_idempotent() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("rerun");
    std::env::set_var("XDG_DATA_HOME", &tmp);
    let db_file = tmp.join("torgashka").join("offline.db");

    // Перший запуск.
    let db1 = OfflineDatabase::new().expect("перший запуск");
    drop(db1);

    // Дані після міграції (симулюємо роботу каси).
    let conn = Connection::open(&db_file).expect("БД відкрита");
    conn.execute(
        "INSERT INTO sync_meta (entity, version) VALUES ('products', 7)",
        [],
    )
    .expect("дані sync_meta");

    // Другий запуск — ідемпотентний.
    let db2 = OfflineDatabase::new().expect("повторний запуск безпечний");
    drop(db2);

    let conn = Connection::open(&db_file).expect("БД відкрита");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(v, migrations::SCHEMA_VERSION as i64, "версія не змінилась");

    let ver: i64 = conn
        .query_row("SELECT version FROM sync_meta WHERE entity='products'", [], |row| row.get(0))
        .expect("sync_meta рядок");
    assert_eq!(ver, 7, "дані не затерті повторним запуском");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Двигун міграцій — публічний API (current_version / migrate)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn engine_reports_current_version() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("engine");
    std::env::set_var("XDG_DATA_HOME", &tmp);

    let db = OfflineDatabase::new().expect("БД відкрита");
    let conn = Connection::open(db.get_db_path()).expect("БД відкрита для перевірки");
    assert_eq!(
        migrations::current_version(&conn).unwrap(),
        migrations::SCHEMA_VERSION,
        "двигун бачить актуальну версію"
    );
    assert_eq!(migrations::SCHEMA_VERSION, 8, "двигун бачить актуальну версію (0008)");
    drop(db);
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. ЕТАП 6: свіжа БД → v7 зі stock і транзакційними таблицями (0005/0006)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn fresh_db_reaches_latest_with_stock_txn_and_sync_log() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("fresh-v7");
    std::env::set_var("XDG_DATA_HOME", &tmp);
    let db_file = tmp.join("torgashka").join("offline.db");

    OfflineDatabase::new().expect("БД відкрита");

    let conn = Connection::open(&db_file).expect("БД відкрита для перевірки");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(v, migrations::SCHEMA_VERSION as i64, "свіжа БД доходить до останньої версії");

    // 0005: локальний stock.
    assert!(table_exists(&db_file, "stock"), "stock існує (0005)");
    conn.execute(
        "INSERT INTO stock (store_id, product_id, quantity) VALUES ('s-1', 'p-1', 1000)",
        [],
    )
    .expect("stock приймає рядок");

    // 0006: транзакційні таблиці.
    for t in [
        "receipt_items",
        "purchase_orders",
        "inventories",
        "transfers",
        "write_offs",
        "debtors_ledger",
        "cash_ledger",
    ] {
        assert!(table_exists(&db_file, t), "{t} існує (0006)");
    }
    // 0008: sync_log (моніторинг ЕТАП 7) — таблиця, CHECK і індекс.
    assert!(table_exists(&db_file, "sync_log"), "sync_log існує (0008)");
    conn.execute(
        "INSERT INTO sync_log (kind, entity, detail, attempts) \
         VALUES ('push_ok', 'cu-x', NULL, 1)",
        [],
    )
    .expect("sync_log приймає валідну подію");
    let bad = conn.execute("INSERT INTO sync_log (kind) VALUES ('unknown_kind')", []);
    assert!(bad.is_err(), "CHECK: невідомий kind відхиляється");
    let idx: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_sync_log_kind_ts'",
            [],
            |row| row.get(0),
        )
        .expect("індекс");
    assert_eq!(idx, 1, "індекс (kind, ts) створено");

    // Агрегати приймають рядок з client_uuid (UNIQUE, nullable).
    conn.execute(
        "INSERT INTO purchase_orders (client_uuid, store_id, data) \
         VALUES ('cu-1', 's-1', '{}')",
        [],
    )
    .expect("purchase_orders приймає агрегат");
    // receipt_items: FK на receipts.client_uuid (0004) працює.
    conn.execute(
        "INSERT INTO receipts (data, store_id, synced, client_uuid) \
         VALUES ('{}', 's-1', 1, 'cu-receipt')",
        [],
    )
    .expect("receipt");
    conn.execute(
        "INSERT INTO receipt_items (receipt_client_uuid, product_id, quantity) \
         VALUES ('cu-receipt', 'p-1', 2000)",
        [],
    )
    .expect("receipt_items приймає позицію");
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. ЕТАП 6: legacy products (JSON-кеш 0001) → products_v2 без втрат
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn legacy_products_json_migrated_to_products_v2() {
    let _guard = xdg_lock().lock().unwrap();
    let tmp = temp_data_home("legacy-products");
    std::env::set_var("XDG_DATA_HOME", &tmp);
    let db_file = tmp.join("torgashka").join("offline.db");
    create_legacy_db(&db_file);

    // Додаємо legacy-кеш продуктів у різних форматах (readdirs/старий кеш).
    let conn = Connection::open(&db_file).expect("БД відкрита");
    conn.execute_batch(
        r#"
        INSERT INTO products (id, data) VALUES
          ('legacy-2', '{"id":"legacy-2","name":"Кава","barcode":"4820000000001","price":"250.00","category_id":"cat-1"}'),
          ('legacy-3', '{"id":"legacy-3","title":"Старий формат","price":"10.50"}');
        "#,
    )
    .expect("додаткові legacy-продукти");
    let before: i64 = conn
        .query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))
        .expect("count products до");
    drop(conn);

    // Міграція (відкриття OfflineDatabase).
    OfflineDatabase::new().expect("міграція legacy БД");

    let conn = Connection::open(&db_file).expect("БД відкрита після міграції");
    let after: i64 = conn
        .query_row("SELECT COUNT(*) FROM products_v2", [], |row| row.get(0))
        .expect("count products_v2 після");
    assert_eq!(before, after, "КРИТЕРІЙ: COUNT до == COUNT після (без втрат)");

    // Кожен legacy id мігрував з нормалізованими колонками.
    let name1: String = conn
        .query_row("SELECT name FROM products_v2 WHERE id = 'legacy-1'", [], |row| row.get(0))
        .expect("legacy-1");
    assert_eq!(name1, "Тест", "name з data");

    let (barcode, category, price): (Option<String>, Option<String>, Option<f64>) = conn
        .query_row(
            "SELECT barcode, category_id, price FROM products_v2 WHERE id = 'legacy-2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("legacy-2");
    assert_eq!(barcode.as_deref(), Some("4820000000001"));
    assert_eq!(category.as_deref(), Some("cat-1"));
    // price — NUMERIC: "250.00" збережено REAL 250.0 (як у pull-шляху).
    assert_eq!(price, Some(250.0), "price з data");

    let name3: String = conn
        .query_row("SELECT name FROM products_v2 WHERE id = 'legacy-3'", [], |row| row.get(0))
        .expect("legacy-3");
    assert_eq!(name3, "Старий формат", "title → name (старий формат)");

    drop(conn);
    // Повторна міграція — ідемпотентна (повторний запуск не дублює).
    OfflineDatabase::new().expect("повторна міграція");
    let conn = Connection::open(&db_file).expect("БД відкрита після повтору");
    let again: i64 = conn
        .query_row("SELECT COUNT(*) FROM products_v2", [], |row| row.get(0))
        .expect("count після повтору");
    assert_eq!(again, after, "повторний запуск не дублює рядки");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(v, migrations::SCHEMA_VERSION as i64, "БД на останній версії");
}

