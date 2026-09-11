//! Локальні транзакції каси поза продажем (ЕТАП 6 offline-first):
//! закупки, інвентаризації, переміщення, списання.
//!
//! Самодостатність (критерій ЕТАПУ 6): кожна операція записується ЛОКАЛЬНО
//! (таблиці міграції 0006) зі stock-ефектом АТОМАРНО (дизайн 4.4) —
//! працює з вимкненим сервером.
//!
//! ЕТАП 7b: серверний push-приймач розширено на всі 4 типи (sync.rs +
//! sync_receivers.rs) — агрегати тепер кладуться в outbox ОДРАЗУ (той самий
//! контур, що й чеки: INSERT агрегат synced=1 + INSERT outbox(pending) в
//! одній транзакції, дизайн 4.4). Для рядків synced=0, накопичених СТАРОЮ
//! версією (до оновлення), є [`sweep_legacy_unsynced`] — при першому sync
//! вони підмітаються в outbox (INSERT OR IGNORE за client_uuid).
//!
//! Формат payload — як його формує фронт для /v2-ендпоінтів сервера
//! (зберігається в data цілком); Rust читає лише items[].product_id/quantity
//! для stock-ефекту та store_id.

use rusqlite::{params, Connection};
use serde_json::Value;
use uuid::Uuid;

use super::{cash, stock};

/// Тип агрегата «закупка» (дизайн 2.2; майбутній outbox-тип ЕТАП 7).
pub const TYPE_PURCHASE_ORDER: &str = "purchase_order";
/// Тип агрегата «інвентаризація».
pub const TYPE_INVENTORY: &str = "inventory";
/// Тип агрегата «переміщення між точками».
pub const TYPE_TRANSFER: &str = "transfer";
/// Тип агрегата «списання».
pub const TYPE_WRITE_OFF: &str = "write_off";
/// Тип outbox «робоча сесія користувача» (ADR-0007 §3.4 #38–#40, клас
/// LOCAL_SQLITE). Сесія вузла живе в SQLite (`work_sessions`, міграція 0009)
/// та в outbox-опу; це ОКРЕМИЙ шлях, а не агрегат каси: [`table_of`] його не
/// знає, `apply_effects` не викликається — **stock-ефекту немає**.
pub const TYPE_WORK_SESSION: &str = "work_session";
/// Тип агрегата «прибуткова накладна» (ADR-0007 §3.4, клас LOCAL_SQLITE).
/// Локальна таблиця — `invoices`/`invoice_items` (міграція 0010), outbox-опу
/// `invoice`; stock-ефект прибуткової — +qty (як purchase_order).
pub const TYPE_INVOICE: &str = "invoice";
/// Тип агрегата «касова операція» (внесення/інкасація) — ADR-0007 §11.6,
/// клас LocalOutbox (§11.1). Локальна таблиця — НАЯВНА `cash_ledger`
/// (міграція 0006): окремого агрегата не заводимо (`table_of`), бо
/// `cash_ledger` уже має контракт client_uuid + data + store_id + synced.
/// Ефект — НЕ stock, а грошовий ящик вузла (`cash::apply_cash_delta`).
pub const TYPE_CASH_OPERATION: &str = "cash_operation";

/// Результат локального запису транзакції (агрегат + outbox).
#[derive(Debug, Clone, PartialEq)]
pub struct EnqueuedTransaction {
    /// Локальний rowid у таблиці агрегата.
    pub id: i64,
    /// UUIDv4 каси — ідемпотентний ключ push (той самий в агрегаті й outbox).
    pub client_uuid: String,
    /// rowid у outbox (ЕТАП 7b: агрегат одразу стає push-кандидатом).
    pub outbox_id: i64,
}

/// Напрямок stock-ефекту переміщення відносно каси.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TransferSide {
    /// Каса — from (відправляє): −qty.
    Out,
    /// Каса — to (приймає): +qty.
    In,
    /// Каса не є стороною переміщення: stock не чіпаємо (агрегат зберігаємо).
    Other,
}

fn transfer_side(payload: &Value, store_id: &str) -> TransferSide {
    let from = payload.get("from_store_id").and_then(|v| v.as_str());
    let to = payload.get("to_store_id").and_then(|v| v.as_str());
    match (from, to) {
        (Some(f), Some(t)) if f == store_id && t != store_id => TransferSide::Out,
        (Some(f), Some(t)) if t == store_id && f != store_id => TransferSide::In,
        _ => TransferSide::Other,
    }
}

/// Застосувати stock-ефект агрегата (ВСЕРЕДИНІ транзакції).
///
/// purchase_order → +qty; write_off → −qty; inventory → АБСОЛЮТНИЙ рівень
/// (факт перерахунку); transfer → ±qty за стороною каси.
fn apply_effects(
    conn: &Connection,
    kind: &str,
    payload: &Value,
    store_id: &str,
) -> Result<(), String> {
    // Касова операція: ефект — грошовий ящик вузла (cash.rs), не stock.
    // Гілка стоїть ДО розбору позицій: у касової операції позицій немає.
    if kind == TYPE_CASH_OPERATION {
        let op = payload
            .get("operation_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let cash_type = payload
            .get("cash_type")
            .and_then(|v| v.as_str())
            .unwrap_or("cash");
        let delta = cash::cash_delta(payload).ok_or_else(|| {
            format!("касова операція: невідомий operation_type '{op}' або некоректна сума")
        })?;
        return cash::apply_cash_delta(conn, store_id, cash_type, delta);
    }
    let items = stock::parse_items(payload);
    if items.is_empty() {
        return Ok(()); // немає позицій з кількістю — ефекту немає
    }
    match kind {
        TYPE_PURCHASE_ORDER | TYPE_INVOICE => {
            for (pid, q) in items {
                stock::apply_stock_delta(conn, store_id, &pid, q)?;
            }
        }
        TYPE_WRITE_OFF => {
            for (pid, q) in items {
                stock::apply_stock_delta(conn, store_id, &pid, -q)?;
            }
        }
        TYPE_INVENTORY => {
            for (pid, q) in items {
                stock::set_stock_level(conn, store_id, &pid, q)?;
            }
        }
        TYPE_TRANSFER => match transfer_side(payload, store_id) {
            TransferSide::In => {
                for (pid, q) in items {
                    stock::apply_stock_delta(conn, store_id, &pid, q)?;
                }
            }
            TransferSide::Out => {
                for (pid, q) in items {
                    stock::apply_stock_delta(conn, store_id, &pid, -q)?;
                }
            }
            TransferSide::Other => {} // чуже переміщення: зберігаємо, stock не міняємо
        },
        other => return Err(format!("невідомий тип транзакції: {other}")),
    }
    Ok(())
}

/// Таблиця агрегата за типом (міграція 0006).
fn table_of(kind: &str) -> Result<&'static str, String> {
    match kind {
        TYPE_PURCHASE_ORDER => Ok("purchase_orders"),
        TYPE_INVENTORY => Ok("inventories"),
        TYPE_TRANSFER => Ok("transfers"),
        TYPE_WRITE_OFF => Ok("write_offs"),
        TYPE_INVOICE => Ok("invoices"),
        TYPE_CASH_OPERATION => Ok("cash_ledger"),
        other => Err(format!("тип транзакції без локальної таблиці: {other}")),
    }
}

/// Чи є тип «своїм» для outbox: 4 локальні агрегати каси + робочі сесії
/// (ADR-0007 §3.4 #38–#40) + чеки продажу/повернення (шлях sync_push).
/// Використовується як білий список (тести + майбутня валідація приймача).
pub fn is_supported_outbox_type(kind: &str) -> bool {
    matches!(
        kind,
        TYPE_PURCHASE_ORDER
            | TYPE_INVENTORY
            | TYPE_TRANSFER
            | TYPE_WRITE_OFF
            | TYPE_WORK_SESSION
            | TYPE_INVOICE
            | TYPE_CASH_OPERATION
            | super::sync_push::TYPE_RECEIPT
            | super::sync_push::TYPE_RETURN_RECEIPT
    )
}

/// Атомарний запис локальної транзакції (ЕТАП 7b): агрегат (таблиця 0006,
/// synced=1) + outbox-запис (pending) + stock-ефект — ОДНА SQLite-транзакція
/// (BEGIN IMMEDIATE → INSERT агрегат → INSERT outbox → ефекти → COMMIT).
/// Помилка будь-де → ROLLBACK: жодного агрегата без outbox-запису і без
/// stock-ефекту (дизайн 4.4, той самий контур, що й чеки enqueue_receipt).
///
/// Повертає client_uuid агрегата (той самий в агрегаті й outbox — A.1).
pub fn enqueue_transaction(
    conn: &mut Connection,
    kind: &str,
    payload_json: &str,
    store_id: &str,
) -> Result<EnqueuedTransaction, String> {
    let table = table_of(kind)?;
    let payload: Value = serde_json::from_str(payload_json)
        .map_err(|e| format!("Payload {kind} — невалідний JSON: {e}"))?;
    let client_uuid = Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    // Конверт push (дизайн 2.2) — ідентичний чекам sync_push::envelope.
    let envelope = serde_json::json!({
        "type": kind,
        "client_uuid": client_uuid,
        "store_id": store_id,
        "created_at": created_at,
        "payload": payload,
    })
    .to_string();

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE ({kind}): {e}"))?;

    // 1. Агрегат: data = payload як є (фронтовий /v2-формат), synced = 1 —
    //    агрегат передано в outbox (наступний push його забере).
    tx.execute(
        &format!(
            "INSERT INTO {table} (client_uuid, store_id, data, synced) \
             VALUES (?1, ?2, ?3, 1)"
        ),
        params![client_uuid, store_id, payload_json],
    )
    .map_err(|e| format!("INSERT {table} (client_uuid={client_uuid}): {e}"))?;
    let id = tx.last_insert_rowid();

    // 2. Outbox-запис (pending) — доставка на сервер (дизайн 4.2).
    tx.execute(
        "INSERT INTO outbox (type, client_uuid, payload, status) \
         VALUES (?1, ?2, ?3, 'pending')",
        params![kind, client_uuid, envelope],
    )
    .map_err(|e| format!("INSERT outbox ({kind}, client_uuid={client_uuid}): {e}"))?;
    let outbox_id = tx.last_insert_rowid();

    // 3. Stock-ефект — у тій самій транзакції.
    apply_effects(&tx, kind, &payload, store_id)
        .map_err(|e| format!("stock-ефект {kind} (client_uuid={client_uuid}): {e}"))?;

    // 4. COMMIT.
    tx.commit().map_err(|e| format!("COMMIT ({kind}): {e}"))?;

    Ok(EnqueuedTransaction {
        id,
        client_uuid,
        outbox_id,
    })
}

/// Позиція накладної для локальної деталізації `invoice_items`.
struct InvoiceLine {
    product_id: Option<String>,
    qty_milli: i64,
    price: Option<String>,
    sum: Option<String>,
}

/// Позиції накладної для локальної деталізації `invoice_items`.
///
/// На відміну від [`stock::parse_items`] (stock-ефект: лише валідні позиції з
/// qty > 0) ДЕТАЛІЗАЦІЯ зберігає КОЖНУ позицію payload — локальний перегляд
/// накладної не має «губити» рядки.
fn invoice_lines(payload: &Value) -> Vec<InvoiceLine> {
    let Some(items) = payload.get("items").and_then(|i| i.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|it| {
            let pid = it
                .get("product_id")
                .or_else(|| it.get("productId"))
                .and_then(|p| p.as_str())
                .map(str::to_string);
            let qty = it
                .get("quantity")
                .or_else(|| it.get("fact_quantity"))
                .map(stock::qty_to_milli)
                .unwrap_or(0);
            InvoiceLine {
                product_id: pid,
                qty_milli: qty,
                price: json_num(it.get("price")),
                sum: json_num(it.get("total").or_else(|| it.get("sum"))),
            }
        })
        .collect()
}

/// число|рядок → текст числа (Decimal-сумісний для SQLite NUMERIC).
fn json_num(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Атомарний локальний запис ПРИБУТКОВОЇ НАКЛАДНОЇ (invoice, ADR-0007 §3.4,
/// клас LOCAL_SQLITE) — ТОЧНА копія контуру [`enqueue_transaction`]: одна
/// `BEGIN IMMEDIATE` → агрегат `invoices` (synced=1) → деталізація
/// `invoice_items` → outbox-запис (pending) → локальний stock **+qty** по
/// позиціях → COMMIT. Помилка будь-де → ROLLBACK: ні агрегата, ні позицій,
/// ні outbox, ні stock-ефекту.
pub fn enqueue_invoice(
    conn: &mut Connection,
    payload_json: &str,
    store_id: &str,
) -> Result<EnqueuedTransaction, String> {
    let payload: Value = serde_json::from_str(payload_json)
        .map_err(|e| format!("Payload {TYPE_INVOICE} — невалідний JSON: {e}"))?;
    // Реквізити з payload — окремими колонками (локальні запити/діагностика).
    let supplier_id = payload
        .get("supplier_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let number = payload
        .get("number")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let client_uuid = Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    // Конверт push (дизайн 2.2) — ідентичний чекам/агрегатам каси.
    let envelope = serde_json::json!({
        "type": TYPE_INVOICE,
        "client_uuid": client_uuid,
        "store_id": store_id,
        "created_at": created_at,
        "payload": payload,
    })
    .to_string();

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE ({TYPE_INVOICE}): {e}"))?;

    // 1. Агрегат: data = payload як є (фронтовий /v2-формат), synced = 1.
    tx.execute(
        "INSERT INTO invoices (client_uuid, store_id, supplier_id, number, data, synced) \
         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![client_uuid, store_id, supplier_id, number, payload_json],
    )
    .map_err(|e| format!("INSERT invoices (client_uuid={client_uuid}): {e}"))?;
    let id = tx.last_insert_rowid();

    // 2. Деталізація позицій (той самий поіменний контур, що receipt_items).
    for line in invoice_lines(&payload) {
        tx.execute(
            "INSERT INTO invoice_items (invoice_client_uuid, product_id, quantity, price, sum) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                client_uuid,
                line.product_id,
                line.qty_milli,
                line.price,
                line.sum
            ],
        )
        .map_err(|e| format!("INSERT invoice_items (client_uuid={client_uuid}): {e}"))?;
    }

    // 3. Outbox-запис (pending) — доставка на сервер (дизайн 4.2).
    tx.execute(
        "INSERT INTO outbox (type, client_uuid, payload, status) \
         VALUES (?1, ?2, ?3, 'pending')",
        params![TYPE_INVOICE, client_uuid, envelope],
    )
    .map_err(|e| format!("INSERT outbox ({TYPE_INVOICE}, client_uuid={client_uuid}): {e}"))?;
    let outbox_id = tx.last_insert_rowid();

    // 4. Stock-ефект прибуткової (+qty по позиціях) — у тій самій транзакції.
    apply_effects(&tx, TYPE_INVOICE, &payload, store_id)
        .map_err(|e| format!("stock-ефект {TYPE_INVOICE} (client_uuid={client_uuid}): {e}"))?;

    tx.commit()
        .map_err(|e| format!("COMMIT ({TYPE_INVOICE}): {e}"))?;

    Ok(EnqueuedTransaction {
        id,
        client_uuid,
        outbox_id,
    })
}

/// Касова операція каси (внесення/інкасація) — ADR-0007 §11.6.
///
/// Свідомо ТОНКИЙ wrapper над [`enqueue_transaction`]: агрегат `cash_ledger`
/// (0006) + outbox-запис (pending) + касовий ефект (`cash::apply_cash_delta`)
/// в ОДНІЙ SQLite-транзакції — тобто та сама примітивна функція, яку
/// використовує `/api/v1/local/ops` і `OutboxPos` (одна реалізація запису,
/// другої не заводимо — §11.5). Окремих колонок (як `invoices.supplier_id`)
/// не потрібно: реквізити читаються з `data`/payload.
pub fn enqueue_cash_operation(
    conn: &mut Connection,
    payload_json: &str,
    store_id: &str,
) -> Result<EnqueuedTransaction, String> {
    enqueue_transaction(conn, TYPE_CASH_OPERATION, payload_json, store_id)
}

/// Підмітає в outbox агрегати synced=0, накопичені СТАРОЮ версією коду
/// (ЕТАП 6: запис без outbox). Ідемпотентно: INSERT OR IGNORE за client_uuid
/// (outbox.client_uuid UNIQUE) + позначка synced=1. Викликається на початку
/// push-циклу (push_pending_batch) — «при першому sync після оновлення всі
/// не-чекові операції потрапляють в outbox».
pub fn sweep_legacy_unsynced(conn: &mut Connection) -> Result<usize, String> {
    let mut swept = 0usize;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE (sweep): {e}"))?;
    for kind in [
        TYPE_PURCHASE_ORDER,
        TYPE_INVENTORY,
        TYPE_TRANSFER,
        TYPE_WRITE_OFF,
        TYPE_INVOICE,
    ] {
        let table = table_of(kind)?;
        let rows: Vec<(String, String, Option<String>)> = {
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT client_uuid, data, store_id FROM {table} WHERE synced = 0"
                ))
                .map_err(|e| format!("sweep SELECT {table}: {e}"))?;
            let it = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(|e| format!("sweep query {table}: {e}"))?;
            let mut v = Vec::new();
            for row in it {
                v.push(row.map_err(|e| format!("sweep row {table}: {e}"))?);
            }
            v
        };
        for (client_uuid, data, sid) in rows {
            let store_id = sid.unwrap_or_default();
            let payload: Value =
                serde_json::from_str(&data).map_err(|e| format!("sweep {table} data JSON: {e}"))?;
            let envelope = serde_json::json!({
                "type": kind,
                "client_uuid": client_uuid,
                "store_id": store_id,
                "created_at": chrono::Utc::now().to_rfc3339(),
                "payload": payload,
            })
            .to_string();
            tx.execute(
                "INSERT OR IGNORE INTO outbox (type, client_uuid, payload, status) \
                 VALUES (?1, ?2, ?3, 'pending')",
                params![kind, client_uuid, envelope],
            )
            .map_err(|e| format!("sweep INSERT outbox ({table} {client_uuid}): {e}"))?;
            tx.execute(
                &format!("UPDATE {table} SET synced = 1 WHERE client_uuid = ?1"),
                params![client_uuid],
            )
            .map_err(|e| format!("sweep UPDATE {table}: {e}"))?;
            swept += 1;
        }
    }
    tx.commit().map_err(|e| format!("COMMIT (sweep): {e}"))?;
    Ok(swept)
}

/// Отримати агрегат за client_uuid (data JSON + статус) — для тестів і
/// майбутнього push. Повертає (data, synced, store_id).
pub fn get_transaction(
    conn: &Connection,
    kind: &str,
    client_uuid: &str,
) -> Result<(String, i64, Option<String>), String> {
    let table = table_of(kind)?;
    conn.query_row(
        &format!("SELECT data, synced, store_id FROM {table} WHERE client_uuid = ?1"),
        params![client_uuid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .map_err(|e| format!("SELECT {table} ({client_uuid}): {e}"))
}

/// Агрегати, що очікують синхронізації (synced=0) — кандидати ЕТАП 7.
pub fn unsynced_count(conn: &Connection, kind: &str) -> Result<i64, String> {
    let table = table_of(kind)?;
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE synced = 0"),
        [],
        |r| r.get(0),
    )
    .map_err(|e| format!("COUNT {table} (unsynced): {e}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Робочі сесії користувача (ADR-0007 §3.4 #38–#40, клас LOCAL_SQLITE)
// ─────────────────────────────────────────────────────────────────────────────
// Standby-вузол не може писати `work_sessions` у PG (локальна репліка
// read-only), тому сесія фіксується ЛОКАЛЬНО в SQLite вузла (міграція 0009;
// таблиця дзеркалить PG public.work_sessions) + outbox-опу `work_session` —
// доставити на primary має ОКРЕМИЙ приймач (інший контракт).
// Формат payload — конверт дизайну 2.2, ідентичний агрегатам і чекам.

/// Результат локального відкриття сесії (SQLite + outbox).
#[derive(Debug, Clone, PartialEq)]
pub struct LocalWorkSession {
    /// UUIDv4 сесії — той самий у `work_sessions` і outbox
    /// (ідемпотентний ключ push, як у чеків/агрегатів).
    pub client_uuid: String,
    /// Час входу (RFC 3339, UTC) — дзеркало PG `work_sessions.login_time`.
    pub login_time: String,
    /// rowid outbox-запису (pending): сесія одразу push-кандидат.
    pub outbox_id: i64,
}

/// Payload сесії — те, що лежить у `work_sessions.data` і в `payload` конверта.
fn work_session_payload(
    user_id: &str,
    store_id: Option<&str>,
    login_time: &str,
    logout_time: Option<&str>,
    duration_hours: Option<f64>,
) -> Value {
    serde_json::json!({
        "user_id": user_id,
        "store_id": store_id,
        "login_time": login_time,
        "logout_time": logout_time,
        "duration_hours": duration_hours,
    })
}

/// Конверт push (дизайн 2.2) — той самий формат, що в агрегатів і чеків.
fn work_session_envelope(
    client_uuid: &str,
    store_id: Option<&str>,
    created_at: &str,
    payload: &Value,
) -> Value {
    serde_json::json!({
        "type": TYPE_WORK_SESSION,
        "client_uuid": client_uuid,
        "store_id": store_id,
        "created_at": created_at,
        "payload": payload,
    })
}

/// Тривалість сесії в годинах — ТА САМА формула, що в PG-гілці
/// (`repositories/auth.rs`): мілісекунди / 3_600_000, округлення до 2 знаків.
fn session_duration_hours(login_time: &str, logout_time: &str) -> Result<f64, String> {
    let start = chrono::DateTime::parse_from_rfc3339(login_time)
        .map_err(|e| format!("work_session login_time «{login_time}» не RFC 3339: {e}"))?;
    let end = chrono::DateTime::parse_from_rfc3339(logout_time)
        .map_err(|e| format!("work_session logout_time «{logout_time}» не RFC 3339: {e}"))?;
    let hours = (end - start).num_milliseconds() as f64 / 3_600_000.0;
    Ok((hours * 100.0).round() / 100.0)
}

/// Локальний вхід: INSERT сесії (synced=1) + outbox-оп — ОДНА SQLite-
/// транзакція (BEGIN IMMEDIATE → INSERT work_sessions → INSERT outbox →
/// COMMIT); помилка будь-де → ROLLBACK (жодної сесії без outbox-запису).
///
/// Stock не чіпається (сесія — не операція каси).
pub fn open_work_session(
    conn: &mut Connection,
    user_id: &str,
    store_id: Option<&str>,
) -> Result<LocalWorkSession, String> {
    let client_uuid = Uuid::new_v4().to_string();
    let login_time = chrono::Utc::now().to_rfc3339();
    let payload = work_session_payload(user_id, store_id, &login_time, None, None);
    let data = payload.to_string();
    let envelope = work_session_envelope(&client_uuid, store_id, &login_time, &payload).to_string();

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE (work_session open): {e}"))?;

    tx.execute(
        "INSERT INTO work_sessions \
         (client_uuid, user_id, store_id, login_time, logout_time, duration_hours, data, synced) \
         VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, 1)",
        params![client_uuid, user_id, store_id, login_time, data],
    )
    .map_err(|e| format!("INSERT work_sessions (client_uuid={client_uuid}): {e}"))?;

    tx.execute(
        "INSERT INTO outbox (type, client_uuid, payload, status) \
         VALUES (?1, ?2, ?3, 'pending')",
        params![TYPE_WORK_SESSION, client_uuid, envelope],
    )
    .map_err(|e| format!("INSERT outbox ({TYPE_WORK_SESSION}, client_uuid={client_uuid}): {e}"))?;
    let outbox_id = tx.last_insert_rowid();

    tx.commit()
        .map_err(|e| format!("COMMIT (work_session open, client_uuid={client_uuid}): {e}"))?;

    Ok(LocalWorkSession {
        client_uuid,
        login_time,
        outbox_id,
    })
}

/// Локальний вихід: заповнює `logout_time`/`duration_hours` відкритим сесіям
/// користувача і UPSERT-ить фінальний конверт у ТОЙ САМИЙ outbox-запис
/// (за client_uuid) — усе в одній SQLite-транзакції.
///
/// `latest_only = true` — лише найновіша відкрита сесія (як `logout` у PG:
/// `ORDER BY login_time DESC LIMIT 1`); `false` — усі відкриті (як
/// `close_active_work_sessions`).
///
/// Повертає тривалості закритих сесій (години, 2 знаки). Порожній вектор —
/// відкритих сесій не було (не помилка).
pub fn close_work_session(
    conn: &mut Connection,
    user_id: &str,
    latest_only: bool,
) -> Result<Vec<f64>, String> {
    let logout_time = chrono::Utc::now().to_rfc3339();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE (work_session close): {e}"))?;

    // 1. Відкриті сесії користувача (найновіша першою). Statement живе лише
    //    в блоці, щоб далі позичати `tx` на execute.
    let open: Vec<(String, String, Option<String>, String)> = {
        let sql = if latest_only {
            "SELECT client_uuid, login_time, store_id, data FROM work_sessions \
             WHERE user_id = ?1 AND logout_time IS NULL ORDER BY login_time DESC LIMIT 1"
        } else {
            "SELECT client_uuid, login_time, store_id, data FROM work_sessions \
             WHERE user_id = ?1 AND logout_time IS NULL ORDER BY login_time DESC"
        };
        let mut stmt = tx
            .prepare(sql)
            .map_err(|e| format!("prepare SELECT work_sessions: {e}"))?;
        let it = stmt
            .query_map(params![user_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(|e| format!("SELECT work_sessions (user_id={user_id}): {e}"))?;
        let mut v = Vec::new();
        for row in it {
            v.push(row.map_err(|e| format!("рядок work_sessions: {e}"))?);
        }
        v
    };

    let mut durations = Vec::with_capacity(open.len());
    for (client_uuid, login_time, store_id, data) in &open {
        let duration = session_duration_hours(login_time, &logout_time)?;

        // payload: оновлюємо наявний data (невалідний/не-об'єкт → будуємо заново).
        let mut payload = match serde_json::from_str::<Value>(data) {
            Ok(v) if v.is_object() => v,
            _ => work_session_payload(user_id, store_id.as_deref(), login_time, None, None),
        };
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("logout_time".to_string(), serde_json::json!(logout_time));
            obj.insert("duration_hours".to_string(), serde_json::json!(duration));
        }
        let data_updated = payload.to_string();
        let envelope =
            work_session_envelope(client_uuid, store_id.as_deref(), login_time, &payload)
                .to_string();

        // 2. Той самий outbox-оп (client_uuid) → фінальний конверт, знову
        //    pending (сесію закрито — дані для push змінились).
        tx.execute(
            "INSERT INTO outbox (type, client_uuid, payload, status) \
             VALUES (?1, ?2, ?3, 'pending') \
             ON CONFLICT(client_uuid) DO UPDATE SET \
                payload = excluded.payload, status = 'pending', attempts = 0, \
                next_attempt_at = datetime('now'), last_error = NULL",
            params![TYPE_WORK_SESSION, client_uuid, envelope],
        )
        .map_err(|e| {
            format!("UPSERT outbox ({TYPE_WORK_SESSION}, client_uuid={client_uuid}): {e}")
        })?;

        // 3. Локальні колонки сесії (дзеркало PG: logout_time + duration_hours).
        tx.execute(
            "UPDATE work_sessions SET logout_time = ?1, duration_hours = ?2, data = ?3, synced = 1 \
             WHERE client_uuid = ?4",
            params![logout_time, duration, data_updated, client_uuid],
        )
        .map_err(|e| format!("UPDATE work_sessions (client_uuid={client_uuid}): {e}"))?;

        durations.push(duration);
    }

    tx.commit()
        .map_err(|e| format!("COMMIT (work_session close, user_id={user_id}): {e}"))?;

    Ok(durations)
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const STORE: &str = "d9be9608-c011-49be-b776-3317ca5e9af6";

    fn migrated_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("FK");
        super::super::migrations::migrate(&conn).expect("міграції");
        conn
    }

    fn level(conn: &Connection, product: &str) -> i64 {
        stock::get_stock_level(conn, STORE, product).expect("level")
    }

    /// Закупка (ЕТАП 7b): агрегат (synced=1) + outbox-запис + stock +qty.
    #[test]
    fn purchase_enqueues_aggregate_outbox_and_adds_stock() {
        let conn = migrated_conn();
        let mut c = conn;
        let payload = json!({
            "supplier_id": "sup-1",
            "items": [
                {"product_id": "p1", "quantity": 10, "price": "20.00"},
                {"product_id": "p2", "quantity": "2.500", "price": "5.00"},
            ],
            "total_amount": "212.50",
        })
        .to_string();

        let out =
            enqueue_transaction(&mut c, TYPE_PURCHASE_ORDER, &payload, STORE).expect("закупка");
        assert_eq!(level(&c, "p1"), 10_000, "+10 шт");
        assert_eq!(level(&c, "p2"), 2500, "+2.5 шт");

        let (data, synced, sid) =
            get_transaction(&c, TYPE_PURCHASE_ORDER, &out.client_uuid).expect("читання");
        assert_eq!(
            synced, 1,
            "агрегат передано в outbox (не synced=0 «в нікуди»)"
        );
        assert_eq!(sid.as_deref(), Some(STORE));
        let v: Value = serde_json::from_str(&data).expect("data JSON");
        assert_eq!(v["supplier_id"], "sup-1");
        assert_eq!(unsynced_count(&c, TYPE_PURCHASE_ORDER).unwrap(), 0);
        // КРИТЕРІЙ ЕТАП 7b: outbox-запис існує з тим самим client_uuid.
        let (typ, cu, status): (String, String, String) = c
            .query_row(
                "SELECT type, client_uuid, status FROM outbox WHERE id = ?1",
                params![out.outbox_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("outbox");
        assert_eq!(typ, TYPE_PURCHASE_ORDER);
        assert_eq!(cu, out.client_uuid);
        assert_eq!(status, "pending");
    }

    /// Інвентаризація: АБСОЛЮТНИЙ рівень (факт перерахунку), не дельта.
    #[test]
    fn inventory_sets_absolute_level() {
        let conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 5000).expect("до: 5 шт");
        let payload = json!({
            "items": [{"product_id": "p1", "fact_quantity": 7}],
        })
        .to_string();
        let out =
            enqueue_transaction(&mut c, TYPE_INVENTORY, &payload, STORE).expect("інвентаризація");
        assert_eq!(level(&c, "p1"), 7000, "факт 7 шт (не 5+7)");
        assert_eq!(unsynced_count(&c, TYPE_INVENTORY).unwrap(), 0);
        assert!(out.outbox_id > 0, "інвентаризація теж в outbox (ЕТАП 7b)");
    }

    /// Списання: stock −qty.
    #[test]
    fn write_off_subtracts_stock() {
        let conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 10_000).expect("до: 10 шт");
        let payload = json!({
            "reason": "псування",
            "items": [{"product_id": "p1", "quantity": 3}],
        })
        .to_string();
        let out = enqueue_transaction(&mut c, TYPE_WRITE_OFF, &payload, STORE).expect("списання");
        assert_eq!(level(&c, "p1"), 7000, "10 − 3 = 7 шт");
        assert!(out.outbox_id > 0);
    }

    /// Переміщення З каси (from=store): −qty.
    #[test]
    fn transfer_out_subtracts_stock() {
        let conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 10_000).expect("до: 10 шт");
        let payload = json!({
            "from_store_id": STORE,
            "to_store_id": "11111111-1111-1111-1111-111111111111",
            "items": [{"product_id": "p1", "quantity": 4}],
        })
        .to_string();
        enqueue_transaction(&mut c, TYPE_TRANSFER, &payload, STORE).expect("transfer out");
        assert_eq!(level(&c, "p1"), 6000, "10 − 4 = 6 шт");
    }

    /// Переміщення НА касу (to=store): +qty.
    #[test]
    fn transfer_in_adds_stock() {
        let conn = migrated_conn();
        let mut c = conn;
        let payload = json!({
            "from_store_id": "22222222-2222-2222-2222-222222222222",
            "to_store_id": STORE,
            "items": [{"product_id": "p1", "quantity": 8}],
        })
        .to_string();
        enqueue_transaction(&mut c, TYPE_TRANSFER, &payload, STORE).expect("transfer in");
        assert_eq!(level(&c, "p1"), 8000, "+8 шт (прийом)");
    }

    /// Переміщення між чужими точками: агрегат збережено, stock не змінено.
    #[test]
    fn transfer_foreign_stores_keeps_aggregate_only() {
        let conn = migrated_conn();
        let mut c = conn;
        let payload = json!({
            "from_store_id": "33333333-3333-3333-3333-333333333333",
            "to_store_id": "44444444-4444-4444-4444-444444444444",
            "items": [{"product_id": "p1", "quantity": 2}],
        })
        .to_string();
        let out = enqueue_transaction(&mut c, TYPE_TRANSFER, &payload, STORE).expect("чуже");
        assert_eq!(level(&c, "p1"), 0, "каса не сторона — stock без змін");
        assert!(get_transaction(&c, TYPE_TRANSFER, &out.client_uuid).is_ok());
    }

    /// Збій mid-транзакції (невалідний JSON) → ROLLBACK: ні агрегата, ні stock.
    #[test]
    fn mid_tx_failure_rolls_back_aggregate_and_stock() {
        let conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 500).expect("до");

        let res = enqueue_transaction(&mut c, TYPE_PURCHASE_ORDER, "{не-json", STORE);
        assert!(res.is_err(), "невалідний payload → помилка");
        assert_eq!(
            unsynced_count(&c, TYPE_PURCHASE_ORDER).unwrap(),
            0,
            "агрегата немає"
        );
        let ob: i64 = c
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))
            .expect("outbox count");
        assert_eq!(ob, 0, "outbox-запису немає (ROLLBACK)");
        assert_eq!(level(&c, "p1"), 500, "stock без змін (ROLLBACK)");

        // work_session — тепер підтримуваний outbox-тип (окремий шлях сесій,
        // ADR-0007 #38–40); через enqueue_transaction він НЕ йде (немає
        // таблиці агрегата каси), але для outbox є «своїм».
        assert!(is_supported_outbox_type(TYPE_WORK_SESSION));
        // реально невідомий тип — як і раніше помилка ДО транзакції.
        assert!(enqueue_transaction(&mut c, "totally_unknown_kind", "{}", STORE).is_err());
    }

    // ── Прибуткова накладна (invoice, ADR-0007 §3.4 LOCAL_SQLITE) ──────────

    /// payload накладної у форматі фронта (/v2) — як його бере enqueue_invoice.
    fn invoice_payload(supplier: &str, product: &str, qty: &str, total: &str) -> String {
        json!({
            "number": "INV-LOCAL-1",
            "supplier_id": supplier,
            "invoice_date": "2026-08-30T12:00:00+03:00",
            "payment_method": null,
            "is_fiscal": false,
            "notes": "локальна прибуткова (тест)",
            "total_amount": total,
            "items": [{
                "product_id": product,
                "quantity": qty,
                "price": "100.00",
                "total": total,
            }],
        })
        .to_string()
    }

    /// Критерій A: накладна пише агрегат (synced=1) + деталізацію + рівно один
    /// outbox-оп (pending, той самий uuid) + stock +qty по позиціях.
    #[test]
    fn invoice_enqueues_aggregate_outbox_and_adds_stock() {
        let conn = migrated_conn();
        let mut c = conn;
        let payload = invoice_payload("sup-1", "p1", "3", "300.00");
        let out = enqueue_invoice(&mut c, &payload, STORE).expect("накладна");

        // 1. Агрегат: data = payload, synced = 1, реквізити в окремих колонках.
        let (data, synced, sid, sup, num): (String, i64, String, String, String) = c
            .query_row(
                "SELECT data, synced, store_id, supplier_id, number FROM invoices \
                 WHERE client_uuid = ?1",
                params![out.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("агрегат накладної");
        assert!(data.contains("INV-LOCAL-1"), "data = payload як є");
        assert_eq!(synced, 1, "агрегат одразу push-кандидат");
        assert_eq!(sid, STORE);
        assert_eq!(sup, "sup-1");
        assert_eq!(num, "INV-LOCAL-1");
        assert_eq!(
            unsynced_count(&c, TYPE_INVOICE).unwrap(),
            0,
            "synced=0 немає"
        );

        // 2. Деталізація: 1 позиція, 3 шт = 3000 міліодиниць (scale 3).
        // sum/price — колонки NUMERIC (як receipt_items, 0006): SQLite
        // застосовує NUMERIC-афінність, тому читаємо як число.
        let (qty, sum): (i64, f64) = c
            .query_row(
                "SELECT quantity, sum FROM invoice_items WHERE invoice_client_uuid = ?1",
                params![out.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("позиція накладної");
        assert_eq!(qty, 3000, "3 шт у міліодиницях");
        assert!((sum - 300.0).abs() < 0.001, "sum = 300.00, маємо {sum}");

        // 3. Outbox: type='invoice', pending, той самий client_uuid.
        let (typ, cu, status): (String, String, String) = c
            .query_row(
                "SELECT type, client_uuid, status FROM outbox WHERE id = ?1",
                params![out.outbox_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("outbox");
        assert_eq!(typ, TYPE_INVOICE);
        assert_eq!(cu, out.client_uuid);
        assert_eq!(status, "pending");
        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM outbox WHERE type = 'invoice'"),
            1,
            "рівно один outbox-оп"
        );

        // 4. Stock: прибуткова = +qty.
        assert_eq!(level(&c, "p1"), 3000, "+3 шт локально");
        assert!(is_supported_outbox_type(TYPE_INVOICE));
    }

    /// Критерій A (rollback): невдала накладна НЕ лишає ні агрегата, ні
    /// позицій, ні outbox-запису, ні stock-ефекту.
    #[test]
    fn invoice_mid_tx_failure_rolls_back_everything() {
        let conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 500).expect("до: 0.5 шт");

        // (а) битий JSON — помилка ДО транзакції.
        assert!(enqueue_invoice(&mut c, "{не-json", STORE).is_err());
        assert_eq!(count(&c, "SELECT COUNT(*) FROM invoices"), 0);
        assert_eq!(count(&c, "SELECT COUNT(*) FROM outbox"), 0);
        assert_eq!(level(&c, "p1"), 500, "stock без змін");

        // (б) помилка ВСЕРЕДИНІ транзакції (агрегат уже вставлено, далі —
        //     деталізація падає: таблиці немає) → усе відкочується.
        c.execute_batch("DROP TABLE invoice_items;").expect("drop");
        let payload = invoice_payload("sup-1", "p1", "3", "300.00");
        let res = enqueue_invoice(&mut c, &payload, STORE);
        assert!(res.is_err(), "помилка деталізації → Err");
        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM invoices"),
            0,
            "агрегат відкочено (ROLLBACK)"
        );
        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM outbox WHERE type = 'invoice'"),
            0,
            "outbox-запису немає (ROLLBACK)"
        );
        assert_eq!(level(&c, "p1"), 500, "stock без змін (ROLLBACK)");
    }

    /// ЕТАП 7b: накопичені synced=0 (стара версія) → outbox при першому sync.
    #[test]
    fn sweep_moves_legacy_unsynced_into_outbox() {
        let conn = migrated_conn();
        let mut c = conn;
        // Симулюємо СТАРУ версію: агрегат synced=0 без outbox.
        let po = json!({"items": [{"product_id": "p1", "quantity": 5}]}).to_string();
        c.execute(
            "INSERT INTO purchase_orders (client_uuid, store_id, data, synced) \
             VALUES ('11111111-1111-1111-1111-111111111111', ?1, ?2, 0)",
            params![STORE, po],
        )
        .expect("legacy row");
        let wo = json!({"reason": "x", "items": [{"product_id": "p1", "quantity": 1}]}).to_string();
        c.execute(
            "INSERT INTO write_offs (client_uuid, store_id, data, synced) \
             VALUES ('22222222-2222-2222-2222-222222222222', ?1, ?2, 0)",
            params![STORE, wo],
        )
        .expect("legacy row 2");

        let n = sweep_legacy_unsynced(&mut c).expect("sweep");
        assert_eq!(n, 2, "обидва легасі-агрегати підмітено");
        assert_eq!(
            unsynced_count(&c, TYPE_PURCHASE_ORDER).unwrap(),
            0,
            "synced=1"
        );
        assert_eq!(unsynced_count(&c, TYPE_WRITE_OFF).unwrap(), 0);
        // Outbox має обидва записи (type, client_uuid).
        let ob: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM outbox WHERE status = 'pending'",
                [],
                |r| r.get(0),
            )
            .expect("outbox");
        assert_eq!(ob, 2);
        let (typ, cu): (String, String) = c
            .query_row(
                "SELECT type, client_uuid FROM outbox WHERE client_uuid = ?1",
                params!["11111111-1111-1111-1111-111111111111"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("outbox row");
        assert_eq!(typ, TYPE_PURCHASE_ORDER);
        assert_eq!(cu, "11111111-1111-1111-1111-111111111111");
        // Ідемпотентність повторного sweep.
        let n2 = sweep_legacy_unsynced(&mut c).expect("sweep 2");
        assert_eq!(n2, 0, "повторний sweep нічого не робить");
    }

    /// Кожен тип має власну таблицю: запис+читання (критерій тестів ЕТАПУ 6).
    #[test]
    fn each_kind_reads_back_from_own_table() {
        let conn = migrated_conn();
        let mut c = conn;
        for (kind, table) in [
            (TYPE_PURCHASE_ORDER, "purchase_orders"),
            (TYPE_INVENTORY, "inventories"),
            (TYPE_TRANSFER, "transfers"),
            (TYPE_WRITE_OFF, "write_offs"),
        ] {
            let payload = json!({"tag": kind, "items": []}).to_string();
            let out = enqueue_transaction(&mut c, kind, &payload, STORE).expect(kind);
            let n: i64 = c
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE client_uuid = ?1"),
                    params![out.client_uuid],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{table}: агрегат записано");
            let (data, _, _) = get_transaction(&c, kind, &out.client_uuid).expect("читання");
            assert!(data.contains(kind), "{table}: data повертається");
        }
    }

    // ── ADR-0007 §3.4 #38–#40 (LOCAL_SQLITE): сесії на standby ─────────────

    /// Файлова SQLite (критерій 6) — production-шлях відкриття (sync_push).
    fn file_db(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let path = dir.path().join("offline.db");
        super::super::sync_push::open_connection(&path).expect("open_connection");
        path
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect("COUNT")
    }

    /// Критерій 3: вхід пише рядок сесії + рівно один outbox-оп (pending, той
    /// самий uuid), synced=1 — сесія одразу push-кандидат.
    #[test]
    fn work_session_login_writes_sqlite_row_and_outbox_op() {
        let conn = migrated_conn();
        let mut c = conn;
        let s = open_work_session(&mut c, "user-1", Some(STORE)).expect("login");

        let login_time: String = c
            .query_row(
                "SELECT login_time FROM work_sessions WHERE client_uuid = ?1",
                params![s.client_uuid],
                |r| r.get(0),
            )
            .expect("рядок work_sessions");
        assert!(!login_time.is_empty(), "login_time записано локально");
        assert_eq!(login_time, s.login_time);

        let (typ, status, cu, ob_id): (String, String, String, i64) = c
            .query_row(
                "SELECT type, status, client_uuid, id FROM outbox WHERE client_uuid = ?1",
                params![s.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("outbox-оп");
        assert_eq!(typ, TYPE_WORK_SESSION);
        assert_eq!(status, "pending");
        assert_eq!(cu, s.client_uuid, "той самий uuid в агрегаті й outbox");
        assert_eq!(ob_id, s.outbox_id, "outbox_id = rowid outbox-запису");

        let synced: i64 = c
            .query_row(
                "SELECT synced FROM work_sessions WHERE client_uuid = ?1",
                params![s.client_uuid],
                |r| r.get(0),
            )
            .expect("synced");
        assert_eq!(synced, 1, "сесію передано в outbox (не «в нікуди»)");
        assert_eq!(count(&c, "SELECT COUNT(*) FROM outbox"), 1, "рівно один оп");
    }

    /// Критерій 4: вихід локально заповнює logout_time + duration_hours і
    /// оновлює ТОЙ САМИЙ outbox-оп (UPSERT за client_uuid) фінальним конвертом.
    #[test]
    fn work_session_logout_fills_logout_time_and_duration_locally() {
        let conn = migrated_conn();
        let mut c = conn;
        let s = open_work_session(&mut c, "user-1", Some(STORE)).expect("login");

        let durations = close_work_session(&mut c, "user-1", false).expect("logout");
        assert_eq!(durations.len(), 1, "закрито одну відкриту сесію");
        assert!(durations[0] >= 0.0, "тривалість невід'ємна");

        let (lo, dur): (Option<String>, Option<f64>) = c
            .query_row(
                "SELECT logout_time, duration_hours FROM work_sessions WHERE client_uuid = ?1",
                params![s.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("рядок сесії");
        assert!(lo.is_some(), "logout_time заповнено локально");
        let dur = dur.expect("duration_hours заповнено локально");
        assert!(dur >= 0.0);
        assert_eq!(
            (dur * 100.0).round() / 100.0,
            dur,
            "округлення до 2 знаків (та сама формула, що в PG-гілці)"
        );
        assert_eq!(durations[0], dur);

        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM outbox"),
            1,
            "оп не дубльовано (UPSERT)"
        );
        let (status, payload): (String, String) = c
            .query_row(
                "SELECT status, payload FROM outbox WHERE client_uuid = ?1",
                params![s.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("outbox-оп");
        assert_eq!(status, "pending", "фінальний конверт знову чекає push");
        let v: Value = serde_json::from_str(&payload).expect("конверт JSON");
        assert_eq!(v["type"], TYPE_WORK_SESSION);
        assert!(
            v["payload"]["logout_time"].is_string(),
            "logout_time у payload"
        );
        assert!(
            v["payload"]["duration_hours"].is_number(),
            "duration_hours у payload"
        );
    }

    /// Критерій 6: сесія НЕ втрачається — COMMIT на диску переживає reopen.
    #[test]
    fn work_session_survives_reopen_not_lost() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = file_db(&dir);
        {
            let mut c = super::super::sync_push::open_connection(&path).expect("conn1");
            open_work_session(&mut c, "user-1", Some(STORE)).expect("login");
        } // drop → закриття з'єднання (дані мусять бути закомічені)

        let c2 = super::super::sync_push::open_connection(&path).expect("reopen");
        assert_eq!(
            count(&c2, "SELECT COUNT(*) FROM work_sessions"),
            1,
            "сесія на диску після reopen"
        );
        assert_eq!(
            count(
                &c2,
                "SELECT COUNT(*) FROM outbox WHERE type = 'work_session'"
            ),
            1,
            "outbox-оп на диску після reopen"
        );
        assert_eq!(
            count(
                &c2,
                "SELECT COUNT(*) FROM work_sessions WHERE logout_time IS NULL"
            ),
            1,
            "сесія лишається відкритою (не «закрита» drop'ом)"
        );
    }

    /// `latest_only`: закривається рівно одна (найновіша) сесія; повторний
    /// виклик з `false` закриває решту.
    #[test]
    fn close_work_session_latest_only_closes_one() {
        let conn = migrated_conn();
        let mut c = conn;
        open_work_session(&mut c, "user-1", Some(STORE)).expect("login 1");
        open_work_session(&mut c, "user-1", Some(STORE)).expect("login 2");

        let one = close_work_session(&mut c, "user-1", true).expect("logout latest");
        assert_eq!(one.len(), 1, "latest_only закриває рівно одну");
        assert_eq!(
            count(
                &c,
                "SELECT COUNT(*) FROM work_sessions WHERE logout_time IS NULL"
            ),
            1,
            "одна сесія лишилась відкритою"
        );
        assert_eq!(count(&c, "SELECT COUNT(*) FROM outbox"), 2, "два опи сесій");

        let rest = close_work_session(&mut c, "user-1", false).expect("close all");
        assert_eq!(rest.len(), 1, "закрито решту");
        assert_eq!(
            count(
                &c,
                "SELECT COUNT(*) FROM work_sessions WHERE logout_time IS NULL"
            ),
            0,
            "відкритих сесій немає"
        );
    }

    /// Сесія — не операція каси: stock не змінюється ні на вході, ні на виході.
    #[test]
    fn work_session_has_no_stock_effect() {
        let conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 10_000).expect("до: 10 шт");

        open_work_session(&mut c, "user-1", Some(STORE)).expect("login");
        close_work_session(&mut c, "user-1", false).expect("logout");

        assert_eq!(level(&c, "p1"), 10_000, "сесія не має stock-ефекту");
        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM stock"),
            1,
            "stock-таблиця не торкнута (лише передвстановлений рядок)"
        );
    }

    /// Білий список outbox-типів: підтримувані — 4 агрегати + сесії + чеки.
    #[test]
    fn supported_outbox_types_are_whitelisted() {
        for kind in [
            TYPE_PURCHASE_ORDER,
            TYPE_INVENTORY,
            TYPE_TRANSFER,
            TYPE_WRITE_OFF,
            TYPE_WORK_SESSION,
            TYPE_INVOICE,
            TYPE_CASH_OPERATION,
            super::super::sync_push::TYPE_RECEIPT,
            super::super::sync_push::TYPE_RETURN_RECEIPT,
        ] {
            assert!(is_supported_outbox_type(kind), "{kind} мусить бути відомий");
        }
        assert!(!is_supported_outbox_type("totally_unknown_kind"));
    }

    /// Касова операція: агрегат `cash_ledger` (synced=1) + outbox(pending) +
    /// касовий ефект на баланс — усе в одній транзакції (ADR-0007 §11.6).
    #[test]
    fn cash_operation_enqueues_aggregate_outbox_and_moves_balance() {
        let mut conn = migrated_conn();
        let deposit = json!({
            "operation_type": "deposit",
            "cash_type": "cash",
            "amount": "150.00",
            "comment": "внесення (e2e)",
        })
        .to_string();
        let out = enqueue_cash_operation(&mut conn, &deposit, STORE).expect("deposit");
        assert_eq!(cash::get_cash_balance(&conn, STORE, "cash").expect("balance"), 15_000);
        // Агрегат — у НАЯВНІЙ таблиці 0006 cash_ledger, synced=1 (push-кандидат).
        let (data, synced, table): (String, i64, String) = conn
            .query_row(
                "SELECT l.data, l.synced, 'cash_ledger' FROM cash_ledger l WHERE l.client_uuid = ?1",
                rusqlite::params![out.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("агрегат cash_ledger");
        assert_eq!(table, "cash_ledger");
        assert_eq!(synced, 1, "агрегат каси — push-кандидат");
        assert!(data.contains("deposit"), "data = payload як є: {data}");
        // Рівно один outbox-запис із типом cash_operation.
        let (otype, status): (String, String) = conn
            .query_row(
                "SELECT type, status FROM outbox WHERE client_uuid = ?1",
                rusqlite::params![out.client_uuid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("outbox");
        assert_eq!(otype, TYPE_CASH_OPERATION);
        assert_eq!(status, "pending");

        // Інкасація — мінус; окремий ящик card не змішується.
        let collection = json!({
            "operation_type": "collection",
            "cash_type": "cash",
            "amount": "50.00",
        })
        .to_string();
        enqueue_cash_operation(&mut conn, &collection, STORE).expect("collection");
        let card = json!({
            "operation_type": "deposit",
            "cash_type": "card",
            "amount": "10.00",
        })
        .to_string();
        enqueue_cash_operation(&mut conn, &card, STORE).expect("card");
        assert_eq!(cash::get_cash_balance(&conn, STORE, "cash").expect("cash"), 10_000);
        assert_eq!(cash::get_cash_balance(&conn, STORE, "card").expect("card"), 1_000);
        // 3 агрегати, 3 op — нічого не загубилось і не подвоїлось.
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM cash_ledger", [], |r| r.get(0))
            .expect("count");
        assert_eq!(n, 3);
    }

    /// Невідомий `operation_type` → відмова й ROLLBACK: ні агрегата, ні
    /// outbox-запису, ні касового ефекту (межа «ефект і документ — разом»).
    #[test]
    fn cash_operation_unknown_type_rolls_back_everything() {
        let mut conn = migrated_conn();
        let bad = json!({"operation_type": "withdrawal", "cash_type": "cash", "amount": "10.00"})
            .to_string();
        let err = enqueue_cash_operation(&mut conn, &bad, STORE).expect_err("мусить відмовити");
        assert!(err.contains("невідомий operation_type"), "{err}");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM cash_ledger", [], |r| r.get(0))
            .expect("count");
        let ops: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outbox WHERE type = ?1",
                rusqlite::params![TYPE_CASH_OPERATION],
                |r| r.get(0),
            )
            .expect("count ops");
        assert_eq!(n, 0, "агрегата немає");
        assert_eq!(ops, 0, "outbox-запису немає");
        assert_eq!(cash::get_cash_balance(&conn, STORE, "cash").expect("balance"), 0);
    }
}
