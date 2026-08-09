// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Офлайн-база даних (SQLite)
// ─────────────────────────────────────────────────────────────────────────────
// Використовується для:
//   - Кешування товарів (офлайн-довідник)
//   - Збереження чеків при відсутності інтернету
//   - Локальних налаштувань
//   - Синхронізації при поновленні з'єднання
// ─────────────────────────────────────────────────────────────────────────────

use rusqlite::{params, Connection};
use std::path::PathBuf;

/// Офлайн-база даних
pub struct OfflineDatabase {
    conn: Connection,
}

impl OfflineDatabase {
    /// Створити/відкрити SQLite базу даних
    pub fn new() -> Result<Self, String> {
        let db_path = Self::get_db_path_inner()?;

        // Створюємо директорію, якщо не існує
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Не вдалося створити директорію БД: {}", e))?;
        }

        let conn =
            Connection::open(&db_path).map_err(|e| format!("Не вдалося відкрити БД: {}", e))?;

        let db = OfflineDatabase { conn };
        db.initialize_tables()?;

        Ok(db)
    }

    /// Отримати шлях до файлу БД (публічний метод)
    pub fn get_db_path(&self) -> String {
        Self::get_db_path_inner()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Шлях до файлу бази даних (внутрішній)
    fn get_db_path_inner() -> Result<PathBuf, String> {
        // Використовуємо стандартну директорію для даних застосунку
        let data_dir = dirs_next::data_dir()
            .ok_or_else(|| "Не вдалося визначити директорію даних".to_string())?;

        Ok(data_dir.join("torgashka").join("offline.db"))
    }

    /// Ініціалізація таблиць
    fn initialize_tables(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
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
        ",
            )
            .map_err(|e| format!("Помилка ініціалізації таблиць: {}", e))?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Товари (кеш)
    // ─────────────────────────────────────────────────────────────────────────

    /// Кешувати товари (масив JSON)
    ///
    /// Вся серія вставок виконується в ОДНІЙ транзакції (BEGIN/COMMIT):
    /// без транзакції кожна з 4001 вставок проходить окремий write-цикл SQLite,
    /// що блокує головний потік на секунди. При помилці — ROLLBACK.
    pub fn cache_products(&self, products_json: &str) -> Result<usize, String> {
        let products: Vec<serde_json::Value> = serde_json::from_str(products_json)
            .map_err(|e| format!("Помилка парсингу JSON: {}", e))?;

        // Починаємо транзакцію (ручний BEGIN, бо Connection::transaction вимагає &mut)
        self.conn
            .execute("BEGIN", params![])
            .map_err(|e| format!("Помилка початку транзакції: {}", e))?;

        let mut count = 0;
        let result = (|| {
            for product in &products {
                let id = product["id"]
                    .as_str()
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
        })();

        match result {
            Ok(c) => {
                // Фіксуємо транзакцію — всі вставки однією операцією
                self.conn
                    .execute("COMMIT", params![])
                    .map_err(|e| format!("Помилка COMMIT: {}", e))?;
                Ok(c)
            }
            Err(e) => {
                // Відкочуємо транзакцію при будь-якій помилці вставки
                let _ = self.conn.execute("ROLLBACK", params![]);
                Err(e)
            }
        }
    }

    /// Отримати кешовані товари
    pub fn get_cached_products(&self, query: Option<&str>, limit: usize) -> Result<String, String> {
        let products: Vec<serde_json::Value> = if let Some(q) = query {
            let pattern = format!("%{}%", q);
            let mut stmt = self.conn.prepare(
                "SELECT data FROM products WHERE data LIKE ?1 ORDER BY updated_at DESC LIMIT ?2"
            ).map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

            let rows = stmt
                .query_map(params![pattern, limit as i64], |row| {
                    let data: String = row.get(0)?;
                    Ok(data)
                })
                .map_err(|e| format!("Помилка виконання запиту: {}", e))?;

            let mut result = Vec::new();
            for row in rows {
                let Ok(data) = row else { continue };
                let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) else {
                    continue;
                };
                result.push(val);
            }
            result
        } else {
            let mut stmt = self
                .conn
                .prepare("SELECT data FROM products ORDER BY updated_at DESC LIMIT ?1")
                .map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    let data: String = row.get(0)?;
                    Ok(data)
                })
                .map_err(|e| format!("Помилка виконання запиту: {}", e))?;

            let mut result = Vec::new();
            for row in rows {
                let Ok(data) = row else { continue };
                let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) else {
                    continue;
                };
                result.push(val);
            }
            result
        };

        serde_json::to_string(&products).map_err(|e| format!("Помилка серіалізації: {}", e))
    }

    /// Очистити кеш товарів
    pub fn clear_product_cache(&self) -> Result<usize, String> {
        let count = self
            .conn
            .execute("DELETE FROM products", [])
            .map_err(|e| format!("Помилка очищення кешу товарів: {}", e))?;
        Ok(count)
    }

    /// Отримати кількість кешованих товарів
    pub fn get_product_count(&self) -> Result<usize, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM products")
            .map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

        let count: i64 = stmt
            .query_row([], |row| row.get(0))
            .map_err(|e| format!("Помилка виконання запиту: {}", e))?;

        Ok(count as usize)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Чеки (офлайн)
    // ─────────────────────────────────────────────────────────────────────────

    /// Зберегти чек локально
    pub fn save_receipt(&self, receipt_json: &str) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO receipts (data, synced) VALUES (?1, 0)",
                params![receipt_json],
            )
            .map_err(|e| format!("Помилка збереження чека: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Зберегти чек локально (синонім для зручності)
    pub fn save_receipt_offline(&self, receipt_json: &str) -> Result<i64, String> {
        self.save_receipt(receipt_json)
    }

    /// Отримати несинхронізовані чеки (JSON рядок)
    pub fn get_unsynced_receipts_json(&self) -> Result<String, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM receipts WHERE synced = 0 ORDER BY created_at ASC")
            .map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let data: String = row.get(1)?;
                Ok(serde_json::json!({"id": id, "data": data}))
            })
            .map_err(|e| format!("Помилка виконання запиту: {}", e))?;

        let mut receipts = Vec::new();
        for row in rows {
            let Ok(receipt) = row else { continue };
            receipts.push(receipt);
        }

        serde_json::to_string(&receipts).map_err(|e| format!("Помилка серіалізації: {}", e))
    }

    /// Отримати несинхронізовані чеки (Vec<Value>)
    pub fn get_unsynced_receipts(&self) -> Result<Vec<serde_json::Value>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM receipts WHERE synced = 0 ORDER BY created_at ASC")
            .map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let data: String = row.get(1)?;
                Ok(serde_json::json!({"id": id, "data": data}))
            })
            .map_err(|e| format!("Помилка виконання запиту: {}", e))?;

        let mut receipts = Vec::new();
        for row in rows {
            let Ok(receipt) = row else { continue };
            receipts.push(receipt);
        }

        Ok(receipts)
    }

    /// Позначити чек як синхронізований
    pub fn mark_synced(&self, receipt_id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE receipts SET synced = 1 WHERE id = ?1",
                params![receipt_id],
            )
            .map_err(|e| format!("Помилка оновлення: {}", e))?;

        Ok(())
    }

    /// Позначити чек як синхронізований (синонім)
    pub fn mark_receipt_synced(&self, receipt_id: i64) -> Result<(), String> {
        self.mark_synced(receipt_id)
    }

    /// Кількість несинхронізованих чеків
    pub fn count_unsynced_receipts(&self) -> Result<usize, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM receipts WHERE synced = 0")
            .map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

        let count: i64 = stmt
            .query_row([], |row| row.get(0))
            .map_err(|e| format!("Помилка виконання запиту: {}", e))?;

        Ok(count as usize)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Налаштування
    // ─────────────────────────────────────────────────────────────────────────

    /// Зберегти налаштування
    pub fn save_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
                params![key, value],
            )
            .map_err(|e| format!("Помилка збереження налаштування: {}", e))?;

        Ok(())
    }

    /// Зберегти налаштування (синонім)
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.save_setting(key, value)
    }

    /// Отримати налаштування
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| format!("Помилка підготовки запиту: {}", e))?;

        let mut rows = stmt
            .query_map(params![key], |row| {
                let value: String = row.get(0)?;
                Ok(value)
            })
            .map_err(|e| format!("Помилка виконання запиту: {}", e))?;

        match rows.next() {
            Some(Ok(value)) => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    /// Отримати розмір файлу БД
    pub fn get_db_size(&self) -> Result<u64, String> {
        let path = Self::get_db_path_inner()?;
        let metadata =
            std::fs::metadata(&path).map_err(|e| format!("Помилка отримання розміру БД: {}", e))?;
        Ok(metadata.len())
    }
}
