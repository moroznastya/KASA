//! Звірка залишків накладної: локальний (SQLite) vs авторитетний (PG) —
//! ЛИШЕ ПОКАЗ (ADR-0007 §10.3 п.3).
//!
//! Розділення відповідальності (§10.3):
//!   * локальний залишок каси (SQLite `stock`) — **оптимістична ОЦІНКА**:
//!     каса застосовує stock-ефект документа одразу, ще до доставки на
//!     primary (offline-first, дизайн 4.4);
//!   * авторитетний залишок точки — PostgreSQL (на standby — локальна
//!     репліка, читається у `route_local` через RLS-пул);
//!   * ВИРІВНЮВАННЯ робить НАЯВНИЙ механізм — інвентаризація
//!     (`stock::set_stock_level`, `TYPE_INVENTORY`). Нового reconcile-движка
//!     НЕ заводиться (заборона §10.3) — цей модуль лише ЧИТАЄ й показує.
//!
//! Модуль віддає ЛОКАЛЬНУ половину (SQLite): позиції документа та локальний
//! залишок по кожному товару. Авторитетну половину додає HTTP-обробник
//! (`/api/v1/local/stock-reconciliation`), бо вона вимагає пулу репліки.
//!
//! Джерело позицій — `data` ЛОКАЛЬНОГО агрегата (`invoices`/`return_invoices`,
//! client_uuid = id документа у відповіді створення), а не `invoice_items`:
//! так покриваються ОБИДВА контури запису (окремий `enqueue_invoice` і
//! загальний `enqueue_transaction`), і позиції беруться тим самим парсером,
//! що робив stock-ефект (`stock::parse_items`) — звірка показує рівно те, що
//! документ застосував до локального stock.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use super::{catalog, stock};

/// Позиція звірки (локальна половина).
#[derive(Debug, Clone, PartialEq)]
pub struct LocalReconLine {
    /// Товар документа (той самий product_id, що в payload).
    pub product_id: String,
    /// Ім'я з локального каталогу; невідомий товар → сам product_id.
    pub name: String,
    /// Локальний (оптимістичний) залишок точки, МІЛІОДИНИЦІ (scale 3).
    pub local_milli: i64,
}

/// Локальний агрегат документа, знайдений у черзі каси.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalInvoiceView {
    /// client_uuid документа (= id у відповіді створення).
    pub invoice_id: String,
    /// `invoice` (приймання) або `return_invoice` (повернення постачальнику).
    pub kind: &'static str,
    /// Номер документа з payload (для людини).
    pub number: Option<String>,
    /// Точка документа (ключ локального stock).
    pub store_id: Option<String>,
    /// Позиції документа, які дали stock-ефект (qty > 0).
    pub lines: Vec<LocalReconLine>,
}

/// Локальний агрегат документа за `client_uuid` з локальним залишком по
/// кожній позиції. `Ok(None)` — документа немає в черзі ЦІЄЇ каси.
pub fn local_view(conn: &Connection, invoice_id: &str) -> Result<Option<LocalInvoiceView>, String> {
    let found = load_aggregate(conn, invoice_id)?;
    let Some((kind, store_id, number, data)) = found else {
        return Ok(None);
    };
    let payload: Value = serde_json::from_str(&data)
        .map_err(|e| format!("локальний агрегат {invoice_id}: невалідний JSON data: {e}"))?;
    let store_key = store_id.clone().unwrap_or_default();
    let mut lines: Vec<LocalReconLine> = Vec::new();
    for (product_id, _qty_milli) in stock::parse_items(&payload) {
        // Дублікати позицій документа зводимо до одного рядка звірки
        // (stock-ефект застосовується сумою — показуємо залишок один раз).
        if lines.iter().any(|l| l.product_id == product_id) {
            continue;
        }
        let name = catalog::catalog_name(conn, &product_id)?
            .unwrap_or_else(|| product_id.clone());
        let local_milli = stock::get_stock_level(conn, &store_key, &product_id)?;
        lines.push(LocalReconLine {
            product_id,
            name,
            local_milli,
        });
    }
    Ok(Some(LocalInvoiceView {
        invoice_id: invoice_id.to_string(),
        kind,
        number,
        store_id,
        lines,
    }))
}

/// Агрегат із `invoices` (приймання) або `return_invoices` (повернення).
#[allow(clippy::type_complexity)]
fn load_aggregate(
    conn: &Connection,
    invoice_id: &str,
) -> Result<Option<(&'static str, Option<String>, Option<String>, String)>, String> {
    let invoice: Option<(Option<String>, Option<String>, String)> = conn
        .query_row(
            "SELECT store_id, number, data FROM invoices WHERE client_uuid = ?1",
            params![invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| format!("локальна накладна {invoice_id}: {e}"))?;
    if let Some((store_id, number, data)) = invoice {
        return Ok(Some(("invoice", store_id, number, data)));
    }
    let ret: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT store_id, data FROM return_invoices WHERE client_uuid = ?1",
            params![invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("локальне повернення {invoice_id}: {e}"))?;
    Ok(ret.map(|(store_id, data)| ("return_invoice", store_id, None, data)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::transactions;
    use serde_json::json;

    fn migrated_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory");
        super::super::migrations::migrate(&conn).expect("міграції");
        conn
    }

    fn put_catalog(conn: &Connection, id: &str, name: &str) {
        conn.execute(
            "INSERT INTO products_v2 (id, name, is_deleted, server_version) VALUES (?1, ?2, 0, 1)",
            params![id, name],
        )
        .expect("products_v2");
    }

    #[test]
    fn local_view_reads_invoice_lines_and_local_stock() {
        let mut conn = migrated_conn();
        put_catalog(&conn, "p-1", "Кава");
        put_catalog(&conn, "p-2", "Молоко");
        let payload = json!({
            "number": "RECON-1",
            "supplier_id": "sup-1",
            "items": [
                {"product_id": "p-1", "quantity": "3"},
                {"product_id": "p-2", "quantity": "1.5"},
                {"product_id": "p-1", "quantity": "1"}   // дубль → один рядок
            ]
        })
        .to_string();
        let out = transactions::enqueue_invoice(&mut conn, &payload, "store-1")
            .expect("накладна в черзі");

        let view = local_view(&conn, &out.client_uuid)
            .expect("local_view")
            .expect("документ знайдено");
        assert_eq!(view.kind, "invoice");
        assert_eq!(view.number.as_deref(), Some("RECON-1"));
        assert_eq!(view.store_id.as_deref(), Some("store-1"));
        assert_eq!(view.lines.len(), 2, "дубль зведено: {:#?}", view.lines);
        // Локальний optimistic-залишок = stock-ефект документа (3 і 1.5).
        assert_eq!(view.lines[0].product_id, "p-1");
        assert_eq!(view.lines[0].local_milli, 4_000);
        assert_eq!(view.lines[0].name, "Кава");
        assert_eq!(view.lines[1].local_milli, 1_500);
        assert_eq!(view.lines[1].name, "Молоко");
    }

    #[test]
    fn local_view_covers_return_invoice_and_unknown_invoice() {
        let mut conn = migrated_conn();
        put_catalog(&conn, "p-1", "Кава");
        conn.execute(
            "INSERT INTO stock (store_id, product_id, quantity) VALUES ('store-1', 'p-1', 5000)",
            [],
        )
        .expect("stock");
        let payload = json!({
            "number": "RET-1",
            "supplier_id": "sup-1",
            "items": [{"product_id": "p-1", "quantity": "2"}]
        })
        .to_string();
        let out = transactions::enqueue_transaction(
            &mut conn,
            transactions::TYPE_RETURN_INVOICE,
            &payload,
            "store-1",
        )
        .expect("повернення постачальнику");

        let view = local_view(&conn, &out.client_uuid)
            .expect("local_view")
            .expect("повернення знайдено");
        assert_eq!(view.kind, "return_invoice");
        assert_eq!(view.lines.len(), 1);
        // 5000 − 2000 (повернення забирає товар з точки) = 3000.
        assert_eq!(view.lines[0].local_milli, 3_000);
        assert_eq!(view.number, None, "у return_invoices номер живе лише в data");

        assert!(
            local_view(&conn, "нема-такого").expect("local_view").is_none(),
            "невідомий документ → None (обробник віддає 404)"
        );
    }

    #[test]
    fn unknown_product_in_invoice_is_shown_by_id() {
        let mut conn = migrated_conn();
        // Каталог порожній: товар невідомий — звірка це ПОКАЗУЄ (не падає).
        let payload = json!({
            "number": "RECON-2",
            "items": [{"product_id": "p-ghost", "quantity": "1"}]
        })
        .to_string();
        let out = transactions::enqueue_transaction(
            &mut conn,
            transactions::TYPE_PURCHASE_ORDER,
            &payload,
            "store-1",
        )
        .expect("закупівля (валідації каталогу не має)");
        // Закупівля не має локального агрегата-накладної → None (див. таблицю
        // table_of: purchase_orders не читається звіркою накладних).
        assert!(local_view(&conn, &out.client_uuid).expect("local_view").is_none());
    }
}
