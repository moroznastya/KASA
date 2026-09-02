//! ЕТАП 2 — міграційна база SQLite (PRAGMA user_version + двигун міграцій).
//!
//! Покриває критерії прийняття:
//!   1. Нова БД → мігрується до 0002: sync_meta/outbox існують, user_version = 2.
//!   2. Стара схема (створена старим db.rs, user_version = 0) → мігрується
//!      до 0002 БЕЗ втрати даних.
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
// 1. Нова БД → мігрується до 0002
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
    assert_eq!(v, 2, "нова БД мігрується до 0002");
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
    assert_eq!(v, 2, "legacy БД → 0002");

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
    assert_eq!(v, 2, "версія не змінилась");

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
        2,
        "двигун бачить актуальну версію"
    );
    assert_eq!(migrations::SCHEMA_VERSION, 2);
    drop(db);
}
