//! Локальна книга постачальника за очима каси (offline-first, ADR-0007
//! §11.7.9.7, Фаза 3.3b).
//!
//! `POST /api/v1/ledger` і `POST /api/v2/ledger/entries` — **самостійний
//! документ** (ручний запис у книгу: `operation_type` + `amount` + `notes`),
//! НЕ похідна накладної чи повернення. Тому сутність отримує власний тип
//! черги `supplier_ledger` (а не «входить в ефект документа»): на standby
//! запис стає можливим офлайн.
//!
//! Агрегат — локальна таблиця `supplier_ledger` (міграція 0012); похідний
//! стан — `supplier_balances(store_id, supplier_id).pending_amount_cents`:
//! сума записів каси, які primary ще не прийняв. Локальний `balance_after`
//! для відповіді = баланс репліки (читання дозволене, §10) + ця сума після
//! запису — та сама арифметика, що робить primary при прийомі.
//!
//! Гроші — **копійки** (ціле, scale 2). Атомарність — як у
//! [`super::debtor`]: виклик лише всередині транзакції агрегата.

use rusqlite::{params, Connection};
use serde_json::Value;

use super::cash::cents;

/// `(supplier_id, копійки)` з payload агрегата `supplier_ledger`.
/// `None` — немає `supplier_id` або сума не число (агрегат НЕ пишемо).
pub fn entry_from_payload(payload: &Value) -> Option<(String, i64)> {
    let supplier_id = payload
        .get("supplier_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let amount = cents(payload.get("amount")?)?;
    if amount == 0 {
        return None;
    }
    Some((supplier_id, amount))
}

/// `pending_amount_cents += delta_cents` для постачальника точки (UPSERT).
pub fn apply_pending_entry(
    conn: &Connection,
    store_id: &str,
    supplier_id: &str,
    delta_cents: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO supplier_balances (store_id, supplier_id, pending_amount_cents, updated_at) \
         VALUES (?1, ?2, ?3, datetime('now')) \
         ON CONFLICT (store_id, supplier_id) \
         DO UPDATE SET pending_amount_cents = supplier_balances.pending_amount_cents + ?3, \
                       updated_at = datetime('now')",
        params![store_id, supplier_id, delta_cents],
    )
    .map_err(|e| format!("supplier_balances upsert ({store_id}/{supplier_id}): {e}"))?;
    Ok(())
}

/// Сума локальних (ще не прийнятих primary) записів книги, копійки.
pub fn pending_amount_cents(
    conn: &Connection,
    store_id: &str,
    supplier_id: &str,
) -> Result<i64, String> {
    match conn.query_row(
        "SELECT pending_amount_cents FROM supplier_balances \
         WHERE store_id = ?1 AND supplier_id = ?2",
        params![store_id, supplier_id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(format!(
            "SELECT supplier_balances ({store_id}/{supplier_id}): {e}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_parsing() {
        assert_eq!(
            entry_from_payload(&json!({"supplier_id": "s1", "amount": "100.00"})),
            Some(("s1".to_string(), 10000))
        );
        assert_eq!(
            entry_from_payload(&json!({"supplier_id": "s1", "amount": "-50.25"})),
            Some(("s1".to_string(), -5025))
        );
        assert_eq!(entry_from_payload(&json!({"amount": "10"})), None);
        assert_eq!(
            entry_from_payload(&json!({"supplier_id": "s1", "amount": "0"})),
            None
        );
    }
}
