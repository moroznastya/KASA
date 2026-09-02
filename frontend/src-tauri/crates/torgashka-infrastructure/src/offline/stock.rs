//! Локальний stock каси (ЕТАП 6 offline-first).
//!
//! Таблиця `stock` (міграція 0005): (store_id, product_id) → quantity
//! у **міліодиницях** (ціле, scale 3) — серверна конвенція scaled3:
//! 1 шт = 1000, 0.500 кг = 500. Цілочисельна арифметика уникáє похибок f64
//! (0.1 + 0.2 ≠ 0.3) і збігається з серверним `parse_scaled3`.
//!
//! Операції:
//!   * [`apply_stock_delta`] — дельта (продаж −qty, закупка +qty, списання −qty);
//!   * [`set_stock_level`]   — абсолютний рівень (інвентаризація = факт);
//!   * [`get_stock_level`]   — рівень одного товару (0, якщо рядка немає);
//!   * [`stock_with_catalog`] — рівні всіх товарів каталогу точки (LEFT JOIN
//!     products_v2) — для UI «каталог + залишки».
//!
//! **Атомарність (дизайн 4.4)**: функції мутації приймають `&Connection` і
//! ВИКЛИКАЮТЬСЯ ЛИШЕ всередині вже відкритої транзакції агрегата
//! (enqueue_receipt / enqueue_transaction) — stock-ефект і запис агрегата
//! комітяться/відкочуються разом. Самостійно транзакцію не відкривають.
//!
//! Обмеження pull (задокументовано в 0005): серверний master-pull не віддає
//! сутність «stock» (кількості точки) → локальний stock стартує порожнім;
//! первинні залишки фіксує інвентаризація. Від'ємний залишок ДОПУСКАЄТЬСЯ
//! (продаж не блокується неточним локальним рівнем).

use rusqlite::{params, Connection};
use serde_json::Value;

/// Кількість міліодиниць в одній одиниці (scale 3).
pub const UNITS_SCALE: i64 = 1000;

/// Парсинг кількості з JSON-поля агрегата у міліодиниці.
///
/// Приймає число (`1`, `0.5`) або рядок (`"1"`, `"0.500"`, `"1.5"`).
/// Непарсибельне значення → 0 (агрегат записується, stock-ефекту немає).
pub fn qty_to_milli(v: &Value) -> i64 {
    match v {
        Value::Number(n) => (n.as_f64().unwrap_or(0.0) * UNITS_SCALE as f64).round() as i64,
        Value::String(s) => {
            let f: f64 = s.trim().parse().unwrap_or(0.0);
            (f * UNITS_SCALE as f64).round() as i64
        }
        _ => 0,
    }
}

/// Міліодиниці → одиниці (для JSON-відповідей команд).
pub fn milli_to_units(milli: i64) -> f64 {
    milli as f64 / UNITS_SCALE as f64
}

/// Розпарсити `items` агрегата (дизайн 2.2) у (product_id, qty_мілі).
///
/// Формат items — спільний для всіх агрегатів каси (snake_case, як чек
/// продажу ЕТАП 4): `[{"product_id": "...", "quantity": 1|"1.5"}, ...]`.
/// Інвентаризація може називати кількість `fact_quantity` (факт за
/// перерахунком) — враховується як fallback для quantity.
/// Елементи без product_id або з quantity ≤ 0 пропускаються.
pub fn parse_items(payload: &Value) -> Vec<(String, i64)> {
    let Some(items) = payload.get("items").and_then(|i| i.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|it| {
            let pid = it
                .get("product_id")
                .or_else(|| it.get("productId"))
                .and_then(|p| p.as_str())?;
            let q = it
                .get("quantity")
                .or_else(|| it.get("fact_quantity"))
                .map(qty_to_milli)
                .unwrap_or(0);
            if q <= 0 {
                return None;
            }
            Some((pid.to_string(), q))
        })
        .collect()
}

/// UPSERT-ядро: quantity стає `quantity + delta_milli`.
fn upsert_delta(
    conn: &Connection,
    store_id: &str,
    product_id: &str,
    delta_milli: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO stock (store_id, product_id, quantity, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(store_id, product_id) DO UPDATE SET
            quantity = stock.quantity + excluded.quantity,
            updated_at = excluded.updated_at",
        params![store_id, product_id, delta_milli],
    )
    .map_err(|e| format!("stock delta ({product_id}, {delta_milli}): {e}"))?;
    Ok(())
}

/// Дельта залишку (викликається ВСЕРЕДИНІ транзакції агрегата):
/// продаж −qty, закупка +qty, списання −qty, прийом переміщення +qty.
pub fn apply_stock_delta(
    conn: &Connection,
    store_id: &str,
    product_id: &str,
    delta_milli: i64,
) -> Result<(), String> {
    if delta_milli == 0 {
        return Ok(());
    }
    upsert_delta(conn, store_id, product_id, delta_milli)
}

/// Абсолютний рівень (інвентаризація: факт за перерахунком, не дельта).
pub fn set_stock_level(
    conn: &Connection,
    store_id: &str,
    product_id: &str,
    level_milli: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO stock (store_id, product_id, quantity, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(store_id, product_id) DO UPDATE SET
            quantity = excluded.quantity,
            updated_at = excluded.updated_at",
        params![store_id, product_id, level_milli],
    )
    .map_err(|e| format!("stock set_level ({product_id}, {level_milli}): {e}"))?;
    Ok(())
}

/// Поточний рівень товару точки в міліодиницях (0, якщо рядка немає).
pub fn get_stock_level(conn: &Connection, store_id: &str, product_id: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT quantity FROM stock WHERE store_id = ?1 AND product_id = ?2",
        params![store_id, product_id],
        |row| row.get::<_, i64>(0),
    )
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(0),
        other => Err(format!("stock level ({product_id}): {other}")),
    })
}

/// Усі товари каталогу точки з локальними рівнями: (product_id, name, level_мілі).
/// LEFT JOIN products_v2 — товари без stock-рядка повертаються з рівнем 0.
pub fn stock_with_catalog(
    conn: &Connection,
    store_id: &str,
) -> Result<Vec<(String, String, i64)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT p.id, COALESCE(p.name, p.id), COALESCE(s.quantity, 0)
             FROM products_v2 p
             LEFT JOIN stock s ON s.product_id = p.id AND s.store_id = ?1
             WHERE p.is_deleted = 0
             ORDER BY p.name",
        )
        .map_err(|e| format!("підготовка stock_with_catalog: {e}"))?;
    let rows = stmt
        .query_map(params![store_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| format!("stock_with_catalog: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("рядок stock_with_catalog: {e}"))?);
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn migrated_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory");
        super::super::migrations::migrate(&conn).expect("міграції");
        conn
    }

    #[test]
    fn qty_to_milli_parses_numbers_and_strings() {
        assert_eq!(qty_to_milli(&json!(1)), 1000);
        assert_eq!(qty_to_milli(&json!(1.5)), 1500);
        assert_eq!(qty_to_milli(&json!("1")), 1000);
        assert_eq!(qty_to_milli(&json!("0.500")), 500);
        assert_eq!(qty_to_milli(&json!("0.001")), 1);
        assert_eq!(qty_to_milli(&json!("abc")), 0);
        assert_eq!(qty_to_milli(&Value::Null), 0);
    }

    #[test]
    fn parse_items_extracts_product_and_quantity() {
        let payload = json!({
            "items": [
                {"product_id": "p1", "quantity": 2},
                {"product_id": "p2", "quantity": "0.500"},
                {"product_id": "p3", "fact_quantity": 7},
                {"quantity": 5},               // без product_id — skip
                {"product_id": "p4", "quantity": 0}, // qty 0 — skip
                {"productId": "p5", "quantity": 1},  // camelCase fallback
            ]
        });
        let items = parse_items(&payload);
        assert_eq!(items, vec![
            ("p1".to_string(), 2000),
            ("p2".to_string(), 500),
            ("p3".to_string(), 7000),
            ("p5".to_string(), 1000),
        ]);
        assert!(parse_items(&json!({})).is_empty());
        assert!(parse_items(&json!({"items": "no"})).is_empty());
    }

    #[test]
    fn apply_stock_delta_and_set_level() {
        let conn = migrated_conn();
        // Рядка немає → delta створює.
        apply_stock_delta(&conn, "s1", "p1", 1000).expect("delta +1");
        assert_eq!(get_stock_level(&conn, "s1", "p1").unwrap(), 1000);
        // Продаж −0.5.
        apply_stock_delta(&conn, "s1", "p1", -500).expect("delta −0.5");
        assert_eq!(get_stock_level(&conn, "s1", "p1").unwrap(), 500);
        // Закупка +2.
        apply_stock_delta(&conn, "s1", "p1", 2000).expect("delta +2");
        assert_eq!(get_stock_level(&conn, "s1", "p1").unwrap(), 2500);
        // Товар без рядка → 0.
        assert_eq!(get_stock_level(&conn, "s1", "p-none").unwrap(), 0);
        // Інвентаризація: абсолютний рівень (не дельта).
        set_stock_level(&conn, "s1", "p1", 700).expect("set 0.700");
        assert_eq!(get_stock_level(&conn, "s1", "p1").unwrap(), 700);
        // Роздільність точок.
        apply_stock_delta(&conn, "s2", "p1", 3000).expect("s2");
        assert_eq!(get_stock_level(&conn, "s1", "p1").unwrap(), 700);
        assert_eq!(get_stock_level(&conn, "s2", "p1").unwrap(), 3000);
    }

    #[test]
    fn negative_stock_allowed_until_inventory() {
        // Дизайн: продаж не блокується неточним локальним рівнем —
        // від'ємний залишок тимчасово допустимий (інвентаризація вирівнює).
        let conn = migrated_conn();
        apply_stock_delta(&conn, "s1", "p1", 500).expect("+0.5");
        apply_stock_delta(&conn, "s1", "p1", -2000).expect("−2");
        assert_eq!(get_stock_level(&conn, "s1", "p1").unwrap(), -1500);
    }

    #[test]
    fn stock_with_catalog_left_joins_products_v2() {
        let conn = migrated_conn();
        // products_v2 наповнюється pull/міграцією; тут — напряму (тест).
        conn.execute(
            "INSERT INTO products_v2 (id, name, price) VALUES ('p1', 'Товар А', 100.00),
             ('p2', 'Товар Б', 50.00)",
            [],
        )
        .expect("каталог");
        apply_stock_delta(&conn, "s1", "p1", 2500).expect("delta");
        let rows = stock_with_catalog(&conn, "s1").expect("catalog+stock");
        assert_eq!(rows.len(), 2);
        let p1 = rows.iter().find(|(id, _, _)| id == "p1").unwrap();
        assert_eq!(p1.2, 2500);
        let p2 = rows.iter().find(|(id, _, _)| id == "p2").unwrap();
        assert_eq!(p2.2, 0, "без stock-рядка → 0");
    }
}
