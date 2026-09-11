//! Локальний каталог каси — валідація позицій документів ПРИЙМАННЯ
//! (ADR-0007 §5 AT-14).
//!
//! Навіщо окремий модуль: `stock`/`transactions` ведуть КІЛЬКОСТІ, а не
//! довідник. Тут — єдине місце, де питається «чи існує товар у локальному
//! каталозі точки», щоб шляхи створення накладної (`enqueue_invoice`) і
//! повернення постачальнику (`enqueue_transaction`) не заводили власних
//! SELECT-ів.
//!
//! Джерела каталогу (обидва локальні, жодного запиту в мережу):
//!   * `products_v2` (0003) — нормалізована копія серверних товарів, яку
//!     наповнює master-pull (`sync_pull`). ОСНОВНЕ джерело;
//!   * `products` (0001, legacy) — JSON-сховище старих інсталяцій
//!     (`db.rs`-шлях). Враховується, щоб не ламати legacy-касу.
//!
//! `is_deleted = 1` — товар свідомо позначено видаленим на сервері
//! (той самий сигнал, що використовує `snapshots`): приймати його в точку
//! не можна, тому він відрізняється від «відсутній» ЛИШЕ текстом помилки
//! (діагностика), наслідок той самий — відмова + rollback.
//!
//! Межі (свідомі, щоб не «блокувати законне»):
//!   * ЧЕКИ ПРОДАЖУ цією валідацією НЕ проходять: неповний каталог не має
//!     блокувати продаж (§10.3, AT-14 — окремо);
//!   * позиція без `product_id` не валідується: валідувати нічого, а
//!     stock-ефекту така позиція не дає (див. `stock::parse_items`).

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// Стабільний маркер ЛЮДСЬКОЇ відмови валідації каталогу (ADR-0007 §5
/// AT-14). Єдине місце, де він заданий: фасад за ним відрізняє бізнес-відмову
/// (HTTP 400 з текстом) від технічної помилки БД (HTTP 500 із санацією).
pub const CATALOG_REJECTION_MARKER: &str = "локальному каталозі точки";

/// Чи є текст помилки відмовою валідації каталогу (див. маркер вище).
pub fn is_catalog_rejection(message: &str) -> bool {
    message.contains(CATALOG_REJECTION_MARKER)
}

/// Стан товару в ЛОКАЛЬНОМУ каталозі точки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductState {
    /// Знайдено й активний (ім'я — для людських повідомлень/звірки).
    Known { name: String },
    /// Знайдено, але позначено видаленим (`products_v2.is_deleted = 1`).
    Deleted,
    /// Не знайдено ні в `products_v2`, ні в legacy `products`.
    Unknown,
}

/// Стан товару в локальному каталозі (без запитів у мережу).
pub fn product_state(conn: &Connection, product_id: &str) -> Result<ProductState, String> {
    // products_v2 — основне джерело (копія серверного каталогу після pull).
    let v2: Option<(Option<String>, i64)> = conn
        .query_row(
            "SELECT name, is_deleted FROM products_v2 WHERE id = ?1",
            params![product_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("каталог (products_v2, {product_id}): {e}"))?;
    if let Some((name, is_deleted)) = v2 {
        if is_deleted != 0 {
            return Ok(ProductState::Deleted);
        }
        return Ok(ProductState::Known {
            name: name.unwrap_or_else(|| product_id.to_string()),
        });
    }
    // legacy-каталог (0001): data — JSON товару; невалідний JSON → id як ім'я.
    let legacy: Option<String> = conn
        .query_row(
            "SELECT CASE WHEN json_valid(data) THEN COALESCE(json_extract(data, '$.name'), id) \
             ELSE id END FROM products WHERE id = ?1",
            params![product_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| format!("каталог (products, {product_id}): {e}"))?;
    Ok(match legacy {
        Some(name) => ProductState::Known { name },
        None => ProductState::Unknown,
    })
}

/// Ім'я товару з локального каталогу (для звірки/відображення); `None` —
/// товару в каталозі немає.
pub fn catalog_name(conn: &Connection, product_id: &str) -> Result<Option<String>, String> {
    Ok(match product_state(conn, product_id)? {
        ProductState::Known { name } => Some(name),
        _ => None,
    })
}

/// Перевірка КОЖНОЇ позиції документа приймання проти локального каталогу.
///
/// `doc_label` — людська назва документа для повідомлення («накладна»,
/// «повернення постачальнику»). Помилка тексту — людська, без SQL:
/// невідомий товар → відмова з підказкою про синхронізацію довідника.
///
/// Викликається ВСЕРЕДИНІ вже відкритої транзакції агрегата (див.
/// `transactions::enqueue_invoice`/`enqueue_transaction`): каталог читається
/// під тим самим IMMEDIATE-локом, тож перевірене не може «зникнути» до
/// INSERT-ів, а помилка не лишає ні агрегата, ні outbox-запису, ні
/// stock-ефекту.
pub fn ensure_products_known(
    conn: &Connection,
    payload: &Value,
    doc_label: &str,
) -> Result<(), String> {
    let Some(items) = payload.get("items").and_then(|i| i.as_array()) else {
        return Ok(());
    };
    for (idx, item) in items.iter().enumerate() {
        let pid = item
            .get("product_id")
            .or_else(|| item.get("productId"))
            .and_then(|p| p.as_str());
        // Позиція без product_id: валідувати нічого, stock-ефекту вона не дає.
        let Some(pid) = pid else { continue };
        match product_state(conn, pid)? {
            ProductState::Known { .. } => {}
            ProductState::Deleted => {
                return Err(format!(
                    "{doc_label}: товар {pid} (позиція №{}) позначено ВИДАЛЕНИМ \
                     у {CATALOG_REJECTION_MARKER}",
                    idx + 1
                ));
            }
            ProductState::Unknown => {
                return Err(format!(
                    "{doc_label}: товар {pid} (позиція №{}) відсутній у {CATALOG_REJECTION_MARKER} \
                     — довідник синхронізується з primary, повторіть після синхронізації",
                    idx + 1
                ));
            }
        }
    }
    Ok(())
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

    fn put_v2(conn: &Connection, id: &str, name: &str, is_deleted: i64) {
        conn.execute(
            "INSERT INTO products_v2 (id, name, is_deleted, server_version) VALUES (?1, ?2, ?3, 1)",
            params![id, name, is_deleted],
        )
        .expect("products_v2");
    }

    #[test]
    fn product_state_covers_known_deleted_and_unknown() {
        let conn = migrated_conn();
        put_v2(&conn, "p-ok", "Кава Львівська", 0);
        put_v2(&conn, "p-del", "Знятий товар", 1);

        assert_eq!(
            product_state(&conn, "p-ok").unwrap(),
            ProductState::Known {
                name: "Кава Львівська".to_string()
            }
        );
        assert_eq!(product_state(&conn, "p-del").unwrap(), ProductState::Deleted);
        assert_eq!(product_state(&conn, "p-nope").unwrap(), ProductState::Unknown);
        assert_eq!(catalog_name(&conn, "p-ok").unwrap().as_deref(), Some("Кава Львівська"));
        assert_eq!(catalog_name(&conn, "p-nope").unwrap(), None);
    }

    #[test]
    fn legacy_products_table_counts_as_catalog() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO products (id, data, store_id) VALUES (?1, ?2, 's1')",
            params!["legacy-1", json!({"name": "Старий товар"}).to_string()],
        )
        .expect("legacy products");
        assert_eq!(
            product_state(&conn, "legacy-1").unwrap(),
            ProductState::Known {
                name: "Старий товар".to_string()
            }
        );
        // Невалідний JSON у legacy-рядку не валить валідацію — ім'я = id.
        conn.execute(
            "INSERT INTO products (id, data, store_id) VALUES ('legacy-2', 'не json', 's1')",
            [],
        )
        .expect("legacy products 2");
        assert_eq!(
            product_state(&conn, "legacy-2").unwrap(),
            ProductState::Known {
                name: "legacy-2".to_string()
            }
        );
    }

    #[test]
    fn ensure_products_known_passes_known_and_reports_unknown_humanly() {
        let conn = migrated_conn();
        put_v2(&conn, "p-ok", "Кава", 0);
        put_v2(&conn, "p-del", "Знятий товар", 1);

        let ok = json!({"items": [{"product_id": "p-ok", "quantity": "2"}]});
        assert!(ensure_products_known(&conn, &ok, "накладна").is_ok());

        // Позиція без product_id — пропускається (stock-ефекту не дає).
        let без_товару = json!({"items": [{"quantity": "2"}]});
        assert!(ensure_products_known(&conn, &без_товару, "накладна").is_ok());

        let bad = json!({"items": [
            {"product_id": "p-ok", "quantity": "2"},
            {"product_id": "p-ghost", "quantity": "1"}
        ]});
        let err = ensure_products_known(&conn, &bad, "накладна").expect_err("невідомий товар");
        assert!(err.contains("p-ghost"), "повідомлення називає товар: {err}");
        assert!(err.contains("№2"), "повідомлення називає позицію: {err}");
        assert!(err.contains("локальному каталозі"), "людський текст: {err}");
        assert!(!err.contains("SELECT"), "жодного SQL у тексті: {err}");

        let deleted = json!({"items": [{"product_id": "p-del", "quantity": "1"}]});
        let err = ensure_products_known(&conn, &deleted, "накладна").expect_err("видалений");
        assert!(err.contains("ВИДАЛЕНИМ"), "текст про видалення: {err}");
    }

    #[test]
    fn payload_without_items_is_not_blocked() {
        let conn = migrated_conn();
        assert!(ensure_products_known(&conn, &json!({}), "накладна").is_ok());
    }
}
