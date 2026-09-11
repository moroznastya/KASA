//! Snapshot-логіка чека каси (ЕТАП 5 offline-first, дизайн sync-schema-design.md
//! розділ 2.2: items[].price_snapshot / name_snapshot; розділ 6.2: зміна ціни,
//! видалення товару).
//!
//! Обов'язки модуля:
//!   * [`enrich_receipt_with_snapshots`] — перед записом у outbox збагачує
//!     items чека полями `price_snapshot`/`name_snapshot` (ціна/назва з
//!     локального довідника `products_v2` на МОМЕНТ продажу). Якщо item вже
//!     має снапшоти (фронт передав) — не чіпає. Якщо товару в довіднику
//!     немає (ніколи не кешований) — price_snapshot = item.price,
//!     name_snapshot = "Товар видалено".
//!   * [`validate_sale_allowed`] — захист від продажу товару, який сервер
//!     видалив (is_deleted = 1 у локальному довіднику після pull op:delete):
//!     новий чек sale з таким товаром відхиляється. Offline-чек, створений
//!     ДО видалення (вже в outbox зі снапшотом), при цьому push'иться
//!     успішно — сервер приймає його за снапшотом (дизайн 6.2).
//!
//! Товар, видалений на сервері, але ще НЕ позначений is_deleted локально
//! (pull не дійшов) — продається за локальною копією (дизайн 6.2) і чек
//! приймається сервером за снапшотом.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// Ключі ідентифікатора товару в item (фронт може слати product_id | id).
fn item_product_id(item: &Value) -> Option<&str> {
    item["product_id"]
        .as_str()
        .or_else(|| item["id"].as_str())
        .or_else(|| item["productId"].as_str())
}

/// Збагатити чек каси снапшотами назв/цін з локального довідника products_v2.
///
/// Правила (ЕТАП 5, п.1a):
///   * item вже має `price_snapshot` І `name_snapshot` → не чіпаємо;
///   * товар Є в products_v2 (включно з is_deleted=1 — м'яке видалення
///     лишає останню відому копію) → снапшот з цієї копії;
///   * товару в products_v2 НЕМАЄ зовсім → price_snapshot = item.price,
///     name_snapshot = "Товар видалено".
///
/// price_snapshot = ФАКТИЧНА ціна продажу (item.price; ручні знижки касира
/// не спотворюються), fallback — products_v2.price. name_snapshot = назва з
/// products_v2 (навіть is_deleted=1 — остання відома копія), fallback
/// item.name, інакше "Товар видалено". Якщо товару в products_v2 НЕМАЄ —
/// price_snapshot = item.price, name_snapshot = "Товар видалено".
///
/// item.price (ціна продажу) НЕ змінюється — сервер приймає чек як є,
/// виручка рахується за ціною продажу (снапшот — аудит/відновлення).
pub fn enrich_receipt_with_snapshots(
    conn: &Connection,
    receipt_json: &str,
) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(receipt_json)
        .map_err(|e| format!("Чек каси — невалідний JSON: {e}"))?;
    let items = v
        .get_mut("items")
        .and_then(|i| i.as_array_mut())
        .ok_or_else(|| "Чек каси не має items[]".to_string())?;

    for item in items {
        let has_both = item.get("price_snapshot").is_some() && item.get("name_snapshot").is_some();
        if has_both {
            continue;
        }
        // Остання відома копія з довідника (навіть is_deleted=1).
        let mut known_name: Option<String> = None;
        let mut known_price: Option<String> = None;
        if let Some(pid) = item_product_id(item) {
            let row: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT name, CAST(price AS TEXT) FROM products_v2 WHERE id = ?1",
                    params![pid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| format!("SELECT products_v2 (id={pid}): {e}"))?;
            if let Some((name, price)) = row {
                known_name = Some(name);
                known_price = price;
            }
        }

        if item.get("name_snapshot").is_none() {
            let name = known_name
                .or_else(|| item["name"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "Товар видалено".to_string());
            item["name_snapshot"] = Value::String(name);
        }
        if item.get("price_snapshot").is_none() {
            // Фактична ціна продажу (item.price) — перш за все; довідник —
            // fallback (якщо item.price відсутній — аномальний чек).
            let price = match &item["price"] {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => known_price,
            };
            if let Some(p) = price {
                item["price_snapshot"] = Value::String(p);
            } else {
                item["price_snapshot"] = Value::Null;
            }
        }
    }

    serde_json::to_string(&v).map_err(|e| format!("Серіалізація чека: {e}"))
}

/// Відхилити НОВИЙ продаж товару, позначеного видаленим у локальному
/// довіднику (is_deleted = 1, дизайн 6.2). Повернення (return) не блокуються.
pub fn validate_sale_allowed(conn: &Connection, receipt_json: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(receipt_json)
        .map_err(|e| format!("Чек каси — невалідний JSON: {e}"))?;
    if v["receipt_type"].as_str() == Some("return") {
        return Ok(());
    }
    let Some(items) = v.get("items").and_then(|i| i.as_array()) else {
        return Ok(());
    };
    for item in items {
        if let Some(pid) = item_product_id(item) {
            let deleted: Option<i64> = conn
                .query_row(
                    "SELECT is_deleted FROM products_v2 WHERE id = ?1",
                    params![pid],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| format!("SELECT products_v2.is_deleted (id={pid}): {e}"))?;
            if deleted == Some(1) {
                let label = item["name"]
                    .as_str()
                    .or_else(|| item["name_snapshot"].as_str())
                    .unwrap_or(pid);
                return Err(format!(
                    "Товар «{label}» видалено — продаж неможливий (офлайн-чек відхилено)"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::migrations;
    use serde_json::json;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory БД");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("FK");
        migrations::migrate(&conn).expect("міграції");
        conn
    }

    fn seed_product(conn: &Connection, id: &str, name: &str, price: &str, deleted: bool) {
        conn.execute(
            "INSERT INTO products_v2 (id, name, price, is_deleted, server_version, data)
             VALUES (?1, ?2, ?3, ?4, 1, '{}')",
            params![id, name, price, if deleted { 1 } else { 0 }],
        )
        .expect("seed product");
    }

    fn sale_json(items: Value) -> String {
        json!({ "receipt_type": "sale", "items": items, "total_amount": "10.00" }).to_string()
    }

    /// Ціни порівнюємо ЧИСЛОВО: SQLite NUMERIC зберігає 85.00 як 85/85.0
    /// (формат рядка не зберігається) — значення мають збігатися.
    fn price_eq(actual: &Value, expected: &str) -> bool {
        let a = actual.as_str().and_then(|s| s.parse::<f64>().ok());
        let e = expected.parse::<f64>().ok();
        match (a, e) {
            (Some(a), Some(e)) => (a - e).abs() < 1e-9,
            _ => false,
        }
    }

    // ── Збагачення снапшотами ──────────────────────────────────────────────

    /// Товар є в довіднику → item без снапшотів отримує name/price з
    /// products_v2; item.price (ціна продажу) НЕ змінюється.
    #[test]
    fn enrich_adds_snapshots_from_catalog() {
        let conn = test_conn();
        seed_product(&conn, "p-1", "Кава", "85.00", false);
        let receipt = sale_json(json!([{ "product_id": "p-1", "quantity": 1, "price": "85.00" }]));

        let out = enrich_receipt_with_snapshots(&conn, &receipt).expect("enrich");
        let v: Value = serde_json::from_str(&out).unwrap();
        let item = &v["items"][0];
        assert_eq!(item["name_snapshot"], "Кава");
        assert!(
            price_eq(&item["price_snapshot"], "85.00"),
            "снапшот = ціна довідника"
        );
        assert!(
            price_eq(&item["price"], "85.00"),
            "ціна продажу недоторкана"
        );
    }

    /// Зміна ціни (дизайн 6.2): товар у довіднику вже коштує 100, а чек
    /// продає за старою ціною 90 → снапшот фіксує 90 (ціна продажу),
    /// price НЕ перераховується на актуальну ціну довідника.
    #[test]
    fn enrich_keeps_sale_price_not_catalog_price() {
        let conn = test_conn();
        seed_product(&conn, "p-1", "Кава", "100.00", false);
        let receipt = sale_json(json!([{ "product_id": "p-1", "quantity": 2, "price": "90.00" }]));

        let out = enrich_receipt_with_snapshots(&conn, &receipt).expect("enrich");
        let v: Value = serde_json::from_str(&out).unwrap();
        let item = &v["items"][0];
        assert!(
            price_eq(&item["price"], "90.00"),
            "виручка за ціною продажу (снапшот)"
        );
        assert!(
            price_eq(&item["price_snapshot"], "90.00"),
            "снапшот = ціна продажу на момент чека"
        );
        assert_eq!(item["name_snapshot"], "Кава");
    }

    /// Item уже має обидва снапшоти (фронт передав) → не перезаписуються.
    #[test]
    fn enrich_keeps_existing_snapshots() {
        let conn = test_conn();
        seed_product(&conn, "p-1", "Кава", "100.00", false);
        let receipt = sale_json(json!([{
            "product_id": "p-1", "quantity": 1, "price": "95.00",
            "price_snapshot": "95.00", "name_snapshot": "Кава стара назва",
        }]));

        let out = enrich_receipt_with_snapshots(&conn, &receipt).expect("enrich");
        let v: Value = serde_json::from_str(&out).unwrap();
        let item = &v["items"][0];
        assert_eq!(item["name_snapshot"], "Кава стара назва");
        assert_eq!(item["price_snapshot"], "95.00");
    }

    /// Видалений товар (is_deleted=1): снапшот береться з ОСТАННЬОЇ ВІДОМОЇ
    /// копії довідника (дизайн 6.2 — «все одно snapshot з останньої копії»).
    #[test]
    fn enrich_snapshots_from_last_known_copy_when_deleted() {
        let conn = test_conn();
        seed_product(&conn, "p-1", "Кава", "85.00", true);
        let receipt = sale_json(json!([{ "product_id": "p-1", "quantity": 1, "price": "85.00" }]));

        let out = enrich_receipt_with_snapshots(&conn, &receipt).expect("enrich");
        let v: Value = serde_json::from_str(&out).unwrap();
        let item = &v["items"][0];
        assert_eq!(
            item["name_snapshot"], "Кава",
            "копія лишається після видалення"
        );
        assert!(
            price_eq(&item["price_snapshot"], "85.00"),
            "снапшот з останньої копії"
        );
    }

    /// Товару в довіднику НЕМАЄ зовсім → price_snapshot = item.price,
    /// name_snapshot = "Товар видалено".
    #[test]
    fn enrich_fallback_when_product_unknown() {
        let conn = test_conn();
        let receipt =
            sale_json(json!([{ "product_id": "ghost-1", "quantity": 1, "price": "42.00" }]));

        let out = enrich_receipt_with_snapshots(&conn, &receipt).expect("enrich");
        let v: Value = serde_json::from_str(&out).unwrap();
        let item = &v["items"][0];
        assert_eq!(
            item["price_snapshot"], "42.00",
            "fallback: ціна з item.price"
        );
        assert_eq!(item["name_snapshot"], "Товар видалено");
    }

    // ── Валідація продажу видаленого товару ───────────────────────────────

    /// Продаж товару з is_deleted=1 → Err (товар НЕ продається, дизайн 6.2).
    #[test]
    fn sale_of_deleted_product_rejected() {
        let conn = test_conn();
        seed_product(&conn, "p-del", "Кава", "85.00", true);
        let receipt =
            sale_json(json!([{ "product_id": "p-del", "quantity": 1, "price": "85.00" }]));
        let err = validate_sale_allowed(&conn, &receipt).expect_err("має відхилити");
        assert!(err.contains("видалено"), "помилка пояснює: {err}");
    }

    /// Продаж живого товару → Ok.
    #[test]
    fn sale_of_live_product_allowed() {
        let conn = test_conn();
        seed_product(&conn, "p-1", "Кава", "85.00", false);
        let receipt = sale_json(json!([{ "product_id": "p-1", "quantity": 1, "price": "85.00" }]));
        validate_sale_allowed(&conn, &receipt).expect("живий товар — ок");
    }

    /// Повернення видаленого товару → Ok (не блок: раніше проданий товар
    /// повертається навіть якщо виведений з асортименту).
    #[test]
    fn return_of_deleted_product_allowed() {
        let conn = test_conn();
        seed_product(&conn, "p-del", "Кава", "85.00", true);
        let receipt = json!({
            "receipt_type": "return",
            "items": [{ "product_id": "p-del", "quantity": 1, "price": "85.00" }],
            "total_amount": "-85.00",
        })
        .to_string();
        validate_sale_allowed(&conn, &receipt).expect("повернення — ок");
    }

    /// Товар, якого немає в довіднику (ще не кешований), продається —
    /// валідація НЕ блокує (блокує лише явний is_deleted=1).
    #[test]
    fn sale_of_not_yet_cached_product_allowed() {
        let conn = test_conn();
        let receipt =
            sale_json(json!([{ "product_id": "ghost-1", "quantity": 1, "price": "10.00" }]));
        validate_sale_allowed(&conn, &receipt).expect("невідомий товар — ок (продаж за снапшотом)");
    }

    /// Чек без items[] — Err: сміття не потрапляє в outbox (сервер однаково
    /// відхилив би 422; оператор бачить помилку одразу на касі).
    #[test]
    fn enrich_rejects_receipt_without_items() {
        let conn = test_conn();
        let receipt = json!({ "receipt_type": "sale", "total_amount": "0.00" }).to_string();
        let err = enrich_receipt_with_snapshots(&conn, &receipt).expect_err("без items — Err");
        assert!(err.contains("items"), "пояснення: {err}");
    }
}
