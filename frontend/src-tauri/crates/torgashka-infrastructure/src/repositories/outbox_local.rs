//! `outbox_local` — спільна обв'язка standby-адаптерів локальної черги
//! (ADR-0007 §10, §11.1; Фаза 3.3a: `invoice`, `purchase_order`, `inventory`).
//!
//! Схема та сама, що в [`super::outbox_pos`] (Фаза 3.1, еталон):
//!   * **ЧИТАННЯ** — делегат `inner` (локальна репліка — дозволене джерело
//!     читань, §10). Адаптери не переписують читання.
//!   * **ЗАПИС ДОКУМЕНТА** — `offline::transactions::{enqueue_transaction,
//!     enqueue_invoice}`: агрегат + outbox-запис + stock-ефект в ОДНІЙ
//!     SQLite-транзакції (одна реалізація запису, другої не заводимо §11.5).
//!   * **`PgPool` у структурах немає** — адаптер фізично не має чим писати в PG.
//!
//! Навіщо окремий модуль, а не хелпери `outbox_pos`: контракт Фази 3.3a
//! забороняє чіпати `OutboxPos` (еталон лишається байт-в-байт), тому тут — ті
//! самі ПРИМІТИВИ (каталог даних каси, `store_id`, санація помилок), які
//! використовують ТІЛЬКИ нові адаптери. Контур запису не дубльовано.

use serde_json::{json, Value};
use uuid::Uuid;

use crate::offline::{db::OfflineDatabase, sync_push, transactions};

/// Маркер «документ у черзі, primary ще не створив» (той самий рядок, що
/// `OutboxPos::QUEUED_STATUS` — клієнт бачить однаковий бізнес-статус).
pub(crate) const QUEUED_STATUS: &str = "queued";

/// Технічний текст → `torgashka.log`; користувачу — стабільне повідомлення
/// без імен таблиць/колонок/SQL (санація ADR-0007 §D).
pub(crate) fn queue_err(op: &str, tech: impl std::fmt::Display) -> String {
    let tech = tech.to_string();
    crate::embedded_pg::pg_log("ERROR", &format!("[outbox_local] {op}: {tech}"));
    format!("{op}: не вдалося зберегти документ у локальній черзі, спробуйте ще раз")
}

/// Операція, для якої в локальній черзі НЕМАЄ представлення (і приймач
/// `/api/v1/sync/push` її не знає) → явна відмова людським текстом.
/// Прецедент — `outbox_pos.rs::unavailable`.
pub(crate) fn unavailable(op: &str) -> String {
    format!("{op}: операція недоступна на цьому вузлі (потрібен головний сервер)")
}

/// Виконати блокуючу роботу з SQLite у `spawn_blocking` (rusqlite — блокуючий).
pub(crate) async fn with_conn<T, F>(op: &'static str, f: F) -> Result<T, String>
where
    F: FnOnce(&mut rusqlite::Connection) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let join = tokio::task::spawn_blocking(move || -> Result<T, String> {
        let path = OfflineDatabase::default_db_path().map_err(|e| queue_err(op, e))?;
        // Каталог даних каси може бути ще не створений (headless standby).
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                queue_err(op, format!("каталог даних каси {}: {e}", parent.display()))
            })?;
        }
        let mut conn = sync_push::open_connection(&path).map_err(|e| queue_err(op, e))?;
        f(&mut conn)
    })
    .await;
    match join {
        Ok(res) => res,
        Err(e) => Err(queue_err(op, format!("локальний таск: {e}"))),
    }
}

/// `store_id` точки каси: SQLite `settings.store_id` → env `TORGASHKA_STORE_ID`.
/// Порожньо в обох джерелах → «точку продажу не налаштовано» (не 500-текст).
pub(crate) fn resolve_store_id(conn: &rusqlite::Connection) -> Result<String, String> {
    let from_sqlite = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'store_id'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok();
    from_sqlite
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("TORGASHKA_STORE_ID").ok())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "точку продажу не налаштовано".to_string())
}

/// Число з рядка домену → JSON-число (fallback — рядок як є).
pub(crate) fn num(s: &str) -> Value {
    match s.trim().parse::<f64>() {
        Ok(f) if f.is_finite() => json!(f),
        _ => Value::String(s.to_string()),
    }
}

/// `f64` з рядка домену (для DTO-відповіді); некоректне → 0.
pub(crate) fn f64_of(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// Добуток Decimal-рядків (total позиції) як рядок.
pub(crate) fn dec_mul(a: &str, b: &str) -> String {
    use std::str::FromStr;
    match (
        bigdecimal::BigDecimal::from_str(a.trim()),
        bigdecimal::BigDecimal::from_str(b.trim()),
    ) {
        (Ok(x), Ok(y)) => (x * y).to_string(),
        _ => "0".to_string(),
    }
}

/// Сума Decimal-рядків як рядок (total документа).
pub(crate) fn dec_sum(items: impl Iterator<Item = String>) -> String {
    use std::str::FromStr;
    items
        .filter_map(|s| bigdecimal::BigDecimal::from_str(s.trim()).ok())
        .fold(bigdecimal::BigDecimal::from(0), |acc, x| acc + x)
        .to_string()
}

pub(crate) fn uuid_of(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or_else(|_| Uuid::nil())
}

/// Агрегатні типи черги (`offline::transactions::enqueue_transaction`).
pub(crate) async fn enqueue(
    op: &'static str,
    kind: &'static str,
    payload: Value,
) -> Result<transactions::EnqueuedTransaction, String> {
    let payload_json = payload.to_string();
    with_conn(op, move |conn| {
        let store_id = resolve_store_id(conn)?;
        transactions::enqueue_transaction(conn, kind, &payload_json, &store_id)
            .map_err(|e| queue_err(op, e))
    })
    .await
}

/// Прибуткова накладна каси: окремий контур `enqueue_invoice` (колонки
/// `supplier_id`/`number` + деталізація `invoice_items` у тій самій транзакції).
pub(crate) async fn enqueue_invoice_payload(
    op: &'static str,
    payload: Value,
) -> Result<transactions::EnqueuedTransaction, String> {
    let payload_json = payload.to_string();
    with_conn(op, move |conn| {
        let store_id = resolve_store_id(conn)?;
        transactions::enqueue_invoice(conn, &payload_json, &store_id)
            .map_err(|e| queue_err(op, e))
    })
    .await
}
