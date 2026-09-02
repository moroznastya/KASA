//! Push-клієнт каси (ЕТАП 4 offline-first): outbox → сервер.
//!
//! Дизайн: docs/design/sync-schema-design.md, розділи 3 (UUID+ідемпотентність),
//! 4 (outbox, FIFO, retry/backoff), 2.2 (push payload).
//!
//! Обов'язки модуля:
//!   * [`enqueue_receipt`] — атомарний запис локальної транзакції каси:
//!     BEGIN IMMEDIATE → INSERT receipts(+client_uuid, synced=1) →
//!     INSERT outbox(pending) → COMMIT. При будь-якій помилці — ROLLBACK:
//!     не буває «чека без outbox-запису» і навпаки (дизайн 4.4; локальний
//!     stock-ефект — ЕТАП 6, коли каса вестиме власний stock; сьогодні
//!     stock_delta є частиною payload і застосовується сервером при прийомі);
//!   * [`pending_outbox`] — вибірка outbox СТРОГО FIFO за (created_at, id),
//!     пакети до 50 (дизайн 4.2);
//!   * [`push_pending_batch`] — POST /api/v1/sync/push + обробка відповідей
//!     сервера: created/already_exists → done+pushed_at; 5xx/429 → attempts+=1
//!     + exponential backoff next_attempt_at = now + min(2^attempts, 3600)c;
//!     400/422 (per-item error) → failed + last_error (без retry — потрібне
//!     втручання); немає мережі → pending без змін (повтор за подією/30с);
//!     після 10 невдалих спроб (5xx) → failed + алерт (статус видимий — дизайн
//!     4.3);
//!   * [`spawn_push_task`] — циклічний фоновий push (tokio), інтервал 30 с.

use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::{migrations, stock, transactions};

/// Максимум агрегатів на один HTTP-запит push (дизайн 4.2: до 50).
pub const PUSH_BATCH_MAX: usize = 50;
/// Максимум невдалих спроб (5xx) до переходу в failed (дизайн 4.3: 10).
pub const MAX_ATTEMPTS: i64 = 10;
/// Стеля exponential backoff, секунд (дизайн 4.3: min(2^attempts, 3600)).
pub const BACKOFF_CAP_SECS: i64 = 3600;
/// Інтервал циклічного push за замовчуванням (дизайн 4.3: 30 с).
pub const DEFAULT_INTERVAL_SECS: u64 = 30;
/// Мінімальний інтервал циклу.
pub const MIN_INTERVAL_SECS: u64 = 5;

/// Тип агрегата outbox для чека продажу.
pub const TYPE_RECEIPT: &str = "receipt";
/// Тип агрегата outbox для чека повернення.
pub const TYPE_RETURN_RECEIPT: &str = "return_receipt";

/// Результат атомарного запису локальної транзакції каси.
#[derive(Debug, Clone, PartialEq)]
pub struct EnqueuedReceipt {
    /// rowid у receipts (локальний id чека каси).
    pub receipt_id: i64,
    /// UUIDv4 каси — ідемпотентний ключ push (той самий у receipts і outbox).
    pub client_uuid: String,
    /// rowid у outbox.
    pub outbox_id: i64,
}

// ─── Атомарний запис локальної транзакції (A.1 + A.2) ──────────────────────

/// Мапає `receipt_type` з JSON чека каси на тип агрегата outbox (дизайн 2.2).
pub fn outbox_type_of(receipt_json: &str) -> &'static str {
    match serde_json::from_str::<Value>(receipt_json) {
        Ok(v) => match v["receipt_type"].as_str() {
            Some("return") => TYPE_RETURN_RECEIPT,
            _ => TYPE_RECEIPT,
        },
        Err(_) => TYPE_RECEIPT,
    }
}

/// Генерує payload агрегата outbox (конверт дизайну 2.2).
fn envelope(
    outbox_type: &str,
    client_uuid: &str,
    store_id: Option<&str>,
    receipt_json: &str,
) -> Result<Value, String> {
    let payload: Value = serde_json::from_str(receipt_json)
        .map_err(|e| format!("Чек каси — невалідний JSON: {e}"))?;
    Ok(json!({
        "type": outbox_type,
        "client_uuid": client_uuid,
        "store_id": store_id,
        "created_at": Utc::now().to_rfc3339(),
        "payload": payload,
    }))
}

/// Атомарний запис чека каси + outbox (дизайн 4.4) з ЯВНИМ client_uuid.
///
/// ОДНА SQLite-транзакція (BEGIN IMMEDIATE): INSERT receipts(+client_uuid) →
/// INSERT outbox(pending). ROLLBACK при будь-якій помилці — жодного чека без
/// outbox-запису і навпаки. `client_uuid` зберігається в ОБОХ таблицях — той
/// самий (A.1).
///
/// Публічний для тестів ідемпотентності/rollback; у production client_uuid
/// генерує [`enqueue_receipt`].
pub fn enqueue_receipt_with_uuid(
    conn: &mut Connection,
    receipt_json: &str,
    store_id: Option<&str>,
    client_uuid: &str,
) -> Result<EnqueuedReceipt, String> {
    let outbox_type = outbox_type_of(receipt_json);
    let payload = envelope(outbox_type, client_uuid, store_id, receipt_json)?.to_string();

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE (запис чека): {e}"))?;

    // 1. Агрегат: локальний чек каси (data = повний JSON чека).
    tx.execute(
        "INSERT INTO receipts (data, store_id, synced, client_uuid) VALUES (?1, ?2, 1, ?3)",
        params![receipt_json, store_id, client_uuid],
    )
    .map_err(|e| format!("INSERT receipts (client_uuid={client_uuid}): {e}"))?;
    let receipt_id = tx.last_insert_rowid();

    // 2. Черга push: outbox(pending) — той самий client_uuid.
    tx.execute(
        "INSERT INTO outbox (type, client_uuid, payload, status) VALUES (?1, ?2, ?3, 'pending')",
        params![outbox_type, client_uuid, payload],
    )
    .map_err(|e| format!("INSERT outbox (client_uuid={client_uuid}): {e}"))?;
    let outbox_id = tx.last_insert_rowid();

    // 2.5. ЕТАП 6: деталізація чека (receipt_items) + локальний stock-ефект
    // (дизайн 4.4) — У ТІЙ САМІЙ транзакції, що receipts+outbox: продаж −qty,
    // повернення +qty. Будь-яка помилка тут → tx drop → ROLLBACK і чека,
    // і outbox, і stock (жодного часткового стану).
    let receipt_val: Value = serde_json::from_str(receipt_json)
        .map_err(|e| format!("Чек каси — невалідний JSON: {e}"))?;
    match receipt_val.get("items") {
        None => {} // чек без позицій (легально для деяких типів)
        Some(Value::Array(items)) => {
            for item in items {
                let pid = item.get("product_id").and_then(|p| p.as_str());
                let qty_milli = item.get("quantity").map(stock::qty_to_milli).unwrap_or(0);
                // Нормалізована деталізація (локальний перегляд без парсингу data).
                let price_v = item.get("price");
                let price_s = price_v.and_then(|v| v.as_str()).map(|s| s.to_string()).or_else(
                    || price_v.and_then(|v| v.as_f64()).map(|f| format!("{f:.2}")),
                );
                let snap_v = item.get("price_snapshot").or(price_v);
                let snap_s = snap_v
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| snap_v.and_then(|v| v.as_f64()).map(|f| format!("{f:.2}")));
                let sum_s = item
                    .get("sum")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        item.get("sum").and_then(|v| v.as_f64()).map(|f| format!("{f:.2}"))
                    })
                    .or_else(|| {
                        let price_f = price_v.and_then(|v| v.as_f64())
                            .or_else(|| price_v.and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()))
                            .unwrap_or(0.0);
                        let q = stock::milli_to_units(qty_milli);
                        Some(format!("{:.2}", price_f * q))
                    });
                tx.execute(
                    "INSERT INTO receipt_items
                     (receipt_client_uuid, product_id, barcode, name_snapshot,
                      quantity, price, price_snapshot, sum)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        client_uuid,
                        pid,
                        item.get("barcode").and_then(|v| v.as_str()),
                        item.get("name_snapshot").and_then(|v| v.as_str()),
                        qty_milli,
                        price_s,
                        snap_s,
                        sum_s,
                    ],
                )
                .map_err(|e| format!("INSERT receipt_items (чек {client_uuid}): {e}"))?;

                // Stock-ефект: лише коли каса знає свою точку (store_id) і
                // позиція має product_id. Від'ємний залишок допустимий
                // (локальний stock стартує порожнім; див. stock.rs).
                if let (Some(sid), Some(pid), q) = (store_id, pid, qty_milli) {
                    if q != 0 {
                        let delta = if outbox_type == TYPE_RETURN_RECEIPT { q } else { -q };
                        stock::apply_stock_delta(&tx, sid, pid, delta)
                            .map_err(|e| format!("stock-ефект чека {client_uuid}: {e}"))?;
                    }
                }
            }
        }
        Some(_) => {
            return Err(format!("Чек каси (client_uuid={client_uuid}): поле items — не масив"));
        }
    }

    // 3. COMMIT. Будь-яка помилка вище → tx drop → ROLLBACK.
    tx.commit()
        .map_err(|e| format!("COMMIT (запис чека): {e}"))?;

    Ok(EnqueuedReceipt {
        receipt_id,
        client_uuid: client_uuid.to_string(),
        outbox_id,
    })
}

/// Атомарний запис чека каси + outbox: генерує client_uuid (UUIDv4) ОДИН раз
/// при створенні локальної транзакції (дизайн 3.1) і зберігає його в
/// receipts(client_uuid) та outbox.client_uuid — той самий.
pub fn enqueue_receipt(
    conn: &mut Connection,
    receipt_json: &str,
    store_id: Option<&str>,
) -> Result<EnqueuedReceipt, String> {
    let client_uuid = Uuid::new_v4().to_string();
    enqueue_receipt_with_uuid(conn, receipt_json, store_id, &client_uuid)
}

// ─── Вибірка outbox (FIFO) ─────────────────────────────────────────────────

/// Рядок outbox, готовий до відправки.
#[derive(Debug, Clone)]
pub struct OutboxItem {
    pub id: i64,
    pub outbox_type: String,
    pub client_uuid: String,
    /// Конверт агрегата (дизайн 2.2) — тіло POST /sync/push.
    pub payload: String,
}

/// Вибірка до `limit` pending-агрегатів СТРОГО FIFO за **rowid** (id).
///
/// Дизайн 4.2 (FIFO) + 6.2 (зсув годинника): порядок outbox — локальний
/// порядок вставки (rowid), НЕ created_at (годинник каси може бути
/// зсунутий — created_at лише метадата). Враховує next_attempt_at (backoff).
pub fn pending_outbox(conn: &Connection, limit: usize) -> Result<Vec<OutboxItem>, String> {
    let limit = limit.min(PUSH_BATCH_MAX);
    let mut stmt = conn
        .prepare(
            "SELECT id, type, client_uuid, payload FROM outbox \
             WHERE status = 'pending' AND next_attempt_at <= datetime('now') \
             ORDER BY id LIMIT ?1",
        )
        .map_err(|e| format!("підготовка SELECT outbox: {e}"))?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok(OutboxItem {
                id: row.get(0)?,
                outbox_type: row.get(1)?,
                client_uuid: row.get(2)?,
                payload: row.get(3)?,
            })
        })
        .map_err(|e| format!("SELECT outbox: {e}"))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| format!("рядок outbox: {e}"))?);
    }
    Ok(items)
}

/// Лічильник агрегатів, що очікують вивантаження (pending + failed —
/// failed «потребує уваги», дизайн 4.3).
pub fn pending_count(conn: &Connection) -> Result<usize, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM outbox WHERE status IN ('pending','failed')",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as usize)
    .map_err(|e| format!("COUNT outbox: {e}"))
}

/// Статистика outbox для UI-статусу синхронізації (ЕТАП 5, sync_status):
/// pending/failed лічильники, остання помилка (failed.last_error останнього
/// за rowid) і час останньої успішної синхронізації (MAX(pushed_at) по done).
#[derive(Debug, Clone, Default)]
pub struct OutboxStats {
    /// Агрегати, що чекають вивантаження (status='pending').
    pub pending: usize,
    /// Агрегати, що потребують уваги (status='failed', дизайн 4.3).
    pub failed: usize,
    /// last_error останнього failed-агрегата (за rowid).
    pub last_error: Option<String>,
    /// MAX(pushed_at) по done — час останнього успішного push.
    pub last_sync_at: Option<String>,
}

/// Поточна статистика outbox (для sync_status / get_offline_stats).
pub fn outbox_stats(conn: &Connection) -> Result<OutboxStats, String> {
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("COUNT outbox pending: {e}"))?;
    let failed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'failed'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("COUNT outbox failed: {e}"))?;
    let last_error: Option<String> = conn
        .query_row(
            "SELECT last_error FROM outbox WHERE status = 'failed' \
             AND last_error IS NOT NULL ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|e| format!("SELECT outbox last_error: {e}"))?
        .flatten();
    // MAX(...) ЗАВЖДИ повертає один рядок (можливо з NULL) — без .optional().
    let last_sync_at: Option<String> = conn
        .query_row(
            "SELECT MAX(pushed_at) FROM outbox WHERE status = 'done' \
             AND pushed_at IS NOT NULL",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|e| format!("SELECT outbox last_sync_at: {e}"))?;
    Ok(OutboxStats {
        pending: pending as usize,
        failed: failed as usize,
        last_error,
        last_sync_at,
    })
}

/// Стан здоров'я синхронізації каси (ЕТАП 7 — моніторинг sync_log).
///
/// Читає outbox + sync_log і віддає JSON для Tauri-команди `sync_health`
/// та поля `health` у `sync_status`:
/// ```json
/// {
///   "outbox_pending": 0,
///   "outbox_failed": 0,
///   "stale_failed_since": "…" | null,  // MIN(created_at) по failed outbox
///   "last_push_ok_at": "…" | null,     // MAX(ts) sync_log kind='push_ok'
///   "last_pull_ok_at": "…" | null,     // MAX(ts) kind='pull_ok'
///   "last_push_fail_at": "…" | null,   // MAX(ts) kind='push_fail'
///   "last_error": "…" | null,          // last_error останнього failed
///   "degraded": false
/// }
/// ```
///
/// **degraded** (алерт на failed/стагнацію, дизайн розділ 9):
///   1. `outbox_failed > 0` — агрегати, що потребують уваги (дизайн 4.3);
///   2. АБО є pending, чий `next_attempt_at` прострочений більше ніж на
///      [`BACKOFF_CAP_SECS`] (3600с — стеля exponential backoff): каса не
///      пробувала слати понад 1 годину — цикл push мертвий/зупинений.
pub fn sync_health(conn: &Connection) -> Result<serde_json::Value, String> {
    let stats = outbox_stats(conn)?;
    // MAX(ts) ЗАВЖДИ повертає рядок (можливо NULL) — без .optional().
    let last_ts = |kind: &str| -> Result<Option<String>, String> {
        conn.query_row(
            "SELECT MAX(ts) FROM sync_log WHERE kind = ?1",
            params![kind],
            |r| r.get::<_, Option<String>>(0),
        )
        .map_err(|e| format!("SELECT sync_log MAX(ts) '{kind}': {e}"))
    };
    let stale_failed_since: Option<String> = conn
        .query_row(
            "SELECT MIN(created_at) FROM outbox WHERE status = 'failed' AND created_at IS NOT NULL",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|e| format!("SELECT MIN(created_at) failed: {e}"))?
        .flatten();
    let stale_pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM outbox WHERE status = 'pending' \
             AND next_attempt_at IS NOT NULL \
             AND next_attempt_at < datetime('now', ?1)",
            params![format!("-{} seconds", BACKOFF_CAP_SECS)],
            |r| r.get(0),
        )
        .map_err(|e| format!("COUNT stale pending (стагнація): {e}"))?;
    let degraded = stats.failed > 0 || stale_pending > 0;
    Ok(json!({
        "outbox_pending": stats.pending,
        "outbox_failed": stats.failed,
        "stale_failed_since": stale_failed_since,
        "last_push_ok_at": last_ts("push_ok")?,
        "last_pull_ok_at": last_ts("pull_ok")?,
        "last_push_fail_at": last_ts("push_fail")?,
        "last_error": stats.last_error,
        "degraded": degraded,
    }))
}

// ─── Backoff ────────────────────────────────────────────────────────────────

/// Затримка exponential backoff: min(2^attempts, BACKOFF_CAP_SECS) (дизайн 4.3).
/// attempts — номер НЕВДАЛОЇ спроби (1, 2, 3, …): 1с → 2с → 4с → … → 1 год.
pub fn backoff_delay_secs(attempts: i64) -> i64 {
    let attempts = attempts.max(1);
    let exp = 1i64.checked_shl(attempts as u32).unwrap_or(i64::MAX);
    exp.min(BACKOFF_CAP_SECS)
}

// ─── Відповіді сервера ──────────────────────────────────────────────────────

/// Результат обробки одного агрегата сервером (відповідь POST /sync/push).
#[derive(Debug, Clone, Deserialize)]
pub struct ServerPushResult {
    pub client_uuid: String,
    /// created | already_exists | error
    pub status: String,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Підсумок одного циклу push.
#[derive(Debug, Clone, Default)]
pub struct PushSummary {
    pub sent: usize,
    pub done: usize,
    pub already_exists: usize,
    pub failed: usize,
    pub deferred: usize,
}

// ─── HTTP push ──────────────────────────────────────────────────────────────

/// Конфігурація push-клієнта каси.
#[derive(Debug, Clone)]
pub struct PushConfig {
    /// Базовий URL сервера (напр. `http://127.0.0.1:8000`).
    pub base_url: String,
    /// JWT access_token (Bearer) — з логіну каси (PIN/пароль).
    pub token: String,
    /// store_id каси (X-Store-Id) — RLS-контур сервера.
    pub store_id: String,
    /// Шлях до SQLite-БД каси.
    pub db_path: std::path::PathBuf,
    /// Інтервал циклу push (сек; нижче MIN_INTERVAL_SECS — обрізається).
    pub interval_secs: u64,
}

/// Один цикл push: вибірка pending (FIFO, ≤50) → POST → обробка.
///
/// Повертає підсумок. Мережева помилка (сервер недоступний) → Err: статуси
/// outbox НЕ змінюються (pending), повтор за подією/наступним циклом
/// (дизайн 4.3 — «немає мережі»).
pub async fn push_pending_batch(
    db_path: &Path,
    client: &reqwest::Client,
    cfg: &PushConfig,
) -> Result<PushSummary, String> {
    let mut conn = open_connection(db_path)?;
    // ЕТАП 7b: легасі-агрегати synced=0 (створені до оновлення) підмітаються
    // в outbox при першому ж sync — «не synced=0 в нікуди».
    let _ = transactions::sweep_legacy_unsynced(&mut conn)?;
    let batch = pending_outbox(&conn, PUSH_BATCH_MAX)?;
    if batch.is_empty() {
        return Ok(PushSummary::default());
    }

    let body: Vec<Value> = batch
        .iter()
        .map(|it| serde_json::from_str(&it.payload).unwrap_or(Value::Null))
        .collect();

    let resp = client
        .post(format!("{}/api/v1/sync/push", cfg.base_url))
        .bearer_auth(&cfg.token)
        .header("X-Store-Id", &cfg.store_id)
        .json(&body)
        .send()
        .await;

    // Немає мережі / сервер недоступний: pending без змін (дизайн 4.3).
    // Подія push_fail у sync_log — моніторинг бачить, що push не йде.
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("push недоступний (сервер вимкнений?): {e}");
            // Помилка журналу НЕ блокує основний push-цикл.
            let _ = log_event(&conn, "push_fail", None, Some(&msg), None);
            return Err(msg);
        }
    };

    let status = resp.status();
    let mut summary = PushSummary {
        sent: batch.len(),
        ..Default::default()
    };

    // Серверна помилка (5xx, 429): exponential backoff на ВЕСЬ пакет
    // (дизайн 4.3). Після MAX_ATTEMPTS — failed + алерт.
    if status.is_server_error() || status.as_u16() == 429 {
        for it in &batch {
            defer_or_fail(&mut conn, it, None)?;
        }
        summary.deferred = batch.len();
        return Ok(summary);
    }

    // 400/422: валідація всього пакета — failed без retry.
    if status.is_client_error() {
        let text = resp.text().await.unwrap_or_default();
        for it in &batch {
            mark_failed(&mut conn, it, Some(format!("HTTP {status}: {text}")))?;
        }
        summary.failed = batch.len();
        return Ok(summary);
    }

    // 200: per-item результати сервера. Аномалія: HTTP 200, але тіло —
    // невалідний JSON (дефектний/чужий сервер). Журналюємо push_fail —
    // моніторинг бачить причину, а не мовчить (LOW 5.4, sync_push.rs:537).
    let results: Vec<ServerPushResult> = match resp.json().await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("невалідна відповідь push (HTTP 200, тіло не JSON): {e}");
            for it in &batch {
                let _ = log_event(&conn, "push_fail", None, Some(&msg), None);
                defer_or_fail(&mut conn, it, None)?;
            }
            return Err(msg);
        }
    };
    for it in &batch {
        let result = results.iter().find(|r| r.client_uuid == it.client_uuid);
        match result {
            // created/already_exists → done + pushed_at (дизайн 3.2/4.4).
            Some(r) if r.status == "created" => {
                mark_done(&mut conn, it)?;
                summary.done += 1;
            }
            Some(r) if r.status == "already_exists" => {
                mark_done(&mut conn, it)?;
                summary.already_exists += 1;
            }
            // Валідаційна/бізнес-помилка сервера → failed (без retry).
            Some(r) => {
                mark_failed(&mut conn, it, r.error.clone())?;
                summary.failed += 1;
            }
            // Сервер не повернув результат для агрегата (аномалія):
            // deferred — наступний цикл спробує знову. Подія retry у sync_log
            // (LOW 5.4, sync_push.rs:549-551): при систематичному ігноруванні
            // сервером журнал показує причину, health — stale pending.
            None => {
                summary.deferred += 1;
                let _ = log_event(
                    &conn,
                    "retry",
                    None,
                    Some("сервер не повернув результат для агрегата — повтор"),
                    None,
                );
            }
        }
    }
    Ok(summary)
}

/// attempts += 1; якщо спроба 5xx/429: next_attempt_at = now + backoff;
/// після MAX_ATTEMPTS невдалих спроб → failed (алерт — статус видимий).
/// Поточна кількість спроб агрегата (SELECT attempts).
fn outbox_attempts(conn: &Connection, item_id: i64) -> Result<i64, String> {
    let attempts: i64 = conn
        .query_row(
            "SELECT attempts FROM outbox WHERE id = ?1",
            params![item_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| format!("SELECT attempts outbox: {e}"))?
        .unwrap_or(0);
    Ok(attempts)
}

/// Вставити подію в sync_log (ЕТАП 7, моніторинг).
///
/// Для подій ПОЗА транзакціями стану outbox (мережева помилка циклу).
/// Події, що супроводжують зміну статусу outbox ([`mark_done`],
/// [`mark_failed`], [`defer_or_fail`]), пишуться В ТІЙ САМІЙ транзакції:
/// rollback статусу відкочує і подію — фейкового push_ok у log немає.
pub fn log_event(
    conn: &Connection,
    kind: &str,
    entity: Option<&str>,
    detail: Option<&str>,
    attempts: Option<i64>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sync_log (kind, entity, detail, attempts) VALUES (?1, ?2, ?3, ?4)",
        params![kind, entity, detail, attempts],
    )
    .map(|_| ())
    .map_err(|e| format!("INSERT sync_log ({kind}): {e}"))
}

/// attempts += 1; якщо спроба 5xx/429: next_attempt_at = now + backoff;
/// після MAX_ATTEMPTS невдалих спроб → failed (алерт — статус видимий).
/// Кожна зміна статусу супроводжується подією sync_log у тій самій
/// транзакції: 'retry' (відкладено) або 'push_fail' (failed).
fn defer_or_fail(
    conn: &mut Connection,
    item: &OutboxItem,
    error: Option<String>,
) -> Result<(), String> {
    let attempts = outbox_attempts(conn, item.id)?;
    let next_attempts = attempts + 1;
    let delay = backoff_delay_secs(next_attempts);
    let offset = format!("+{delay} seconds");
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN (backoff id={}): {e}", item.id))?;

    if next_attempts >= MAX_ATTEMPTS {
        // 10 невдалих спроб (5xx) → failed + last_error (дизайн 4.3).
        let msg = error.unwrap_or_else(|| format!("10 невдалих спроб (5xx)"));
        tx.execute(
            "UPDATE outbox SET attempts = ?1, status = 'failed', last_error = ?2 \
             WHERE id = ?3",
            params![next_attempts, msg, item.id],
        )
        .map_err(|e| format!("UPDATE outbox → failed: {e}"))?;
        tx.execute(
            "INSERT INTO sync_log (kind, entity, detail, attempts) \
             VALUES ('push_fail', ?1, ?2, ?3)",
            params![item.client_uuid, msg, next_attempts],
        )
        .map_err(|e| format!("INSERT sync_log (push_fail): {e}"))?;
    } else {
        tx.execute(
            "UPDATE outbox SET attempts = ?1, next_attempt_at = datetime('now', ?2), last_error = ?3 \
             WHERE id = ?4",
            params![next_attempts, offset, error, item.id],
        )
        .map_err(|e| format!("UPDATE outbox backoff: {e}"))?;
        let detail = format!("5xx/429: спроба {next_attempts}, backoff {delay}с");
        tx.execute(
            "INSERT INTO sync_log (kind, entity, detail, attempts) \
             VALUES ('retry', ?1, ?2, ?3)",
            params![item.client_uuid, detail, next_attempts],
        )
        .map_err(|e| format!("INSERT sync_log (retry): {e}"))?;
    }
    tx.commit()
        .map_err(|e| format!("COMMIT (backoff id={}): {e}", item.id))
}

/// done + pushed_at (успішний прийом сервером) + подія push_ok у sync_log —
/// в тій самій транзакції: rollback статусу відкочує і подію.
fn mark_done(conn: &mut Connection, item: &OutboxItem) -> Result<(), String> {
    let attempts = outbox_attempts(conn, item.id)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN (push_ok id={}): {e}", item.id))?;
    tx.execute(
        "UPDATE outbox SET status = 'done', pushed_at = datetime('now'), last_error = NULL \
         WHERE id = ?1",
        params![item.id],
    )
    .map_err(|e| format!("UPDATE outbox → done: {e}"))?;
    tx.execute(
        "INSERT INTO sync_log (kind, entity, detail, attempts) VALUES ('push_ok', ?1, NULL, ?2)",
        params![item.client_uuid, attempts],
    )
    .map_err(|e| format!("INSERT sync_log (push_ok): {e}"))?;
    tx.commit()
        .map_err(|e| format!("COMMIT (push_ok id={}): {e}", item.id))
}

/// failed + last_error (без retry — потрібне втручання, дизайн 4.3) +
/// подія push_fail у sync_log (в тій самій транзакції).
fn mark_failed(
    conn: &mut Connection,
    item: &OutboxItem,
    error: Option<String>,
) -> Result<(), String> {
    let attempts = outbox_attempts(conn, item.id)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN (push_fail id={}): {e}", item.id))?;
    tx.execute(
        "UPDATE outbox SET status = 'failed', last_error = ?1 WHERE id = ?2",
        params![error, item.id],
    )
    .map_err(|e| format!("UPDATE outbox → failed: {e}"))?;
    tx.execute(
        "INSERT INTO sync_log (kind, entity, detail, attempts) VALUES ('push_fail', ?1, ?2, ?3)",
        params![item.client_uuid, error, attempts],
    )
    .map_err(|e| format!("INSERT sync_log (push_fail): {e}"))?;
    tx.commit()
        .map_err(|e| format!("COMMIT (push_fail id={}): {e}", item.id))
}

/// Відкриває SQLite-БД каси з міграціями до актуальної версії (0001-0004).
pub fn open_connection(db_path: &Path) -> Result<Connection, String> {
    let conn =
        Connection::open(db_path).map_err(|e| format!("Не вдалося відкрити БД каси: {}", e))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;",
    )
    .map_err(|e| format!("Помилка PRAGMA (WAL/FK): {}", e))?;
    migrations::migrate(&conn)?;
    Ok(conn)
}

// ─── Циклічний push (tokio task) ───────────────────────────────────────────

/// Спавнить фоновий циклічний push (tokio task).
///
/// Інтервал: cfg.interval_secs (мін. MIN_INTERVAL_SECS); перший цикл — одразу
/// після старту; помилки циклу логуються і не зупиняють таск.
pub fn spawn_push_task(cfg: PushConfig) -> tokio::task::JoinHandle<()> {
    let interval_secs = cfg.interval_secs.max(MIN_INTERVAL_SECS);
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
            let started = std::time::Instant::now();
            match push_pending_batch(&cfg.db_path, &client, &cfg).await {
                Ok(s) if s.sent > 0 => eprintln!(
                    "[sync_push] цикл: {}/{} за {:.1}с (done {}, already_exists {}, failed {}, deferred {})",
                    s.sent,
                    s.sent,
                    started.elapsed().as_secs_f64(),
                    s.done,
                    s.already_exists,
                    s.failed,
                    s.deferred
                ),
                Ok(_) => {}
                Err(e) => eprintln!("[sync_push] цикл: помилка: {e}"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory БД з міграціями до актуальної версії (0007).
    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory БД");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("FK");
        migrations::migrate(&conn).expect("міграції");
        conn
    }

    fn sale_receipt_json(number: i64) -> String {
        json!({
            "receipt_type": "sale",
            "receipt_number": number,
            "items": [{"product_id": "t-1", "quantity": 1, "price": "50.00"}],
            "total_amount": "50.00",
            "payment_method": "cash",
            "cash_amount": "50.00",
        })
        .to_string()
    }

    fn outbox_row(conn: &Connection, client_uuid: &str) -> (String, String, String) {
        conn.query_row(
            "SELECT type, status, payload FROM outbox WHERE client_uuid = ?1",
            params![client_uuid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("outbox рядок")
    }

    // ── A.1+A.2: атомарний запис чека + outbox ─────────────────────────────

    /// Чек + outbox в ОДНІЙ транзакції; client_uuid той самий в обох;
    /// payload — конверт дизайну 2.2.
    #[test]
    fn enqueue_writes_receipt_and_outbox_atomically() {
        let mut conn = test_conn();
        let store = "d9be9608-c011-49be-b776-3317ca5e9af6";
        let out =
            enqueue_receipt(&mut conn, &sale_receipt_json(1001), Some(store)).expect("enqueue");

        // receipts: чек з client_uuid, synced=1 (виключений з legacy-черги).
        let (data, uuid, synced): (String, String, i64) = conn
            .query_row(
                "SELECT data, client_uuid, synced FROM receipts WHERE id = ?1",
                params![out.receipt_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("receipt");
        assert_eq!(uuid, out.client_uuid, "той самий client_uuid у receipts");
        assert_eq!(synced, 1, "новий чек поза legacy-чергою (synced=1)");
        assert!(data.contains("50.00"));

        // outbox: той самий client_uuid, pending, конверт 2.2.
        let (otype, status, payload) = outbox_row(&conn, &out.client_uuid);
        assert_eq!(otype, TYPE_RECEIPT);
        assert_eq!(status, "pending");
        let env: Value = serde_json::from_str(&payload).expect("payload JSON");
        assert_eq!(env["type"], TYPE_RECEIPT);
        assert_eq!(env["client_uuid"], out.client_uuid);
        assert_eq!(env["store_id"], store);
        assert!(env["created_at"].is_string());
        assert!(env["payload"]["total_amount"].is_string());
    }

    /// Return-чек → тип агрегата return_receipt (дизайн 2.2).
    #[test]
    fn return_receipt_maps_to_return_type() {
        let mut conn = test_conn();
        let receipt = json!({
            "receipt_type": "return",
            "items": [{"product_id": "t-1", "quantity": 1, "price": "50.00"}],
            "total_amount": "-50.00",
        })
        .to_string();
        let out = enqueue_receipt(&mut conn, &receipt, None).expect("enqueue return");
        let (otype, _, _) = outbox_row(&conn, &out.client_uuid);
        assert_eq!(otype, TYPE_RETURN_RECEIPT);
    }

    // ── Критерій: збій mid-транзакції → ROLLBACK ──────────────────────────

    /// Повторний enqueue того самого client_uuid: UNIQUE idx_receipts_client_uuid
    /// (0004) → INSERT receipts fail всередині транзакції → ROLLBACK: ні
    /// нового чека, ні нового outbox-запису (жодного часткового стану).
    #[test]
    fn mid_transaction_failure_rolls_back_everything() {
        let mut conn = test_conn();
        let first = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("перший");
        let count_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        let ob_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))
            .unwrap();

        // Другий запис з тим самим client_uuid — падає на INSERT receipts
        // (UNIQUE), транзакція має відкотитись повністю.
        let dup =
            enqueue_receipt_with_uuid(&mut conn, &sale_receipt_json(2), None, &first.client_uuid);
        assert!(dup.is_err(), "дублікат client_uuid → помилка");

        let count_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        let ob_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_after, count_before, "ROLLBACK: жодного нового чека");
        assert_eq!(
            ob_after, ob_before,
            "ROLLBACK: жодного нового outbox-запису"
        );
    }

    // ── FIFO порядок (дизайн 4.2) ──────────────────────────────────────────

    /// pending_outbox повертає агрегати СТРОГО в порядку created_at, id.
    #[test]
    fn pending_outbox_is_fifo() {
        let mut conn = test_conn();
        let mut ids = Vec::new();
        for n in 1..=3i64 {
            let out = enqueue_receipt(&mut conn, &sale_receipt_json(n), None).expect("enqueue");
            ids.push(out.outbox_id);
        }
        // Зсув годинника (дизайн 6.2): перший запис отримує created_at на
        // 100с У МАЙБУТНЬОМУ (годинник каси пішов уперед). Порядок outbox —
        // локальний порядок вставки (rowid), НЕ годинник: черга НЕ змінюється.
        conn.execute(
            "UPDATE outbox SET created_at = datetime('now', '+100 seconds') WHERE id = ?1",
            params![ids[0]],
        )
        .expect("created_at першого");

        let batch = pending_outbox(&conn, 50).expect("batch");
        assert_eq!(batch.len(), 3);
        assert_eq!(
            batch[0].id, ids[0],
            "FIFO: перший вставлений — перший у черзі (rowid)"
        );
        assert_eq!(batch[1].id, ids[1], "FIFO: другий за rowid");
        assert_eq!(batch[2].id, ids[2], "FIFO: третій за rowid");
        // Клієнтський код відправляє payload у тому ж порядку.
        assert!(batch[0].payload.contains("\"receipt_number\":1"));
    }

    /// Edge-case «зсув годинника» (дизайн 6.2): два чеки з created_at у
    /// payload «у минулому/майбутньому» — порядок outbox за rowid; тобто
    /// штучний created_at у payload/outbox НЕ переставляє чергу.
    #[test]
    fn clock_skew_does_not_reorder_outbox() {
        let mut conn = test_conn();
        let a = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("a");
        let b = enqueue_receipt(&mut conn, &sale_receipt_json(2), None).expect("b");

        // Перший чек «у минулому» (payload-created_at давній), другий —
        // «у майбутньому». Спершу скоригуємо outbox.created_at обох.
        conn.execute(
            "UPDATE outbox SET created_at = datetime('now', '-1 day') WHERE id = ?1",
            params![a.outbox_id],
        )
        .expect("a в минулому");
        conn.execute(
            "UPDATE outbox SET created_at = datetime('now', '+1 day') WHERE id = ?1",
            params![b.outbox_id],
        )
        .expect("b у майбутньому");

        let batch = pending_outbox(&conn, 50).expect("batch");
        assert_eq!(batch.len(), 2);
        assert_eq!(
            batch[0].id, a.outbox_id,
            "rowid порядок збережено: a перший"
        );
        assert_eq!(
            batch[1].id, b.outbox_id,
            "rowid порядок збережено: b другий"
        );

        // Ідемпотентність не ламається: кожен outbox має свій UNIQUE
        // client_uuid незалежно від часу.
        let (ua, ub): (String, String) = (
            conn.query_row(
                "SELECT client_uuid FROM outbox WHERE id = ?1",
                params![a.outbox_id],
                |r| r.get(0),
            )
            .unwrap(),
            conn.query_row(
                "SELECT client_uuid FROM outbox WHERE id = ?1",
                params![b.outbox_id],
                |r| r.get(0),
            )
            .unwrap(),
        );
        assert_ne!(ua, ub, "унікальні client_uuid при зсуві годинника");
    }

    /// Ліміт пакета: pending_outbox(2) повертає 2 (дизайн 4.2: ≤50).
    #[test]
    fn pending_outbox_respects_batch_limit() {
        let mut conn = test_conn();
        for n in 1..=4i64 {
            enqueue_receipt(&mut conn, &sale_receipt_json(n), None).expect("enqueue");
        }
        let batch = pending_outbox(&conn, 2).expect("batch");
        assert_eq!(batch.len(), 2);
        let max_all = pending_outbox(&conn, 999).expect("batch");
        assert_eq!(max_all.len(), 4);
    }

    // ── Backoff (дизайн 4.3) ───────────────────────────────────────────────

    /// min(2^attempts, 3600): 1→2, 2→4, 3→8, … 10→3600 (стеля).
    #[test]
    fn backoff_is_exponential_with_cap() {
        assert_eq!(
            backoff_delay_secs(0),
            2,
            "attempt 0 → трактується як 1 → 2с"
        );
        assert_eq!(backoff_delay_secs(1), 2);
        assert_eq!(backoff_delay_secs(2), 4);
        assert_eq!(backoff_delay_secs(3), 8);
        assert_eq!(backoff_delay_secs(4), 16);
        assert_eq!(backoff_delay_secs(10), 1024, "2^10 = 1024 (< капу)");
        assert_eq!(backoff_delay_secs(11), 2048);
        assert_eq!(backoff_delay_secs(12), 3600, "2^12 = 4096 → кап 3600");
        assert_eq!(backoff_delay_secs(20), 3600, "стеля 1 год");
    }

    /// 10 невдалих спроб (5xx) → failed + last_error (алерт оператору).
    #[test]
    fn ten_failed_attempts_marks_failed() {
        let mut conn = test_conn();
        let out = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("enqueue");
        let item = pending_outbox(&conn, 10).expect("batch").remove(0);

        // 9 defer_or_fail — pending зростаючий backoff.
        for _ in 0..9 {
            defer_or_fail(&mut conn, &item, None).expect("defer");
        }
        let (status, attempts): (String, i64) = conn
            .query_row(
                "SELECT status, attempts FROM outbox WHERE id = ?1",
                params![out.outbox_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("outbox стан");
        assert_eq!(status, "pending", "до 10-ї спроби — pending");
        assert_eq!(attempts, 9);
        assert_eq!(backoff_delay_secs(9), 512, "9-та спроба: 2^9 = 512с");

        // 10-та невдала спроба → failed.
        defer_or_fail(&mut conn, &item, None).expect("10-та спроба");
        let (status, attempts, err): (String, i64, Option<String>) = conn
            .query_row(
                "SELECT status, attempts, last_error FROM outbox WHERE id = ?1",
                params![out.outbox_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("outbox стан");
        assert_eq!(status, "failed", "10 спроб (5xx) → failed");
        assert_eq!(attempts, 10);
        assert!(
            err.unwrap_or_default().contains("10 невдалих"),
            "алерт: last_error заповнено"
        );
    }

    // ── Статусна модель ────────────────────────────────────────────────────

    /// created/already_exists → done + pushed_at; failed лишається видимим
    /// у pending_count («потребує уваги», дизайн 4.3).
    #[test]
    fn done_and_failed_status_flow() {
        let mut conn = test_conn();
        let a = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("a");
        let b = enqueue_receipt(&mut conn, &sale_receipt_json(2), None).expect("b");

        let items = pending_outbox(&conn, 10).expect("batch");
        assert_eq!(items.len(), 2);
        mark_done(&mut conn, &items[0]).expect("done");
        mark_failed(&mut conn, &items[1], Some("тестова помилка".to_string())).expect("failed");

        assert_eq!(
            pending_count(&conn).expect("count"),
            1,
            "failed видимий (потребує уваги)"
        );
        let (s1, pushed): (String, Option<String>) = conn
            .query_row(
                "SELECT status, pushed_at FROM outbox WHERE id = ?1",
                params![a.outbox_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("a");
        assert_eq!(s1, "done");
        assert!(pushed.is_some(), "pushed_at проставлено");
        let (s2, err): (String, Option<String>) = conn
            .query_row(
                "SELECT status, last_error FROM outbox WHERE id = ?1",
                params![b.outbox_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("b");
        assert_eq!(s2, "failed");
        assert_eq!(err.as_deref(), Some("тестова помилка"));
    }

    // ── ЕТАП 6: receipt_items деталізація + локальний stock-ефект ──────────

    fn stock_level(conn: &Connection, store: &str, product: &str) -> i64 {
        stock::get_stock_level(conn, store, product).expect("stock level")
    }

    /// Продаж: stock −qty, receipt_items записано — У ТІЙ САМІЙ транзакції.
    #[test]
    fn sale_enqueue_applies_stock_delta_and_items() {
        let mut conn = test_conn();
        let store = "d9be9608-c011-49be-b776-3317ca5e9af6";
        let mut c = conn;
        // Передпродажний залишок: +3 шт.
        stock::apply_stock_delta(&c, store, "t-1", 3000).expect("початковий +3");

        let receipt = json!({
            "receipt_type": "sale",
            "receipt_number": 2001,
            "items": [{"product_id": "t-1", "quantity": 2, "price": "50.00",
                       "name_snapshot": "Товар T"}],
            "total_amount": "100.00",
        })
        .to_string();
        let out = enqueue_receipt(&mut c, &receipt, Some(store)).expect("sale");

        assert_eq!(stock_level(&c, store, "t-1"), 1000, "3 − 2 = 1 шт");

        // Деталізація: одна позиція з правильними полями.
        let (pid, qty, price, name): (Option<String>, i64, Option<f64>, Option<String>) = c
            .query_row(
                "SELECT product_id, quantity, price, name_snapshot FROM receipt_items \
                 WHERE receipt_client_uuid = ?1",
                params![out.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("receipt_items");
        assert_eq!(pid.as_deref(), Some("t-1"));
        assert_eq!(qty, 2000, "quantity у міліодиницях");
        assert_eq!(price, Some(50.0), "price — NUMERIC: \"50.00\" збережено REAL 50.0");
        assert_eq!(name.as_deref(), Some("Товар T"));
    }

    /// Повернення: stock +qty (товар повертається на склад).
    #[test]
    fn return_enqueue_applies_positive_stock_delta() {
        let mut conn = test_conn();
        let store = "d9be9608-c011-49be-b776-3317ca5e9af6";
        let mut c = conn;
        let receipt = json!({
            "receipt_type": "return",
            "receipt_number": 2002,
            "items": [{"product_id": "t-1", "quantity": "0.500", "price": "50.00"}],
            "total_amount": "-25.00",
        })
        .to_string();
        let out = enqueue_receipt(&mut c, &receipt, Some(store)).expect("return");
        assert_eq!(stock_level(&c, store, "t-1"), 500, "повернення +0.5 шт");
        let (otype, _, _) = outbox_row(&c, &out.client_uuid);
        assert_eq!(otype, TYPE_RETURN_RECEIPT);
    }

    /// Без store_id каса не веде stock (немає ключа точки) — але чек і
    /// деталізація записуються (поведінка ЕТАП 4 збережена).
    #[test]
    fn enqueue_without_store_skips_stock_but_writes_items() {
        let mut conn = test_conn();
        let mut c = conn;
        let out = enqueue_receipt(&mut c, &sale_receipt_json(3001), None).expect("sale");
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM receipt_items WHERE receipt_client_uuid = ?1",
                params![out.client_uuid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "деталізація записана");
        let rows: i64 = c
            .query_row("SELECT COUNT(*) FROM stock", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "stock не оновлюється без store_id");
    }

    /// Збій mid-транзакції (items — не масив) → ROLLBACK: ні чека, ні
    /// outbox, ні receipt_items, ні stock-ефекту.
    #[test]
    fn mid_tx_items_error_rolls_back_stock_and_items() {
        let mut conn = test_conn();
        let store = "d9be9608-c011-49be-b776-3317ca5e9af6";
        let mut c = conn;
        stock::apply_stock_delta(&c, store, "t-1", 1000).expect("початковий");

        let bad = json!({
            "receipt_type": "sale",
            "items": "не-масив",
            "total_amount": "10.00",
        })
        .to_string();
        assert!(enqueue_receipt(&mut c, &bad, Some(store)).is_err());

        let counts: (i64, i64, i64, i64) = (
            c.query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0)).unwrap(),
            c.query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0)).unwrap(),
            c.query_row("SELECT COUNT(*) FROM receipt_items", [], |r| r.get(0)).unwrap(),
            stock_level(&c, store, "t-1"),
        );
        assert_eq!(counts, (0, 0, 0, 1000), "ROLLBACK: жодного часткового стану");
    }

    // ── ЕТАП 7: sync_log (моніторинг) ──────────────────────────────────────

    /// mark_done/mark_failed пишуть події push_ok/push_fail у sync_log.
    #[test]
    fn sync_log_records_push_ok_and_fail() {
        let mut conn = test_conn();
        let a = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("a");
        let b = enqueue_receipt(&mut conn, &sale_receipt_json(2), None).expect("b");
        let items = pending_outbox(&conn, 10).expect("batch");

        mark_done(&mut conn, &items[0]).expect("done");
        mark_failed(&mut conn, &items[1], Some("бізнес-помилка".to_string())).expect("failed");

        let log: Vec<(String, String, Option<String>, Option<i64>)> = conn
            .prepare("SELECT kind, entity, detail, attempts FROM sync_log ORDER BY id")
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(log.len(), 2, "дві події: push_ok + push_fail");
        assert_eq!(log[0].0, "push_ok");
        assert_eq!(log[0].1, a.client_uuid, "entity = client_uuid агрегата");
        assert_eq!(log[1].0, "push_fail");
        assert_eq!(log[1].1, b.client_uuid);
        assert_eq!(log[1].2.as_deref(), Some("бізнес-помилка"));
    }

    /// ROLLBACK push-операції НЕ лишає фейкового push_ok у sync_log:
    /// подія пишеться в тій самій транзакції, що й зміна статусу outbox.
    #[test]
    fn sync_log_rollback_leaves_no_fake_push_ok() {
        let mut conn = test_conn();
        let a = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("a");
        let item = pending_outbox(&conn, 10).expect("batch").remove(0);

        // Імітація транзакції push, що відкотилась: статус done + подія
        // push_ok — потім ROLLBACK (як при збої після часткового запису).
        {
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            tx.execute(
                "UPDATE outbox SET status = 'done', pushed_at = datetime('now') WHERE id = ?1",
                params![item.id],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO sync_log (kind, entity) VALUES ('push_ok', ?1)",
                params![item.client_uuid],
            )
            .unwrap();
            // tx drop без commit = ROLLBACK.
        }

        let n_log: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_log, 0, "ROLLBACK: sync_log порожній — фейкового push_ok немає");
        let status: String = conn
            .query_row("SELECT status FROM outbox WHERE id = ?1", params![a.outbox_id], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "pending", "ROLLBACK: статус outbox не змінився");
    }

    /// sync_health: degraded=true при failed>0; поля заповнені.
    #[test]
    fn sync_health_degraded_when_failed() {
        let mut conn = test_conn();
        let h0 = sync_health(&conn).expect("health (порожня каса)");
        assert_eq!(h0["degraded"], false, "чиста каса — не degraded");
        assert_eq!(h0["outbox_pending"], 0);

        let a = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("a");
        let _ = a;
        let items = pending_outbox(&conn, 10).expect("batch");
        mark_done(&mut conn, &items[0]).expect("done");

        let h1 = sync_health(&conn).expect("health (після успіху)");
        assert_eq!(h1["degraded"], false);
        assert!(h1["last_push_ok_at"].is_string(), "last_push_ok_at записано");
        assert!(h1["last_push_ok_at"].as_str().unwrap().len() >= 19);

        let b = enqueue_receipt(&mut conn, &sale_receipt_json(2), None).expect("b");
        let _ = b;
        let items = pending_outbox(&conn, 10).expect("batch");
        mark_failed(&mut conn, &items[0], Some("помилка валідації".to_string())).expect("failed");

        let h2 = sync_health(&conn).expect("health (після failed)");
        assert_eq!(h2["degraded"], true, "degraded: outbox_failed > 0");
        assert_eq!(h2["outbox_failed"], 1);
        assert_eq!(h2["last_error"].as_str(), Some("помилка валідації"));
        assert!(h2["stale_failed_since"].is_string(), "stale_failed_since = MIN(created_at)");
    }

    /// sync_health: degraded=true при стагнації (pending з next_attempt_at,
    /// простроченим більше BACKOFF_CAP_SECS); нормальний backoff — ні.
    #[test]
    fn sync_health_degraded_on_stale_pending_only() {
        let mut conn = test_conn();
        // 1. pending зі свіжим next_attempt_at (backoff у розумних межах) —
        //    каса активно пробує — degraded=false.
        let a = enqueue_receipt(&mut conn, &sale_receipt_json(1), None).expect("a");
        conn.execute(
            "UPDATE outbox SET next_attempt_at = datetime('now', '+300 seconds') \
             WHERE id = ?1",
            params![a.outbox_id],
        )
        .unwrap();
        let h1 = sync_health(&conn).expect("health");
        assert_eq!(h1["degraded"], false, "backoff < капу — не стагнація");

        // 2. pending, що не рухається понад кап (3600с) — цикл мертвий.
        conn.execute(
            "UPDATE outbox SET next_attempt_at = datetime('now', '-7200 seconds') \
             WHERE id = ?1",
            params![a.outbox_id],
        )
        .unwrap();
        let h2 = sync_health(&conn).expect("health");
        assert_eq!(h2["degraded"], true, "стагнація: pending не рухається > 1 год");
        assert_eq!(h2["outbox_failed"], 0, "degraded саме через стагнацію, не failed");
    }

    /// log_event приймає pull-події (pull_ok/pull_fail) — той самий журнал.
    #[test]
    fn sync_log_accepts_pull_events() {
        let mut conn = test_conn();
        log_event(&conn, "pull_ok", Some("products"), Some("→ v7"), None).expect("pull_ok");
        log_event(&conn, "pull_fail", Some("suppliers"), Some("GET: мережа"), None).expect("pull_fail");
        let h = sync_health(&conn).expect("health");
        assert!(h["last_pull_ok_at"].is_string());
        let kinds: Vec<String> = conn
            .prepare("SELECT kind FROM sync_log ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(kinds, vec!["pull_ok".to_string(), "pull_fail".to_string()]);
    }
}
