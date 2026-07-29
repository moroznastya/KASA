// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Офлайн-база даних (SQLite)
// ─────────────────────────────────────────────────────────────────────────────
// Використовується для:
//   - Кешування товарів (офлайн-довідник)
//   - Збереження чеків при відсутності інтернету
//   - Локальних налаштувань
//   - Синхронізації при поновленні з'єднання
// ─────────────────────────────────────────────────────────────────────────────

use rusqlite::{Connection, params};
use std::path::PathBuf;

/// Офлайн-база даних
pub struct OfflineDatabase {
    conn: Connection,
}

impl OfflineDatabase {
    /// Створити/відкрити SQLite базу даних
    pub fn new() -> Result<Self, String> {
        let db_path = Self::get_db_path()?;

        // Створюємо директорію, якщо не існує
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Не вдалося створити директорію БД: {}", e))?;
        }

        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Не вдалося відкрити БД: {}", e))?;

        let db = OfflineDatabase { conn };
        db.initialize_tables()?;

        Ok(db)
    }

    /// Шлях до файлу бази даних
    fn get_db_path() -> Result<PathBuf, String> {
        // Використовуємо стандартну директорію для даних застосунку
        let data_dir = dirs_next::data_dir()
            .ok_or_else(|| "Не вдалося визначити директорію даних".to_string())?;

        Ok(data_dir.join("kasa-pos").join("offline.db"))
    }

    /// Ініціалізація таблиць
    fn initialize_tables(&self) -> Result<(), String> {
        self.conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            -- Товари (офлайн-кеш)
            CREATE TABLE IF NOT EXISTS products (
                id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Чеки (офлайн)
            CREATE TABLE IF NOT EXISTS receipts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                synced INTEGER NOT NULL DEFAULT 0
            );

            -- Налаштування
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Індекси
            CREATE INDEX IF NOT EXISTS idx_receipts_synced ON receipts(synced);
            CREATE INDEX IF NOT EXISTS idx_products_name ON products(id);
        ")
        .map_err(|e| format!("Помилка ініціалізації таблиць: {}", e))?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Товари (кеш)
    // ─────────────────────────────────────────────────────────────────────────

    /// Кешувати товари (масив JSON)
    pub fn cache_products(&self, products_json: &str) -> Result<usize, String> {
        let products: Vec<serde_json::Value> = serde_json::from_str(products_json)
            .map_err(|e| format!("Помилка парсингу JSON: {}", e))?;

        let mut count = 0;
        for product in &products {
            let id = product["id"].as_str()
                .or_else(|| product["barcode"].as_str())
                .unwrap_or("unknown");

            let data = serde_json::to_string(product)
                .map_err(|e| format!("Помилка серіалізації: {}", e))?;

            self.conn.execute(
                "INSERT INTO products (id, data, updated_at) VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET data = ?2, updated_at = datetime('now')",
                params![id, data],
            ).map_err(|e| format!("Помилка вставки товару: {}", e))?;

            count += 1;
        }

        Ok(count)
    }

    /// Отримати кешовані товари
    pub fn get_cached_products(&self, query: Option<&str>) -> Result<String, String> {
        let products: Vec<serde_json::Value> = if let Some(q) = query {
            let pattern = format!("%{}%", q);
            let mut stmt = self.conn.prepare(
                "SELECT data FROM products WHERE data LIKE ?1 ORDER BY updated_at DESC LIMIT 100"
            ).map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

            let rows = stmt.query_map(params![pattern], |row| {
                let data: String = row.get(0)?;
                Ok(data)
            }).map_err(|e| format!("Помилка виконання запиту: {}", e))?;

            let mut result = Vec::new();
            for row in rows {
                if let Ok(data) = row {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                        result.push(val);
                    }
                }
            }
            result
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT data FROM products ORDER BY updated_at DESC LIMIT 1000"
            ).map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

            let rows = stmt.query_map([], |row| {
                let data: String = row.get(0)?;
                Ok(data)
            }).map_err(|e| format!("Помилка виконання запиту: {}", e))?;

            let mut result = Vec::new();
            for row in rows {
                if let Ok(data) = row {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                        result.push(val);
                    }
                }
            }
            result
        };

        serde_json::to_string(&products)
            .map_err(|e| format!("Помилка серіалізації: {}", e))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Чеки (офлайн)
    // ─────────────────────────────────────────────────────────────────────────

    /// Зберегти чек локально
    pub fn save_receipt(&self, receipt_json: &str) -> Result<i64, String> {
        self.conn.execute(
            "INSERT INTO receipts (data, synced) VALUES (?1, 0)",
            params![receipt_json],
        ).map_err(|e| format!("Помилка збереження чека: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Отримати несинхронізовані чеки
    pub fn get_unsynced_receipts(&self) -> Result<String, String> {
        let mut stmt = self.conn.prepare(
            "SELECT id, data FROM receipts WHERE synced = 0 ORDER BY created_at ASC"
        ).map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let data: String = row.get(1)?;
            Ok(serde_json::json!({"id": id, "data": data}))
        }).map_err(|e| format!("Помилка виконання запиту: {}", e))?;

        let mut receipts = Vec::new();
        for row in rows {
            if let Ok(receipt) = row {
                receipts.push(receipt);
            }
        }

        serde_json::to_string(&receipts)
            .map_err(|e| format!("Помилка серіалізації: {}", e))
    }

    /// Позначити чек як синхронізований
    pub fn mark_synced(&self, receipt_id: i64) -> Result<(), String> {
        self.conn.execute(
            "UPDATE receipts SET synced = 1 WHERE id = ?1",
            params![receipt_id],
        ).map_err(|e| format!("Помилка оновлення: {}", e))?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Налаштування
    // ─────────────────────────────────────────────────────────────────────────

    /// Зберегти налаштування
    pub fn save_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
            params![key, value],
        ).map_err(|e| format!("Помилка збереження налаштування: {}", e))?;

        Ok(())
    }

    /// Отримати налаштування
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT value FROM settings WHERE key = ?1"
        ).map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

        let mut rows = stmt.query_map(params![key], |row| {
            let value: String = row.get(0)?;
            Ok(value)
        }).map_err(|e| format!("Помилка виконання запиту: {}", e))?;

        match rows.next() {
            Some(Ok(value)) => Ok(Some(value)),
            _ => Ok(None),
        }
    }
}
