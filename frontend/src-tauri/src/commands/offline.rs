// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Tauri Команди офлайн-режиму
// ─────────────────────────────────────────────────────────────────────────────
//
// Забезпечує роботу POS-системи без інтернету:
//   - Кешування товарів (офлайн-довідник)
//   - Локальне збереження чеків
//   - Синхронізація при поновленні з'єднання
// ─────────────────────────────────────────────────────────────────────────────

use crate::db::OfflineDatabase;

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

/// Кешувати товари (масив JSON-рядків)
#[tauri::command]
pub fn cache_products(products_json: String) -> Result<usize, String> {
    let db = get_db()?;
    db.cache_products(&products_json)
}

/// Отримати кешовані товари (JSON-рядок)
#[tauri::command]
pub fn get_cached_products(search: Option<String>, limit: Option<usize>) -> Result<String, String> {
    let db = get_db()?;
    db.get_cached_products(search.as_deref(), limit.unwrap_or(100))
}

/// Зберегти чек локально (для офлайн-режиму)
#[tauri::command]
pub fn save_receipt_offline(receipt_json: String) -> Result<i64, String> {
    let db = get_db()?;
    db.save_receipt_offline(&receipt_json)
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

/// Очистити кеш товарів
#[tauri::command]
pub fn clear_product_cache() -> Result<usize, String> {
    let db = get_db()?;
    db.clear_product_cache()
}

/// Отримати статистику офлайн-бази
#[tauri::command]
pub fn get_offline_stats() -> Result<serde_json::Value, String> {
    let db = get_db()?;
    let product_count = db.get_product_count().unwrap_or(0);
    let unsynced_count = db.count_unsynced_receipts().unwrap_or(0);
    let db_size = db.get_db_size().unwrap_or(0);

    Ok(serde_json::json!({
        "products_cached": product_count,
        "unsynced_receipts": unsynced_count,
        "db_size_bytes": db_size,
        "db_path": db.get_db_path(),
    }))
}
