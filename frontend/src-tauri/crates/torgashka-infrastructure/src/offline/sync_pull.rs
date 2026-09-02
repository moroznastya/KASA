//! Pull-клієнт майстер-даних (ЕТАП 3 offline-first).
//!
//! Дизайн: docs/design/sync-schema-design.md, розділи 5 (порядок pull),
//! 1.4/2.1 (дельти), 7 (міграції).
//!
//! Обов'язки модуля:
//!   * циклічний pull довідників сервера у визначеному порядку
//!     (settings → employees → categories → products → stock_norms →
//!     suppliers), кожна сутність незалежна (помилка не блокує наступні);
//!   * збереження since_version локально в sync_meta (SQLite, міграція 0002);
//!   * застосування дельти ОДНОЮ SQLite-транзакцією: помилка → ROLLBACK,
//!     since_version не просувається, pull повторюється (дизайн 1.4);
//!   * пагінація: поки сервер відповідає has_more=true — повторюємо з
//!     since_version = `to` (кожна сторінка — окрема транзакція);
//!   * op=delete → is_deleted=1 локально (фізичне видалення не
//!     використовується; рядок зникає з продажу, історія зберігається).
//!
//! Майстер-дані пишуться в нормалізовані таблиці міграції 0003
//! (categories, suppliers, employees, stock_norms, products_v2) та
//! settings (ключі сервера; простір `local.*` НЕ зачіпається — дизайн 5).
//! Існуючий JSON-кеш `products` (0001) не змінюється.

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use super::migrations;

/// Порядок pull у межах циклу (дизайн, розділ 5).
pub const ENTITY_ORDER: [&str; 6] = [
    "settings",
    "employees",
    "categories",
    "products",
    "stock_norms",
    "suppliers",
];

/// Інтервал pull за замовчуванням (розділ 5: 30 с у LAN).
pub const DEFAULT_INTERVAL_SECS: u64 = 30;
/// Мінімальний інтервал (розділ 5: мін. 10 с).
pub const MIN_INTERVAL_SECS: u64 = 10;
/// Захист від нескінченного циклу пагінації (порожня каса, багато сторінок).
const MAX_PAGES_PER_ENTITY: u32 = 10_000;

// ─── DTO дельти (серіалізація відповіді GET /api/v1/sync/master) ───────────

#[derive(Debug, Clone, Deserialize)]
pub struct MasterDelta {
    pub entity: String,
    pub since: i64,
    pub to: i64,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Change {
    pub op: String,
    pub id: String,
    pub version: i64,
    pub data: Option<serde_json::Value>,
}

/// Конфігурація циклічного pull (tokio task).
#[derive(Debug, Clone)]
pub struct PullConfig {
    /// Базовий URL сервера (напр. `http://127.0.0.1:8000`).
    pub base_url: String,
    /// JWT access_token (Bearer) — з логіну каси (PIN/пароль).
    pub token: String,
    /// store_id каси (X-Store-Id) — RLS-контур сервера.
    pub store_id: String,
    /// Інтервал циклу pull (сек; нижче MIN_INTERVAL_SECS — обрізається).
    pub interval_secs: u64,
    /// Шлях до SQLite-БД каси.
    pub db_path: std::path::PathBuf,
}

// ─── Відкриття БД ───────────────────────────────────────────────────────────

/// Відкриває SQLite-БД каси з міграціями до актуальної версії (двигун 0001-0003).
pub fn open_connection(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Не вдалося відкрити БД каси: {}", e))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;",
    )
    .map_err(|e| format!("Помилка PRAGMA (WAL/FK): {}", e))?;
    migrations::migrate(&conn)?;
    Ok(conn)
}

// ─── Застосування дельти (атомарно) ─────────────────────────────────────────

/// Застосовує дельту в ОДНІЙ SQLite-транзакції.
///
/// При помилці — ROLLBACK: жоден рядок дельти не застосований, since_version
/// (sync_meta) не просунулась → повторний pull безпечний.
pub fn apply_delta(conn: &mut Connection, delta: &MasterDelta) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("BEGIN (застосування дельти {}): {}", delta.entity, e))?;

    for change in &delta.changes {
        match change.op.as_str() {
            "upsert" => apply_upsert(&tx, &delta.entity, change)?,
            "delete" => apply_delete(&tx, &delta.entity, change)?,
            other => {
                return Err(format!(
                    "невідома операція '{}' у дельті {} (id={})",
                    other, delta.entity, change.id
                ))
            }
        }
    }

    // Версія просувається ЛИШЕ після успішного застосування всіх змін.
    tx.execute(
        "INSERT INTO sync_meta (entity, version) VALUES (?1, ?2)
         ON CONFLICT(entity) DO UPDATE SET version = excluded.version",
        params![delta.entity, delta.to],
    )
    .map_err(|e| format!("оновлення sync_meta ({} → {}): {}", delta.entity, delta.to, e))?;

    tx.commit()
        .map_err(|e| format!("COMMIT (дельти {}): {}", delta.entity, e))
}

/// Обгортка tx.execute → Result<_, String> (rusqlite-помилки не мають
/// From<String>, тож `?` напряму не працює).
fn q(
    tx: &rusqlite::Transaction<'_>,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<usize, String> {
    tx.execute(sql, params)
        .map_err(|e| format!("SQL-помилка: {e}"))
}

fn apply_upsert(
    tx: &rusqlite::Transaction<'_>,
    entity: &str,
    change: &Change,
) -> Result<(), String> {
    let data = change
        .data
        .as_ref()
        .ok_or_else(|| format!("upsert {} без data (id={})", entity, change.id))?;
    let data_str = data.to_string();
    let get_str = |key: &str| -> Result<String, String> {
        data.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("upsert {} (id={}): поле '{}' відсутнє", entity, change.id, key))
    };
    let get_opt_str = |key: &str| -> Option<String> {
        data.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    };

    match entity {
        "categories" => {
            q(tx, 
                "INSERT INTO categories (id, name, parent_id, is_deleted, server_version, data)
                 VALUES (?1, ?2, ?3, 0, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name, parent_id = excluded.parent_id,
                    is_deleted = 0, server_version = excluded.server_version,
                    data = excluded.data",
                params![change.id, get_str("name")?, get_opt_str("parent_id"), change.version, data_str],
            )?;
        }
        "products" => {
            q(tx, 
                "INSERT INTO products_v2 (id, barcode, name, unit, category_id, price, is_deleted, server_version, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    barcode = excluded.barcode, name = excluded.name, unit = excluded.unit,
                    category_id = excluded.category_id, price = excluded.price,
                    is_deleted = 0, server_version = excluded.server_version,
                    data = excluded.data",
                params![
                    change.id,
                    get_opt_str("barcode"),
                    get_str("name")?,
                    get_opt_str("unit"),
                    get_opt_str("category_id"),
                    get_opt_str("price"),
                    change.version,
                    data_str
                ],
            )?;
        }
        "suppliers" => {
            q(tx, 
                "INSERT INTO suppliers (id, name, phone, is_deleted, server_version, data)
                 VALUES (?1, ?2, ?3, 0, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name, phone = excluded.phone,
                    is_deleted = 0, server_version = excluded.server_version,
                    data = excluded.data",
                params![change.id, get_str("name")?, get_opt_str("phone"), change.version, data_str],
            )?;
        }
        "employees" => {
            q(tx, 
                "INSERT INTO employees (id, name, pin_hash, role, is_deleted, server_version, data)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name, pin_hash = excluded.pin_hash, role = excluded.role,
                    is_deleted = 0, server_version = excluded.server_version,
                    data = excluded.data",
                params![change.id, get_str("name")?, get_opt_str("pin_hash"), get_opt_str("role"), change.version, data_str],
            )?;
        }
        "stock_norms" => {
            q(tx, 
                "INSERT INTO stock_norms (product_id, min_qty, max_qty, is_deleted, server_version, data)
                 VALUES (?1, ?2, ?3, 0, ?4, ?5)
                 ON CONFLICT(product_id) DO UPDATE SET
                    min_qty = excluded.min_qty, max_qty = excluded.max_qty,
                    is_deleted = 0, server_version = excluded.server_version,
                    data = excluded.data",
                params![
                    change.id,
                    get_opt_str("min_qty"),
                    get_opt_str("max_qty"),
                    change.version,
                    data_str
                ],
            )?;
        }
        "settings" => {
            // Серверні налаштування → settings(key, value). Простір `local.*`
            // належить касі (дизайн 5) — серверні ключі туди не пишуться.
            let key = get_str("key")?;
            if key.starts_with("local.") {
                return Ok(());
            }
            let value = data.get("value").and_then(|v| v.as_str()).unwrap_or("");
            q(tx, 
                "INSERT INTO settings (key, value, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![key, value],
            )?;
        }
        other => {
            return Err(format!("upsert: невідома сутність '{other}'"));
        }
    }
    Ok(())
}

fn apply_delete(
    tx: &rusqlite::Transaction<'_>,
    entity: &str,
    change: &Change,
) -> Result<(), String> {
    match entity {
        // Рядок позначається is_deleted=1. Якщо каса його ще не бачила
        // (delete до першого upsert) — no-op: товару немає, і не треба.
        "categories" => {
            q(tx, 
                "UPDATE categories SET is_deleted = 1, server_version = ?2 WHERE id = ?1",
                params![change.id, change.version],
            )?;
        }
        "products" => {
            q(tx, 
                "UPDATE products_v2 SET is_deleted = 1, server_version = ?2 WHERE id = ?1",
                params![change.id, change.version],
            )?;
        }
        "suppliers" => {
            q(tx, 
                "UPDATE suppliers SET is_deleted = 1, server_version = ?2 WHERE id = ?1",
                params![change.id, change.version],
            )?;
        }
        "employees" => {
            q(tx, 
                "UPDATE employees SET is_deleted = 1, server_version = ?2 WHERE id = ?1",
                params![change.id, change.version],
            )?;
        }
        "stock_norms" => {
            q(tx, 
                "UPDATE stock_norms SET is_deleted = 1, server_version = ?2 WHERE product_id = ?1",
                params![change.id, change.version],
            )?;
        }
        "settings" => {
            // Серверні налаштування без soft-delete: фізичне видалення ключа.
            let key = change.data.as_ref().and_then(|d| d.get("key")).and_then(|v| v.as_str());
            if let Some(key) = key {
                if !key.starts_with("local.") {
                    q(tx, "DELETE FROM settings WHERE key = ?1", params![key])?;
                }
            }
        }
        other => {
            return Err(format!("delete: невідома сутність '{other}'"));
        }
    }
    Ok(())
}

// ─── HTTP pull однієї сутності ──────────────────────────────────────────────

/// Локальна since_version сутності (0, якщо ще не було pull).
fn local_since(conn: &Connection, entity: &str) -> Result<i64, String> {
    let v: Option<i64> = conn
        .query_row(
            "SELECT version FROM sync_meta WHERE entity = ?1",
            params![entity],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| format!("читання sync_meta({}): {}", entity, e))?;
    Ok(v.unwrap_or(0))
}

/// Тягне одну сутність з сервера з since_version і застосовує всі сторінки.
///
/// Кожна сторінка — окрема транзакція (apply_delta). Повертає останню
/// застосовану версію сутності.
pub async fn pull_entity(
    conn: &mut Connection,
    client: &reqwest::Client,
    cfg: &PullConfig,
    entity: &str,
) -> Result<i64, String> {
    let mut since = local_since(conn, entity)?;
    for _page in 0..MAX_PAGES_PER_ENTITY {
        let url = format!(
            "{}/api/v1/sync/master?entity={}&since_version={}",
            cfg.base_url.trim_end_matches('/'),
            entity,
            since
        );
        let resp = client
            .get(&url)
            .bearer_auth(&cfg.token)
            .header("X-Store-Id", &cfg.store_id)
            .send()
            .await
            .map_err(|e| format!("GET {entity} since={since}: мережа: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "GET {entity} since={since}: HTTP {status}: {}",
                body.chars().take(300).collect::<String>()
            ));
        }
        let delta: MasterDelta = resp
            .json()
            .await
            .map_err(|e| format!("GET {entity}: невалідний JSON дельти: {e}"))?;

        apply_delta(conn, &delta)?;
        since = delta.to;

        if !delta.has_more {
            break;
        }
    }
    Ok(since)
}

/// Один цикл pull: усі сутності у порядку ENTITY_ORDER.
///
/// Кожен запит незалежний: помилка однієї сутності не блокує наступні
/// (дизайн 5). Повертає кількість успішно оновлених сутностей.
pub async fn pull_all(
    db_path: &Path,
    client: &reqwest::Client,
    cfg: &PullConfig,
) -> Result<usize, String> {
    let mut conn = open_connection(db_path)?;
    let mut ok = 0usize;
    for entity in ENTITY_ORDER {
        match pull_entity(&mut conn, client, cfg, entity).await {
            Ok(to) => {
                eprintln!("[sync_pull] {entity}: → v{to}");
                ok += 1;
            }
            Err(e) => {
                // Незалежність: помилка не зупиняє цикл (дизайн 5).
                eprintln!("[sync_pull] {entity}: ПОМИЛКА: {e}");
            }
        }
    }
    Ok(ok)
}

// ─── Циклічний pull (tokio task) ───────────────────────────────────────────

/// Спавнить фоновий циклічний pull (tokio task).
///
/// Інтервал: cfg.interval_secs (мін. MIN_INTERVAL_SECS), дизайн 5 — 30 с.
/// Перший pull виконується одразу після старту. Помилки циклу логуються
/// і не зупиняють таск (наступний тик — нова спроба).
pub fn spawn_pull_task(cfg: PullConfig) -> tokio::task::JoinHandle<()> {
    let interval_secs = cfg.interval_secs.max(MIN_INTERVAL_SECS);
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        // Перший tick — одразу (interval у tokio пропускає нульовий delay лише
        // якщо перший тик спожито; тут ми просто виконуємо pull до циклу).
        loop {
            ticker.tick().await;
            let started = std::time::Instant::now();
            match pull_all(&cfg.db_path, &client, &cfg).await {
                Ok(n) => eprintln!(
                    "[sync_pull] цикл завершено: {n}/{} сутностей за {:.1}с",
                    ENTITY_ORDER.len(),
                    started.elapsed().as_secs_f64()
                ),
                Err(e) => eprintln!("[sync_pull] цикл: помилка: {e}"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// In-memory БД з міграціями до актуальної версії (0003).
    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory БД");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("FK");
        migrations::migrate(&conn).expect("міграції");
        conn
    }

    fn ch(op: &str, id: &str, version: i64, data: Option<serde_json::Value>) -> Change {
        Change {
            op: op.to_string(),
            id: id.to_string(),
            version,
            data,
        }
    }

    fn dlt(entity: &str, to: i64, changes: Vec<Change>) -> MasterDelta {
        MasterDelta {
            entity: entity.to_string(),
            since: 0,
            to,
            has_more: false,
            changes,
        }
    }

    fn local_version(conn: &Connection, entity: &str) -> i64 {
        conn.query_row(
            "SELECT version FROM sync_meta WHERE entity = ?1",
            params![entity],
            |r| r.get(0),
        )
        .optional()
        .expect("sync_meta version")
        .unwrap_or(0)
    }

    #[test]
    fn upsert_persists_and_version_advances() {
        let mut conn = test_conn();
        let delta = dlt(
            "categories",
            5,
            vec![
                ch("upsert", "cat-1", 1, Some(json!({"name": "Напої"}))),
                ch("upsert", "cat-2", 2, Some(json!({"name": "Бакалія"}))),
            ],
        );
        apply_delta(&mut conn, &delta).expect("apply");

        let count: i64 = conn
            .query_row("SELECT count(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "обидві категорії застосовані");
        assert_eq!(local_version(&conn, "categories"), 5, "since_version просунулась");
    }

    #[test]
    fn repeat_apply_does_not_duplicate() {
        let mut conn = test_conn();
        let delta = dlt(
            "products",
            3,
            vec![ch(
                "upsert",
                "prod-1",
                3,
                Some(json!({"name": "Молоко", "barcode": "4820000000000", "price": "45.50"})),
            )],
        );
        apply_delta(&mut conn, &delta).expect("перший pull");
        apply_delta(&mut conn, &delta).expect("повторний pull (retry)");

        let count: i64 = conn
            .query_row("SELECT count(*) FROM products_v2 WHERE id = 'prod-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "повторний pull не дублює рядок");
        assert_eq!(local_version(&conn, "products"), 3);
    }

    #[test]
    fn delete_marks_is_deleted_keeps_row() {
        let mut conn = test_conn();
        // 1) upsert товару
        apply_delta(
            &mut conn,
            &dlt(
                "products",
                1,
                vec![ch("upsert", "p1", 1, Some(json!({"name": "Хліб", "price": "25.00"})))],
            ),
        )
        .expect("upsert");
        // 2) сервер позначив видаленим (soft-delete)
        apply_delta(
            &mut conn,
            &dlt("products", 2, vec![ch("delete", "p1", 2, None)]),
        )
        .expect("delete");

        let (is_deleted, server_version): (i64, i64) = conn
            .query_row(
                "SELECT is_deleted, server_version FROM products_v2 WHERE id = 'p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(is_deleted, 1, "рядок позначено is_deleted=1");
        assert_eq!(server_version, 2);
        assert_eq!(local_version(&conn, "products"), 2);
    }

    #[test]
    fn invalid_delta_rolls_back_version_untouched() {
        let mut conn = test_conn();
        // categories.name NOT NULL — data без "name" → SQL-помилка.
        let bad = dlt("categories", 9, vec![ch("upsert", "cat-x", 7, Some(json!({"parent_id": null})))]);
        let err = apply_delta(&mut conn, &bad).expect_err("дельта з помилкою має впасти");
        assert!(!err.is_empty(), "помилка не порожня");

        let count: i64 = conn
            .query_row("SELECT count(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "ROLLBACK: жоден рядок не застосований");
        assert_eq!(local_version(&conn, "categories"), 0, "since_version не просунулась");

        // Після помилки повторний pull з валідною дельтою працює.
        apply_delta(
            &mut conn,
            &dlt("categories", 9, vec![ch("upsert", "cat-x", 9, Some(json!({"name": "Ок"})))]),
        )
        .expect("повторний pull після помилки");
        assert_eq!(local_version(&conn, "categories"), 9);
    }

    #[test]
    fn settings_local_prefix_not_overwritten() {
        let mut conn = test_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('local.store_id', 'casa-1')",
            [],
        )
        .unwrap();

        // Серверна дельта намагається перезаписати local.* — має ігноруватись.
        apply_delta(
            &mut conn,
            &dlt(
                "settings",
                4,
                vec![ch(
                    "upsert",
                    "srv-row",
                    3,
                    Some(json!({"key": "local.store_id", "value": "ЗЛАМАНО"})),
                )],
            ),
        )
        .expect("apply");
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'local.store_id'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "casa-1", "local.* простір каси не зачіпається серверним pull");

        // Звичайний серверний ключ пишеться.
        apply_delta(
            &mut conn,
            &dlt(
                "settings",
                6,
                vec![ch("upsert", "srv-row", 5, Some(json!({"key": "company_name", "value": "ФОП Тест"})))],
            ),
        )
        .expect("apply server key");
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'company_name'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "ФОП Тест");
    }

    #[test]
    fn multi_page_delta_advances_in_steps() {
        let mut conn = test_conn();
        // Сторінка 1: 2 рядки, has_more=true, to=500.
        let mut page1 = dlt("products", 500, vec![ch("upsert", "p1", 400, Some(json!({"name": "A"})))]);
        page1.has_more = true;
        apply_delta(&mut conn, &page1).expect("page1");
        assert_eq!(local_version(&conn, "products"), 500);
        // Сторінка 2: to=900.
        let page2 = dlt("products", 900, vec![ch("upsert", "p2", 501, Some(json!({"name": "B"})))]);
        apply_delta(&mut conn, &page2).expect("page2");
        assert_eq!(local_version(&conn, "products"), 900);
        let count: i64 = conn
            .query_row("SELECT count(*) FROM products_v2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
