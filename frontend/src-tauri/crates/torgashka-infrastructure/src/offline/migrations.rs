//! Версійовані міграції offline.db через `PRAGMA user_version`.
//!
//! Дизайн: `docs/design/sync-schema-design.md`, розділ 7.
//!
//! Механізм (7.3):
//!   1. Rust-код читає `PRAGMA user_version`.
//!   2. Для кожної міграції N > current: `BEGIN` → execute SQL файлу N →
//!      `COMMIT` → `PRAGMA user_version = N`.
//!   3. Кожна міграція — в ОКРЕМІЙ транзакції: при помилці rollback,
//!      версія не змінюється, дані не пошкоджуються.
//!
//! Існуючі БД (user_version = 0) проходять ті самі міграції з 0001 —
//! це єдиний шлях розвитку схеми (7.1).
//!
//! Відоме обмеження середовища: локальна SQLite-збірка (перевірено
//! 3.45.1/3.46.0) НЕ підтримує `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`
//! ("near EXISTS: syntax error"). Тому додавання колонок в ІСНУЮЧІ legacy-
//! таблиці виконується Rust-кроком у межах транзакції міграції:
//! `PRAGMA table_info` → `ALTER TABLE ... ADD COLUMN` (без IF NOT EXISTS).

use rusqlite::Connection;

/// Актуальна версія схеми offline.db.
pub const SCHEMA_VERSION: u32 = 4;

/// Опис однієї міграції.
pub struct Migration {
    /// Номер версії, яку встановлює міграція (PRAGMA user_version = N).
    pub version: u32,
    /// Коротка назва (для діагностики).
    pub name: &'static str,
    /// SQL-тіло міграції (без BEGIN/COMMIT/PRAGMA — транзакцією керує код).
    pub sql: &'static str,
}

/// Реєстр міграцій у порядку зростання версій.
///
/// Файли: `src/offline/migrations/offline/NNNN_назва.sql` (дизайн 7.1).
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline_legacy",
        sql: include_str!("migrations/offline/0001_baseline_legacy.sql"),
    },
    Migration {
        version: 2,
        name: "sync_meta",
        sql: include_str!("migrations/offline/0002_sync_meta.sql"),
    },
    Migration {
        version: 3,
        name: "master_tables",
        sql: include_str!("migrations/offline/0003_master_tables.sql"),
    },
    Migration {
        version: 4,
        name: "transaction_idempotency",
        sql: include_str!("migrations/offline/0004_transaction_idempotency.sql"),
    },
];

/// Поточна версія схеми (`PRAGMA user_version`).
pub fn current_version(conn: &Connection) -> Result<u32, String> {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|v| v as u32)
        .map_err(|e| format!("Не вдалося прочитати PRAGMA user_version: {}", e))
}

/// Таблиця має колонку?
fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("PRAGMA table_info({table}): {}", e))?;
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("читання PRAGMA table_info({table}): {}", e))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("збір колонок {table}: {}", e))?;
    Ok(cols.iter().any(|c| c == column))
}

/// Legacy-апгрейд міграції 0001: додати `store_id` в ІСНУЮЧІ таблиці
/// (БД, створені до Етапу 5, не мають цієї колонки; CREATE TABLE IF NOT
/// EXISTS її не додасть).
///
/// Виконується Rust-кроком, бо `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`
/// не підтримується локальною SQLite-збіркою (див. doc-коментар модуля).
/// Для нових БД (колонка вже в DDL) — no-op. Викликається в межах
/// транзакції міграції 0001.
fn legacy_add_store_id(conn: &Connection) -> Result<(), String> {
    for table in ["products", "receipts"] {
        // Таблиці ще немає (свіжа БД до виконання DDL) — пропустити.
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .map_err(|e| format!("sqlite_master({table}): {}", e))?;
        if exists == 0 {
            continue;
        }
        if has_column(conn, table, "store_id")? {
            continue; // вже є — ідемпотентність
        }
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN store_id TEXT"),
            [],
        )
        .map_err(|e| format!("legacy ALTER {table} ADD store_id: {}", e))?;
    }
    Ok(())
}

/// Застосувати всі міграції N > поточної версії послідовно.
///
/// Кожна міграція — в окремій транзакції:
/// `BEGIN` → execute SQL → (Rust legacy-крок, якщо є) → `COMMIT` →
/// `PRAGMA user_version = N`.
/// Повторний виклик на актуальній БД — no-op (ідемпотентність).
///
/// Повертає фінальну версію схеми.
pub fn migrate(conn: &Connection) -> Result<u32, String> {
    let mut current = current_version(conn)?;

    for m in MIGRATIONS {
        if m.version <= current {
            continue;
        }

        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("BEGIN (міграція {} v{}): {}", m.name, m.version, e))?;

        // Міграція 0001: legacy-БД без store_id (до Етапу 5) — Rust-крок
        // у межах тієї самої транзакції, ДО SQL (індекси 0001 посилаються
        // на store_id). SQLite не підтримує IF NOT EXISTS для ADD COLUMN
        // у цій збірці; для нових БД колонка вже в DDL → no-op.
        if m.version == 1 {
            legacy_add_store_id(&tx)
                .map_err(|e| format!("Legacy-крок {} v{}: {}", m.name, m.version, e))?;
        }

        tx.execute_batch(m.sql)
            .map_err(|e| format!("Міграція {} v{}: {}", m.name, m.version, e))?;

        tx.commit()
            .map_err(|e| format!("COMMIT (міграція {} v{}): {}", m.name, m.version, e))?;

        conn.execute_batch(&format!("PRAGMA user_version = {}", m.version)).map_err(|e| {
            format!(
                "PRAGMA user_version = {} (міграція {}): {}",
                m.version, m.name, e
            )
        })?;

        current = m.version;
    }

    Ok(current)
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Таблиця існує в БД?
    fn table_exists(conn: &Connection, table: &str) -> bool {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("sqlite_master запит");
        n > 0
    }

    /// Таблиця має колонку?
    fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
        super::has_column(conn, table, column).expect("has_column")
    }

    /// Індекс існує?
    fn index_exists(conn: &Connection, index: &str) -> bool {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                [index],
                |row| row.get(0),
            )
            .expect("sqlite_master index");
        n > 0
    }

    /// Свіжа БД (user_version=0) → мігрується до 0002:
    /// sync_meta/outbox створені, user_version = SCHEMA_VERSION.
    #[test]
    fn fresh_db_migrates_to_actual() {
        let conn = Connection::open_in_memory().expect("in-memory");
        assert_eq!(current_version(&conn).unwrap(), 0);

        let v = migrate(&conn).expect("міграція свіжої БД");
        assert_eq!(v, super::SCHEMA_VERSION);
        assert_eq!(current_version(&conn).unwrap(), super::SCHEMA_VERSION);

        assert!(table_exists(&conn, "products"));
        assert!(table_exists(&conn, "receipts"));
        assert!(table_exists(&conn, "settings"));
        assert!(table_exists(&conn, "sync_meta"));
        assert!(table_exists(&conn, "outbox"));

        // Схема outbox з розділу 4.1.
        for col in [
            "id",
            "type",
            "client_uuid",
            "payload",
            "status",
            "attempts",
            "next_attempt_at",
            "last_error",
            "created_at",
            "pushed_at",
        ] {
            assert!(has_column(&conn, "outbox", col), "outbox.{col}");
        }
        // sync_meta — розділ 1.2 / 8.1.
        assert!(has_column(&conn, "sync_meta", "entity"));
        assert!(has_column(&conn, "sync_meta", "version"));
        // Індекси.
        assert!(index_exists(&conn, "idx_outbox_status"));
        assert!(index_exists(&conn, "idx_outbox_created"));
        assert!(index_exists(&conn, "idx_receipts_synced"));
        assert!(index_exists(&conn, "idx_receipts_store"));
        assert!(index_exists(&conn, "idx_products_store"));
        // Свіжа БД: store_id вже в DDL, legacy-крок — no-op.
        assert!(has_column(&conn, "products", "store_id"));
        assert!(has_column(&conn, "receipts", "store_id"));

        // Міграція 0003 (master_tables): нормалізовані довідники на місці.
        for t in ["categories", "suppliers", "employees", "stock_norms", "products_v2"] {
            assert!(table_exists(&conn, t), "0003: таблиця {t}");
        }
        for col in ["is_deleted", "server_version"] {
            assert!(has_column(&conn, "products_v2", col), "products_v2.{col}");
            assert!(has_column(&conn, "categories", col), "categories.{col}");
        }
    }

    /// Стара БД (схема старого db.rs, user_version=0) → мігрується до 0002
    /// БЕЗ втрати даних; store_id додається legacy-кроком движка.
    #[test]
    fn legacy_schema_migrates_without_data_loss() {
        let conn = Connection::open_in_memory().expect("in-memory");
        // Точна схема старого db.rs (до Етапу 5 — без store_id, без user_version).
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
            INSERT INTO products (id, data) VALUES ('legacy-1', '{\"id\":\"legacy-1\"}');
            INSERT INTO receipts (data, synced) VALUES ('{\"type\":\"sale\"}', 0);
            INSERT INTO settings (key, value) VALUES ('shop_name', 'Тест');
            ",
        )
        .expect("legacy schema");

        let v = migrate(&conn).expect("legacy → актуальна");
        assert_eq!(v, super::SCHEMA_VERSION);

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
        assert_eq!(sv, "Тест");

        // store_id додано legacy-кроком движка.
        assert!(has_column(&conn, "products", "store_id"));
        assert!(has_column(&conn, "receipts", "store_id"));

        // Нові таблиці синку на місці.
        assert!(table_exists(&conn, "sync_meta"));
        assert!(table_exists(&conn, "outbox"));
    }

    /// Повторний запуск движка — ідемпотентний (no-op), версія не змінюється,
    /// дані цілі.
    #[test]
    fn rerun_is_idempotent() {
        let conn = Connection::open_in_memory().expect("in-memory");
        migrate(&conn).expect("перша міграція");
        assert_eq!(current_version(&conn).unwrap(), super::SCHEMA_VERSION);

        conn.execute(
            "INSERT INTO sync_meta (entity, version) VALUES ('products', 42)",
            [],
        )
        .expect("данi sync_meta");

        // Повторний запуск.
        let v = migrate(&conn).expect("повторна міграція");
        assert_eq!(v, super::SCHEMA_VERSION);
        assert_eq!(current_version(&conn).unwrap(), super::SCHEMA_VERSION);

        // Дані після міграції цілі.
        let ver: i64 = conn
            .query_row("SELECT version FROM sync_meta WHERE entity='products'", [], |row| row.get(0))
            .expect("sync_meta рядок");
        assert_eq!(ver, 42);
    }

    /// Міграція 0002 дозволяє вставку в outbox зі статусом pending
    /// і відхиляє невалідний статус (CHECK з розділу 4.1).
    #[test]
    fn outbox_accepts_valid_status_rejects_invalid() {
        let conn = Connection::open_in_memory().expect("in-memory");
        migrate(&conn).expect("міграція");

        conn.execute(
            "INSERT INTO outbox (type, client_uuid, payload) VALUES ('receipt', 'uuid-1', '{}')",
            [],
        )
        .expect("вставка pending");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))
            .expect("count");
        assert_eq!(n, 1);

        let invalid = conn.execute(
            "INSERT INTO outbox (type, client_uuid, payload, status) VALUES ('receipt', 'uuid-2', '{}', 'bogus')",
            [],
        );
        assert!(invalid.is_err(), "CHECK має відхилити невалідний статус");
    }

    /// Двигун: при помилці в міграції 0002 транзакція відкочується ЦІЛКОМ
    /// (жоден об'єкт 0002 не залишається), 0001 залишається застосованою
    /// (версія = 1), дані цілі. Повторний запуск знову падає на 0002 —
    /// БД не псується.
    #[test]
    fn failed_migration_rolls_back_atomically() {
        let conn = Connection::open_in_memory().expect("in-memory");
        // Дані користувача (до міграції).
        conn.execute_batch("CREATE TABLE t_user (id INTEGER PRIMARY KEY);")
            .expect("таблиця користувача");
        conn.execute("INSERT INTO t_user (id) VALUES (1)", [])
            .expect("дані користувача");

        // Штучно ламаємо 0002: outbox з несумісною структурою (id TEXT PK).
        // CREATE TABLE IF NOT EXISTS пропустить створення, але CREATE INDEX
        // на колонки status/next_attempt_at впаде всередині транзакції 0002.
        conn.execute_batch("CREATE TABLE outbox (id TEXT PRIMARY KEY);")
            .expect("конфліктна outbox");

        let err = migrate(&conn);
        assert!(err.is_err(), "0002 має впасти на несумісній outbox");

        // 0001 закомічена (user_version = 1), 0002 — повністю відкочена.
        assert_eq!(current_version(&conn).unwrap(), 1, "0001 застосована");
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sync_meta'",
                [],
                |row| row.get(0),
            )
            .expect("sqlite_master");
        assert_eq!(n, 0, "sync_meta відкочена разом з 0002 (атомарність)");
        // Дані користувача не пошкоджені.
        let uid: i64 = conn
            .query_row("SELECT id FROM t_user WHERE id=1", [], |row| row.get(0))
            .expect("дані користувача");
        assert_eq!(uid, 1);
    }
}
