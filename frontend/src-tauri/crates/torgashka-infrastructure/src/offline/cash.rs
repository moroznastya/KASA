//! Локальний касовий ящик каси (offline-first, ADR-0007 §11.6).
//!
//! Таблиця `cash_balance` (offline-міграція 0011): `(store_id, cash_type)` →
//! `balance_cents` у **копійках** (ціле, scale 2 — серверний `numeric(12,2)`).
//! Цілочисельна арифметика уникáє похибок f64 (0.1 + 0.2 ≠ 0.3) — та сама
//! конвенція, що в `stock.rs` (міліодиниці, scale 3).
//!
//! Операції:
//!   * [`apply_cash_delta`]  — дельта балансу за **доступну готівку**
//!     (внесення `deposit` → +amount, інкасація `collection` → −amount);
//!   * [`get_cash_balance`]  — поточний локальний баланс (копійки);
//!   * [`cash_delta`]        — дельта з payload агрегата `cash_operation`.
//!
//! **Атомарність (дизайн 4.4)**: `apply_cash_delta` приймає `&Connection` і
//! ВИКЛИКАЄТЬСЯ ЛИШЕ всередині вже відкритої транзакції агрегата
//! (`transactions::enqueue_transaction` → `apply_effects`) — баланс і запис
//! агрегата + outbox комітяться/відкочуються разом. Самостійно транзакцію не
//! відкриває.
//!
//! **Обмеження (задокументовано)**: локальний баланс — **оцінка** каси
//! (той самий статус, що локальний stock, §10.3 ADR-0007). Авторитетний
//! баланс рахує primary за таблицею `cash_operations`; після ack локальний
//! баланс не «доганяє» репліку автоматично (master-pull не віддає
//! `cash_operations`) — розбіжність вирівнюється новою операцією/інкасацією.

use rusqlite::{params, Connection};
use serde_json::Value;

/// Копійок в одній грошовій одиниці (server `numeric(12,2)`).
pub const CENTS_PER_UNIT: i64 = 100;

/// Знак операції для балансу каси: `deposit` → +1, `collection` → −1.
/// Невідомий тип → `None` (агрегат НЕ пишеться: див. `cash_delta`).
pub fn op_sign(operation_type: &str) -> Option<i64> {
    match operation_type {
        "deposit" => Some(1),
        "collection" => Some(-1),
        _ => None,
    }
}

/// Грошова сума з JSON у копійках (scale 2), без f64: число або рядок
/// розбираються як десятковий (`"100.005"` → 10000 — обрізання до 2 знаків,
/// як серверний `numeric(12,2)`).
pub fn cents(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => cents_from_str(&n.to_string()),
        Value::String(s) => cents_from_str(s),
        _ => None,
    }
}

/// Десятковий рядок → копійки. `None` для нечислового/порожнього.
fn cents_from_str(s: &str) -> Option<i64> {
    let s = s.trim().replace(',', ".");
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.as_str()),
    };
    if body.is_empty() {
        return None;
    }
    let (int_part, frac_part) = match body.split_once('.') {
        Some((a, b)) => (a, b),
        None => (body, ""),
    };
    if !int_part.chars().all(|c| c.is_ascii_digit()) && !int_part.is_empty() {
        return None;
    }
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let int: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let mut frac = frac_part.to_string();
    frac.truncate(2);
    while frac.len() < 2 {
        frac.push('0');
    }
    let frac: i64 = if frac.is_empty() {
        0
    } else {
        frac.parse().ok()?
    };
    let total = int.checked_mul(CENTS_PER_UNIT)?.checked_add(frac)?;
    Some(if neg { -total } else { total })
}

/// Дельта балансу каси з payload агрегата `cash_operation`.
///
/// `deposit` → +amount, `collection` → −amount. Невідомий `operation_type`,
/// відсутня/нечислова сума → `None`: агрегат не пишеться в SQLite (краще
/// відмовити в хендлері, ніж покласти в чергу операцію без ефекту).
pub fn cash_delta(payload: &Value) -> Option<i64> {
    let op = payload.get("operation_type")?.as_str()?;
    let sign = op_sign(op)?;
    let amount = cents(payload.get("amount")?)?;
    amount.checked_mul(sign)
}

/// Застосувати дельту до балансу `(store_id, cash_type)` ВСЕРЕДИНІ транзакції
/// агрегата. UPSERT: перша операція створює рядок, далі — накопичення.
pub fn apply_cash_delta(
    conn: &Connection,
    store_id: &str,
    cash_type: &str,
    delta_cents: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO cash_balance (store_id, cash_type, balance_cents, updated_at) \
         VALUES (?1, ?2, ?3, datetime('now')) \
         ON CONFLICT (store_id, cash_type) \
         DO UPDATE SET balance_cents = cash_balance.balance_cents + ?3, \
                       updated_at = datetime('now')",
        params![store_id, cash_type, delta_cents],
    )
    .map_err(|e| format!("cash_balance upsert ({store_id}/{cash_type}): {e}"))?;
    Ok(())
}

/// Поточний локальний баланс каси (копійки); 0, якщо рядка немає.
pub fn get_cash_balance(conn: &Connection, store_id: &str, cash_type: &str) -> Result<i64, String> {
    match conn.query_row(
        "SELECT balance_cents FROM cash_balance WHERE store_id = ?1 AND cash_type = ?2",
        params![store_id, cash_type],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(format!("SELECT cash_balance ({store_id}/{cash_type}): {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory SQLite");
        conn.execute_batch(
            "CREATE TABLE cash_balance (\
                 id INTEGER PRIMARY KEY AUTOINCREMENT,\
                 store_id TEXT NOT NULL, cash_type TEXT NOT NULL,\
                 balance_cents INTEGER NOT NULL DEFAULT 0,\
                 updated_at TEXT NOT NULL DEFAULT (datetime('now')),\
                 UNIQUE (store_id, cash_type));",
        )
        .expect("схема cash_balance");
        conn
    }

    #[test]
    fn decimal_string_maps_to_cents_without_f64() {
        assert_eq!(cents(&json!("100.00")), Some(10_000));
        assert_eq!(cents(&json!("100.005")), Some(10_000)); // обрізання scale 2
        assert_eq!(cents(&json!("0.5")), Some(50));
        assert_eq!(cents(&json!(250)), Some(25_000));
        assert_eq!(cents(&json!("100,25")), Some(10_025));
        assert_eq!(cents(&json!("")), None);
        assert_eq!(cents(&json!("abc")), None);
        assert_eq!(cents(&json!(null)), None);
    }

    #[test]
    fn delta_signs_follow_operation_type() {
        let dep = json!({"operation_type": "deposit", "amount": "150.00"});
        let col = json!({"operation_type": "collection", "amount": "150.00"});
        assert_eq!(cash_delta(&dep), Some(15_000));
        assert_eq!(cash_delta(&col), Some(-15_000));
        // Невідомий тип / без суми → None (агрегат не пишеться).
        assert_eq!(cash_delta(&json!({"operation_type": "x", "amount": "1"})), None);
        assert_eq!(cash_delta(&json!({"operation_type": "deposit"})), None);
    }

    #[test]
    fn balance_accumulates_per_cash_type_inside_one_transaction() {
        let conn = mem();
        let s = "store-1";
        assert_eq!(get_cash_balance(&conn, s, "cash").expect("balance"), 0);
        // deposit 100.00 + deposit 50.50 − collection 30.00 = 120.50
        apply_cash_delta(&conn, s, "cash", 10_000).expect("deposit 1");
        apply_cash_delta(&conn, s, "cash", 5_050).expect("deposit 2");
        apply_cash_delta(&conn, s, "cash", -3_000).expect("collection");
        assert_eq!(get_cash_balance(&conn, s, "cash").expect("balance"), 12_050);
        // card — окремий ящик (не змішується з cash).
        assert_eq!(get_cash_balance(&conn, s, "card").expect("card"), 0);
        apply_cash_delta(&conn, s, "card", 7_700).expect("card deposit");
        assert_eq!(get_cash_balance(&conn, s, "card").expect("card"), 7_700);
        assert_eq!(get_cash_balance(&conn, s, "cash").expect("cash"), 12_050);
        // інша точка — окремий баланс.
        assert_eq!(get_cash_balance(&conn, "store-2", "cash").expect("b2"), 0);
    }
}
