// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Команди офлайн-режиму
// ─────────────────────────────────────────────────────────────────────────────
//
// Забезпечує роботу POS-системи без інтернету:
//   - Кешування товарів (офлайн-довідник)
//   - Локальне збереження чеків
//   - Синхронізація при поновленні з'єднання
// ─────────────────────────────────────────────────────────────────────────────

use crate::offline::db::OfflineDatabase;

// ─────────────────────────────────────────────────────────────────────────────
// Допоміжна функція: отримати екземпляр БД
// ─────────────────────────────────────────────────────────────────────────────

fn get_db() -> Result<OfflineDatabase, String> {
    OfflineDatabase::new()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Команди
// ─────────────────────────────────────────────────────────────────────────────

/// Перевірити чи доступний офлайн-режим
#[tauri::command]
pub fn is_offline_available() -> Result<bool, String> {
    Ok(true)
}

/// Отримати кількість несинхронізованих чеків
#[tauri::command]
pub fn get_unsynced_count() -> Result<usize, String> {
    let db = get_db()?;
    db.count_unsynced_receipts()
}

/// Кешувати товари (масив JSON-рядків) для поточної точки продажу.
///
/// `store_id` (опціонально) — UUID точки; кеш позначається точкою, щоб
/// офлайн-довідник фільтрував товари поточного магазину.
#[tauri::command]
pub fn cache_products(products_json: String, store_id: Option<String>) -> Result<usize, String> {
    let db = get_db()?;
    db.cache_products_for_store(&products_json, store_id.as_deref())
}

/// Логувати помилку фронтенду у /tmp/torgashka-frontend.log
/// (діагностика «синього екрану»: пастка window.onerror / unhandledrejection)
#[tauri::command]
pub fn log_frontend_error(message: String) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/torgashka-frontend.log")
    {
        let _ = writeln!(
            f,
            "[{}] {}",
            chrono::Local::now().format("%H:%M:%S"),
            message
        );
    }
}

/// Отримати кешовані товари поточної точки (JSON-рядок)
#[tauri::command]
pub fn get_cached_products(
    search: Option<String>,
    limit: Option<usize>,
    store_id: Option<String>,
) -> Result<String, String> {
    let db = get_db()?;
    db.get_cached_products_for_store(search.as_deref(), limit.unwrap_or(100), store_id.as_deref())
}

/// Зберегти чек локально (для офлайн-режиму) з точкою продажу.
///
/// `store_id` зберігається в черзі синхронізації — при відправці на сервер
/// чек потрапляє в точку, де був створений (навіть якщо зараз активна інша).
#[tauri::command]
pub fn save_receipt_offline(receipt_json: String, store_id: Option<String>) -> Result<i64, String> {
    let db = get_db()?;
    db.save_receipt_offline_for_store(&receipt_json, store_id.as_deref())
}

/// Отримати несинхронізовані чеки
#[tauri::command]
pub fn get_unsynced_receipts() -> Result<Vec<serde_json::Value>, String> {
    let db = get_db()?;
    db.get_unsynced_receipts()
}

/// Позначити чек як синхронізований
#[tauri::command]
pub fn mark_receipt_synced(receipt_id: i64) -> Result<(), String> {
    let db = get_db()?;
    db.mark_receipt_synced(receipt_id)
}

/// Отримати налаштування
#[tauri::command]
pub fn get_setting(key: String) -> Result<Option<String>, String> {
    let db = get_db()?;
    db.get_setting(&key)
}

/// Зберегти налаштування
#[tauri::command]
pub fn set_setting(key: String, value: String) -> Result<(), String> {
    let db = get_db()?;
    db.set_setting(&key, &value)
}

/// Очистити кеш товарів (поточної точки, якщо store_id передано)
#[tauri::command]
pub fn clear_product_cache(store_id: Option<String>) -> Result<usize, String> {
    let db = get_db()?;
    match store_id {
        Some(sid) => db.clear_product_cache_for_store(&sid),
        None => db.clear_product_cache(),
    }
}

/// Отримати статистику офлайн-бази (кількість товарів — поточної точки)
#[tauri::command]
pub fn get_offline_stats(store_id: Option<String>) -> Result<serde_json::Value, String> {
    let db = get_db()?;
    let product_count = match store_id {
        Some(ref sid) => db.get_product_count_for_store(sid).unwrap_or(0),
        None => db.get_product_count().unwrap_or(0),
    };
    let unsynced_count = db.count_unsynced_receipts().unwrap_or(0);
    let db_size = db.get_db_size().unwrap_or(0);

    Ok(serde_json::json!({
        "products_cached": product_count,
        "unsynced_receipts": unsynced_count,
        "db_size_bytes": db_size,
        "db_path": db.get_db_path(),
    }))
}
