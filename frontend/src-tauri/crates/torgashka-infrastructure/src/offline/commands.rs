// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Команди офлайн-режиму
// ─────────────────────────────────────────────────────────────────────────────
//
// Забезпечує роботу POS-системи без інтернету:
//   - Кешування товарів (офлайн-довідник)
//   - Локальне збереження чеків (outbox-шлях, ЕТАП 4/5)
//   - Синхронізація при поновленні з'єднання (sync_now / фоновий push)
//
// ЕТАП 5: команди переведені на НОВИЙ механізм (offline/sync_push.rs +
// offline/snapshots.rs). Стара таблиця/потік (save_receipt_offline_for_store,
// count_unsynced_receipts, mark_receipt_synced у db.rs) НЕ видаляється —
// міграції 0001-0003 недоторкані; нові чеки в неї більше не пишуться.
// ─────────────────────────────────────────────────────────────────────────────

use crate::offline::db::OfflineDatabase;
use crate::offline::snapshots;
use crate::offline::stock;
use crate::offline::transactions;
use crate::offline::sync_push::{self, PushConfig};
use std::sync::atomic::{AtomicBool, Ordering};

// ─────────────────────────────────────────────────────────────────────────────
// Допоміжна функція: отримати екземпляр БД
// ─────────────────────────────────────────────────────────────────────────────

fn get_db() -> Result<OfflineDatabase, String> {
    OfflineDatabase::new()
}

/// Відкрити з'єднання до стандартної БД каси (міграції до актуальної версії).
fn open_db_conn() -> Result<rusqlite::Connection, String> {
    let path = OfflineDatabase::default_db_path()?;
    sync_push::open_connection(&path)
}

/// Прочитати SQLite-ключ налаштування (settings.key).
fn get_setting_conn(conn: &rusqlite::Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| {
        if e == rusqlite::Error::QueryReturnedNoRows {
            Ok(None)
        } else {
            Err(format!("SELECT settings.{key}: {e}"))
        }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Фоновий push-цикл (один на процес; налаштування з SQLite settings)
// ─────────────────────────────────────────────────────────────────────────────

static PUSH_TASK_STARTED: AtomicBool = AtomicBool::new(false);

/// Запустити фоновий циклічний push (spawn_push_task), якщо:
///   * налаштування вже збережені (server_url + api_token + store_id) і
///   * таск ще не запущено (один на процес).
///
/// Викликається з `setup` (lib.rs) і після `set_setting` — коли фронт
/// зберігає конфігурацію синхронізації. Не-помилка, якщо налаштувань немає:
/// повертає Ok(false), таск стартує пізніше (sync_now або наступний
/// set_setting).
pub fn ensure_push_task_started() -> Result<bool, String> {
    if PUSH_TASK_STARTED.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let path = OfflineDatabase::default_db_path()?;
    let conn = sync_push::open_connection(&path)?;
    let (Some(base_url), Some(token), Some(store_id)) = (
        get_setting_conn(&conn, "server_url")?,
        get_setting_conn(&conn, "api_token")?,
        get_setting_conn(&conn, "store_id")?,
    ) else {
        return Ok(false); // не налаштовано — фоновий цикл пізніше
    };
    if base_url.trim().is_empty() || token.trim().is_empty() || store_id.trim().is_empty() {
        return Ok(false);
    }
    let cfg = PushConfig {
        base_url,
        token,
        store_id,
        db_path: path,
        interval_secs: sync_push::DEFAULT_INTERVAL_SECS,
    };
    if PUSH_TASK_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(false); // хтось уже запустив
    }
    // spawn_push_task робить tokio::spawn — потребує tokio-контексту;
    // tauri::async_runtime::spawn гарантує його з будь-якого потоку.
    let _ = tauri::async_runtime::spawn(async move {
        sync_push::spawn_push_task(cfg);
    });
    Ok(true)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Команди
// ─────────────────────────────────────────────────────────────────────────────

/// Перевірити чи доступний офлайн-режим
#[tauri::command]
pub fn is_offline_available() -> Result<bool, String> {
    Ok(true)
}

/// Отримати кількість несинхронізованих транзакцій.
///
/// ЕТАП 5 (нова семантика): лічильник outbox status='pending' — старий UI
/// бачить реальний стан черги push, а не legacy-таблицю.
#[tauri::command]
pub fn get_unsynced_count() -> Result<usize, String> {
    let conn = open_db_conn()?;
    Ok(sync_push::outbox_stats(&conn)?.pending)
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

/// Зберегти чек локально (офлайн) — НОВИЙ outbox-шлях (ЕТАП 4/5).
///
/// Послідовність:
///   1. `snapshots::validate_sale_allowed` — відхилити продаж товару,
///      позначеного видаленим у локальному довіднику (дизайн 6.2);
///   2. `snapshots::enrich_receipt_with_snapshots` — items отримують
///      price_snapshot/name_snapshot з products_v2 на МОМЕНТ продажу;
///   3. `sync_push::enqueue_receipt` — атомарний запис чека (+client_uuid)
///      та outbox(pending) в ОДНІЙ SQLite-транзакції.
///
/// Повертає локальний id чека (receipts.id) — сумісно зі старою сигнатурою.
#[tauri::command]
pub fn save_receipt_offline(receipt_json: String, store_id: Option<String>) -> Result<i64, String> {
    let mut conn = open_db_conn()?;
    // 1. Продаж видаленого товару — заборонено (товар «зникає» з продажу
    //    після того, як pull доставив видалення; дизайн 6.2).
    snapshots::validate_sale_allowed(&conn, &receipt_json)?;
    // 2. Снапшоти назв/цін на момент продажу (розділ 2.2).
    let enriched = snapshots::enrich_receipt_with_snapshots(&conn, &receipt_json)?;
    // 3. Атомарний запис: чек + outbox.
    let out = sync_push::enqueue_receipt(&mut conn, &enriched, store_id.as_deref())?;
    Ok(out.receipt_id)
}

/// Отримати несинхронізовані чеки — LEGACY (стара synced-черга).
///
/// Нові чеки (ЕТАП 4/5) йдуть через outbox і сюди не потрапляють; команда
/// лишена для сумісності зі старими рядками (створеними до ЕТАП 4, synced=0)
/// і старим фронтом. Новий UI використовує sync_status()/sync_now().
#[tauri::command]
pub fn get_unsynced_receipts() -> Result<Vec<serde_json::Value>, String> {
    let db = get_db()?;
    db.get_unsynced_receipts()
}

/// Позначити чек як синхронізований — no-op (ЕТАП 5).
///
/// Статусами outbox керує push-механізм (pending → done/failed); legacy-потік
/// більше не використовується для нових чеків. Залишено для сумісності
/// сигнатури зі старим фронтом.
#[tauri::command]
pub fn mark_receipt_synced(_receipt_id: i64) -> Result<(), String> {
    Ok(())
}

/// Отримати налаштування
#[tauri::command]
pub fn get_setting(key: String) -> Result<Option<String>, String> {
    let db = get_db()?;
    db.get_setting(&key)
}

/// Зберегти налаштування.
///
/// Після збереження ключів синхронізації (server_url/api_token/store_id)
/// пробує запустити фоновий push-цикл (якщо всі три присутні і таск ще не
/// запущено). Помилка spawn не валить збереження.
#[tauri::command]
pub fn set_setting(key: String, value: String) -> Result<(), String> {
    let db = get_db()?;
    db.set_setting(&key, &value)?;
    let _ = ensure_push_task_started();
    Ok(())
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


// ─────────────────────────────────────────────────────────────────────────────
// ЕТАП 6: самодостатні операції каси (закупка/інвентаризація/переміщення/
// списання). Кожна: локальний запис агрегата (таблиця 0006, synced=0) +
// stock-ефект АТОМАРНО (offline/transactions.rs) — працює з вимкненим
// сервером. Повертають client_uuid агрегата. Payload — той самий JSON, що
// фронт відправляє на /v2-ендпоінт сервера (data зберігається цілком).
// Доставка на сервер (outbox) — ЕТАП 7 (серверний Rust-фасад приймає лише
// sale/return; див. transactions.rs doc-коментар).
// ─────────────────────────────────────────────────────────────────────────────

/// Локальна закупка: items[].product_id/quantity → stock +qty.
#[tauri::command]
pub fn save_purchase_order_offline(payload: String, store_id: String) -> Result<String, String> {
    let mut conn = open_db_conn()?;
    let out = transactions::enqueue_transaction(
        &mut conn,
        transactions::TYPE_PURCHASE_ORDER,
        &payload,
        &store_id,
    )?;
    Ok(out.client_uuid)
}

/// Локальна інвентаризація: items[].product_id/fact_quantity → stock = факт.
#[tauri::command]
pub fn save_inventory_offline(payload: String, store_id: String) -> Result<String, String> {
    let mut conn = open_db_conn()?;
    let out = transactions::enqueue_transaction(
        &mut conn,
        transactions::TYPE_INVENTORY,
        &payload,
        &store_id,
    )?;
    Ok(out.client_uuid)
}

/// Локальне переміщення: from_store_id/to_store_id визначає сторону каси
/// (from=каса → −qty, to=каса → +qty; чуже → тільки запис).
#[tauri::command]
pub fn save_transfer_offline(payload: String, store_id: String) -> Result<String, String> {
    let mut conn = open_db_conn()?;
    let out = transactions::enqueue_transaction(
        &mut conn,
        transactions::TYPE_TRANSFER,
        &payload,
        &store_id,
    )?;
    Ok(out.client_uuid)
}

/// Локальне списання: items[].product_id/quantity → stock −qty.
#[tauri::command]
pub fn save_write_off_offline(payload: String, store_id: String) -> Result<String, String> {
    let mut conn = open_db_conn()?;
    let out = transactions::enqueue_transaction(
        &mut conn,
        transactions::TYPE_WRITE_OFF,
        &payload,
        &store_id,
    )?;
    Ok(out.client_uuid)
}

/// Поточний локальний залишок товару точки (одиниці; 0 — рядка немає).
#[tauri::command]
pub fn get_stock_level(product_id: String, store_id: String) -> Result<f64, String> {
    let conn = open_db_conn()?;
    let milli = stock::get_stock_level(&conn, &store_id, &product_id)?;
    Ok(stock::milli_to_units(milli))
}

/// Локальні залишки всього каталогу точки: [{product_id, name, quantity}].
///
/// LEFT JOIN products_v2: товари без stock-рядка повертаються з quantity=0.
#[tauri::command]
pub fn get_stock_levels(store_id: String) -> Result<serde_json::Value, String> {
    let conn = open_db_conn()?;
    let rows = stock::stock_with_catalog(&conn, &store_id)?;
    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|(id, name, milli)| {
            serde_json::json!({
                "product_id": id,
                "name": name,
                "quantity": stock::milli_to_units(*milli),
            })
        })
        .collect();
    Ok(serde_json::Value::Array(items))
}
/// Статус синхронізації каси (ЕТАП 5) — з outbox.
///
/// JSON-контракт:
/// ```json
/// {
///   "pending_count": 3,          // outbox status='pending'
///   "failed_count": 1,           // outbox status='failed' (потребує уваги)
///   "last_error": "…" | null,    // last_error останнього failed (за rowid)
///   "last_sync_at": "…" | null   // MAX(pushed_at) по done
/// }
/// ```
#[tauri::command]
pub fn sync_status() -> Result<serde_json::Value, String> {
    let conn = open_db_conn()?;
    let s = sync_push::outbox_stats(&conn)?;
    Ok(serde_json::json!({
        "pending_count": s.pending,
        "failed_count": s.failed,
        "last_error": s.last_error,
        "last_sync_at": s.last_sync_at,
        // ЕТАП 7: health — додаткове поле, контракт ЕТАП 5 не ламається
        // (старі поля ті самі).
        "health": sync_push::sync_health(&conn)?,
    }))
}

/// Стан здоров'я синхронізації каси (ЕТАП 7 — моніторинг sync_log).
///
/// Див. [`sync_push::sync_health`] — JSON-контракт. `degraded: true` —
/// алерт: є failed-агрегати (потребують уваги) АБО pending застряг
/// (next_attempt_at прострочений > BACKOFF_CAP_SECS — стагнація циклу).
#[tauri::command]
pub fn sync_health() -> Result<serde_json::Value, String> {
    let conn = open_db_conn()?;
    sync_push::sync_health(&conn)
}

/// Вивантажити outbox на сервер зараз (ручний тригер синхронізації).
///
/// Налаштування читаються з SQLite settings (ключі кладе фронт через
/// set_setting): `server_url`, `api_token`, `store_id`. Якщо хоча б одного
/// немає — `last_error = "not_configured"` (не помилка команди).
///
/// Повторює батчі (до 5 × 50 агрегатів), поки є pending і попередній батч
/// успішний (created/already_exists). Мережева помилка → зупинка, статуси
/// pending без змін (повтор за подією/фоновим циклом).
///
/// JSON-контракт:
/// ```json
/// {
///   "pushed": 2,                 // агрегатів created (done)
///   "already_exists": 1,         // ідемпотентних повторів (done)
///   "failed": 0,                 // per-item помилок сервера (outbox failed)
///   "remaining": 0,              // pending після циклу
///   "last_error": null           // текст помилки | "not_configured" | null
/// }
/// ```
#[tauri::command]
pub async fn sync_now() -> Result<serde_json::Value, String> {
    let path = OfflineDatabase::default_db_path()?;
    let conn = sync_push::open_connection(&path)?;

    // Налаштування з SQLite settings (фронт кладе через set_setting).
    let cfg = match (
        get_setting_conn(&conn, "server_url")?,
        get_setting_conn(&conn, "api_token")?,
        get_setting_conn(&conn, "store_id")?,
    ) {
        (Some(u), Some(t), Some(s))
            if !u.trim().is_empty() && !t.trim().is_empty() && !s.trim().is_empty() =>
        {
            PushConfig {
                base_url: u.trim().to_string(),
                token: t.trim().to_string(),
                store_id: s.trim().to_string(),
                db_path: path.clone(),
                interval_secs: sync_push::DEFAULT_INTERVAL_SECS,
            }
        }
        _ => {
            let stats = sync_push::outbox_stats(&conn)?;
            return Ok(serde_json::json!({
                "pushed": 0,
                "already_exists": 0,
                "failed": 0,
                "remaining": stats.pending,
                "last_error": "not_configured",
            }));
        }
    };

    let client = reqwest::Client::new();
    let mut pushed = 0usize;
    let mut already_exists = 0usize;
    let mut failed = 0usize;
    let mut cycle_error: Option<String> = None;

    // До 5 батчів × 50: поки є pending і попередній батч успішний.
    for _ in 0..5 {
        match sync_push::push_pending_batch(&path, &client, &cfg).await {
            Ok(s) => {
                pushed += s.done;
                already_exists += s.already_exists;
                failed += s.failed;
                if s.sent == 0 {
                    break; // pending вичерпано
                }
                let progressed = s.done > 0 || s.already_exists > 0;
                if !progressed {
                    break; // усе deferred (5xx/backoff) або failed — далі молотити нічого
                }
            }
            Err(e) => {
                cycle_error = Some(e);
                break; // мережа: pending без змін, повтор за подією/циклом
            }
        }
    }

    let stats = sync_push::outbox_stats(&sync_push::open_connection(&path)?)?;
    let last_error = if failed > 0 {
        stats.last_error.or(cycle_error)
    } else {
        cycle_error
    };
    Ok(serde_json::json!({
        "pushed": pushed,
        "already_exists": already_exists,
        "failed": failed,
        "remaining": stats.pending,
        "last_error": last_error,
    }))
}

/// Отримати статистику офлайн-бази (кількість товарів — поточної точки)
#[tauri::command]
pub fn get_offline_stats(store_id: Option<String>) -> Result<serde_json::Value, String> {
    let db = get_db()?;
    let product_count = match store_id {
        Some(ref sid) => db.get_product_count_for_store(sid).unwrap_or(0),
        None => db.get_product_count().unwrap_or(0),
    };
    // ЕТАП 5: «несинхронізовані» = pending outbox (новий механізм).
    let unsynced_count = sync_push::outbox_stats(&open_db_conn()?)
        .map(|s| s.pending)
        .unwrap_or(0);
    let db_size = db.get_db_size().unwrap_or(0);

    Ok(serde_json::json!({
        "products_cached": product_count,
        "unsynced_receipts": unsynced_count,
        "db_size_bytes": db_size,
        "db_path": db.get_db_path(),
    }))
}
