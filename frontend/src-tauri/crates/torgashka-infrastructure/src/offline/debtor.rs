//! Локальний борг покупця за очима каси (offline-first, ADR-0007 §11.6.4,
//! Фаза 3.3b).
//!
//! Рішення NIKO §11.6.4 **варіант 1**: боргові сутності — клас `LocalOutbox`
//! (борг стає можливим на standby). Оплата боргу створюється касою офлайн:
//! агрегат → наявна таблиця `debtors_ledger` (міграція 0006), похідний стан →
//! `debtor_balances` (міграція 0012).
//!
//! Таблиця `debtor_balances(store_id, debtor_id)` → `pending_cents`:
//! сума оплат, які каса вже прийняла, але primary ще **не** застосував
//! (outbox `pending`). Тому локальний борг = `total_debt` репліки −
//! `pending_cents` (не менше 0) — саме це віддає адаптер у відповіді
//! `POST /api/v1/debtors/{id}/pay` на standby.
//!
//! Гроші — **копійки** (ціле, scale 2): та сама конвенція, що
//! [`super::cash`]/`stock` (без f64).
//!
//! **Атомарність (дизайн 4.4)**: [`apply_pending_payment`] приймає
//! `&Connection` і викликається ЛИШЕ всередині вже відкритої транзакції
//! агрегата (`transactions::enqueue_transaction` → `apply_effects`):
//! агрегат + outbox + похідний борг комітяться/відкочуються разом.

use rusqlite::{params, Connection};
use serde_json::Value;

use super::cash::cents;

/// `(debtor_id, копійки)` з payload агрегата `debtor_payment`.
/// `None` — немає `debtor_id` або сума не додатна (агрегат НЕ пишемо).
pub fn payment_from_payload(payload: &Value) -> Option<(String, i64)> {
    let debtor_id = payload
        .get("debtor_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let amount = cents(payload.get("amount")?)?;
    if amount <= 0 {
        return None;
    }
    Some((debtor_id, amount))
}

/// `pending_cents += delta_cents` для боржника точки (UPSERT).
pub fn apply_pending_payment(
    conn: &Connection,
    store_id: &str,
    debtor_id: &str,
    delta_cents: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO debtor_balances (store_id, debtor_id, pending_cents, updated_at) \
         VALUES (?1, ?2, ?3, datetime('now')) \
         ON CONFLICT (store_id, debtor_id) \
         DO UPDATE SET pending_cents = debtor_balances.pending_cents + ?3, \
                       updated_at = datetime('now')",
        params![store_id, debtor_id, delta_cents],
    )
    .map_err(|e| format!("debtor_balances upsert ({store_id}/{debtor_id}): {e}"))?;
    Ok(())
}

/// Сума локальних (ще не підтверджених primary) оплат боржника, копійки.
pub fn pending_payment_cents(
    conn: &Connection,
    store_id: &str,
    debtor_id: &str,
) -> Result<i64, String> {
    match conn.query_row(
        "SELECT pending_cents FROM debtor_balances WHERE store_id = ?1 AND debtor_id = ?2",
        params![store_id, debtor_id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(format!(
            "SELECT debtor_balances ({store_id}/{debtor_id}): {e}"
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
            payment_from_payload(&json!({"debtor_id": "d1", "amount": "150.50"})),
            Some(("d1".to_string(), 15050))
        );
        // число-рядок теж приймається (v2-формат)
        assert_eq!(
            payment_from_payload(&json!({"debtor_id": "d1", "amount": 20})),
            Some(("d1".to_string(), 2000))
        );
        // без боржника / нульова / від'ємна сума → агрегат не пишемо
        assert_eq!(payment_from_payload(&json!({"amount": "10"})), None);
        assert_eq!(
            payment_from_payload(&json!({"debtor_id": "d1", "amount": "0"})),
            None
        );
        assert_eq!(
            payment_from_payload(&json!({"debtor_id": "d1", "amount": "-5"})),
            None
        );
    }
}
