//! Локальні транзакції каси поза продажем (ЕТАП 6 offline-first):
//! закупки, інвентаризації, переміщення, списання.
//!
//! Самодостатність (критерій ЕТАПУ 6): кожна операція записується ЛОКАЛЬНО
//! (таблиці міграції 0006) зі stock-ефектом АТОМАРНО (дизайн 4.4) —
//! працює з вимкненим сервером.
//!
//! Чому БЕЗ outbox (розбіжність з текстом завдання «агрегат+ефекти+outbox»):
//! серверний Rust-фасад POST /api/v1/sync/push (ЕТАП 4) приймає ЛИШЕ
//! receipt/return_receipt (перевірено e2e: інші типи → per-item error →
//! outbox failed). Outbox-запис для purchase/inventory/transfer/write_off
//! дав би ГАРАНТОВАНИЙ failed-стан без жодного шансу доставки. Тому агрегати
//! пишуться у власні таблиці з synced=0 + client_uuid (ідемпотентність
//! готова); ЕТАП 7 — розширення фасаду сервера і переведення цих агрегатів
//! в outbox (міграція даних synced=0 → outbox).
//!
//! Формат payload — як його формує фронт для /v2-ендпоінтів сервера
//! (зберігається в data цілком); Rust читає лише items[].product_id/quantity
//! для stock-ефекту та store_id.

use rusqlite::{params, Connection};
use serde_json::Value;
use uuid::Uuid;

use super::stock;

/// Тип агрегата «закупка» (дизайн 2.2; майбутній outbox-тип ЕТАП 7).
pub const TYPE_PURCHASE_ORDER: &str = "purchase_order";
/// Тип агрегата «інвентаризація».
pub const TYPE_INVENTORY: &str = "inventory";
/// Тип агрегата «переміщення між точками».
pub const TYPE_TRANSFER: &str = "transfer";
/// Тип агрегата «списання».
pub const TYPE_WRITE_OFF: &str = "write_off";

/// Результат локального запису транзакції.
#[derive(Debug, Clone, PartialEq)]
pub struct EnqueuedTransaction {
    /// Локальний rowid у таблиці агрегата.
    pub id: i64,
    /// UUIDv4 каси — ідемпотентний ключ майбутнього push.
    pub client_uuid: String,
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
    let items = stock::parse_items(payload);
    if items.is_empty() {
        return Ok(()); // немає позицій з кількістю — ефекту немає
    }
    match kind {
        TYPE_PURCHASE_ORDER => {
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
        other => Err(format!("тип транзакції без локальної таблиці: {other}")),
    }
}

/// Атомарний запис локальної транзакції: агрегат (таблиця 0006, synced=0)
/// + stock-ефект — ОДНА SQLite-транзакція (BEGIN IMMEDIATE → INSERT →
/// ефекти → COMMIT). Помилка будь-де → ROLLBACK: жодного агрегата без
/// stock-ефекту і навпаки (дизайн 4.4).
///
/// Повертає client_uuid агрегата (ідемпотентний ключ майбутнього push).
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

    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE ({kind}): {e}"))?;

    // 1. Агрегат: data = payload як є (фронтовий /v2-формат), synced = 0
    //    (доставка на сервер — ЕТАП 7; outbox НЕ використовується — див.
    //    doc-коментар модуля).
    tx.execute(
        &format!(
            "INSERT INTO {table} (client_uuid, store_id, data, synced) \
             VALUES (?1, ?2, ?3, 0)"
        ),
        params![client_uuid, store_id, payload_json],
    )
    .map_err(|e| format!("INSERT {table} (client_uuid={client_uuid}): {e}"))?;
    let id = tx.last_insert_rowid();

    // 2. Stock-ефект — у тій самій транзакції.
    apply_effects(&tx, kind, &payload, store_id)
        .map_err(|e| format!("stock-ефект {kind} (client_uuid={client_uuid}): {e}"))?;

    // 3. COMMIT.
    tx.commit()
        .map_err(|e| format!("COMMIT ({kind}): {e}"))?;

    Ok(EnqueuedTransaction { id, client_uuid })
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

    /// Закупка: агрегат записано (synced=0, client_uuid) + stock +qty.
    #[test]
    fn purchase_enqueues_aggregate_and_adds_stock() {
        let mut conn = migrated_conn();
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

        let out = enqueue_transaction(&mut c, TYPE_PURCHASE_ORDER, &payload, STORE)
            .expect("закупка");
        assert_eq!(level(&c, "p1"), 10_000, "+10 шт");
        assert_eq!(level(&c, "p2"), 2500, "+2.5 шт");

        let (data, synced, sid) =
            get_transaction(&c, TYPE_PURCHASE_ORDER, &out.client_uuid).expect("читання");
        assert_eq!(synced, 0, "очікує синхронізації (ЕТАП 7)");
        assert_eq!(sid.as_deref(), Some(STORE));
        let v: Value = serde_json::from_str(&data).expect("data JSON");
        assert_eq!(v["supplier_id"], "sup-1");
        assert_eq!(unsynced_count(&c, TYPE_PURCHASE_ORDER).unwrap(), 1);
    }

    /// Інвентаризація: АБСОЛЮТНИЙ рівень (факт перерахунку), не дельта.
    #[test]
    fn inventory_sets_absolute_level() {
        let mut conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 5000).expect("до: 5 шт");
        let payload = json!({
            "items": [{"product_id": "p1", "fact_quantity": 7}],
        })
        .to_string();
        enqueue_transaction(&mut c, TYPE_INVENTORY, &payload, STORE).expect("інвентаризація");
        assert_eq!(level(&c, "p1"), 7000, "факт 7 шт (не 5+7)");
        assert_eq!(unsynced_count(&c, TYPE_INVENTORY).unwrap(), 1);
    }

    /// Списання: stock −qty.
    #[test]
    fn write_off_subtracts_stock() {
        let mut conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 10_000).expect("до: 10 шт");
        let payload = json!({
            "reason": "псування",
            "items": [{"product_id": "p1", "quantity": 3}],
        })
        .to_string();
        enqueue_transaction(&mut c, TYPE_WRITE_OFF, &payload, STORE).expect("списання");
        assert_eq!(level(&c, "p1"), 7000, "10 − 3 = 7 шт");
    }

    /// Переміщення З каси (from=store): −qty.
    #[test]
    fn transfer_out_subtracts_stock() {
        let mut conn = migrated_conn();
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
        let mut conn = migrated_conn();
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
        let mut conn = migrated_conn();
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
        let mut conn = migrated_conn();
        let mut c = conn;
        stock::apply_stock_delta(&c, STORE, "p1", 500).expect("до");

        let res = enqueue_transaction(&mut c, TYPE_PURCHASE_ORDER, "{не-json", STORE);
        assert!(res.is_err(), "невалідний payload → помилка");
        assert_eq!(unsynced_count(&c, TYPE_PURCHASE_ORDER).unwrap(), 0, "агрегата немає");
        assert_eq!(level(&c, "p1"), 500, "stock без змін (ROLLBACK)");

        // Невідомий тип — помилка ДО транзакції.
        assert!(enqueue_transaction(&mut c, "work_session", "{}", STORE).is_err());
    }

    /// Кожен тип має власну таблицю: запис+читання (критерій тестів ЕТАПУ 6).
    #[test]
    fn each_kind_reads_back_from_own_table() {
        let mut conn = migrated_conn();
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
}
