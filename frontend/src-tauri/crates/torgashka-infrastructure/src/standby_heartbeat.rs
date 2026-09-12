//! Локальний стан вузла у SQLite каси: присутність device-ів і налаштування
//! `[node]`-сесії (node_server_url / node_node_id / node_node_token / БД).
//!
//! ADR-0008: standby-частину (фоновий heartbeat-цикл у primary і
//! `PUT /api/v1/network-nodes/:id/heartbeat`) видалено разом із самою
//! концепцією репліки (E7, ADR-0008 §8 п.7). Лишилося те, що НЕ було
//! standby-специфічним:
//!   * `record_device_seen` — журнал життєвості device-ів у ЛОКАЛЬНІЙ БД
//!     вузла (SQLite каналу присутності; пише кожен вузол у себе);
//!   * `read_standby_settings` — читання збережених `node_*` налаштувань
//!     (використовує резолв URL локальної копії БД, `node_config`).

use rusqlite::{params, Connection};

use crate::offline::db::OfflineDatabase;
use crate::offline::sync_push;

/// Локальний журнал життєвості device-каси на standby-вузлі (ADR-0007
/// §11.7.9.8, Фаза 3.3b): `UPDATE devices SET last_seen_at` на standby не
/// виконується (репліка read-only) — факт звернення пишемо в SQLite каси.
/// Авторитетне `devices.last_seen_at` пише primary (там же й device-авторизація).
pub fn record_device_seen(device_id: &str) -> Result<(), String> {
    let conn = open_offline_conn()?;
    conn.execute(
        "INSERT INTO device_heartbeats (device_id, last_seen_at) \
         VALUES (?1, datetime('now')) \
         ON CONFLICT (device_id) DO UPDATE SET last_seen_at = datetime('now')",
        rusqlite::params![device_id],
    )
    .map_err(|e| format!("device_heartbeats upsert ({device_id}): {e}"))?;
    Ok(())
}

/// Відкрити SQLite каси (створює каталог даних, доганяє міграції).
fn open_offline_conn() -> Result<Connection, String> {
    let path = OfflineDatabase::default_db_path().map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("каталог даних каси {}: {e}", parent.display()))?;
    }
    sync_push::open_connection(&path).map_err(|e| e.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// SQLite settings standby-вузла (ключі `node_*`, зберігає NodeJoinPage)
// ─────────────────────────────────────────────────────────────────────────────

/// Збережені налаштування вузла з SQLite (settings.key = `node_*`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StandbySettings {
    /// `node_node_id` — uuid вузла на primary.
    pub node_id: Option<String>,
    /// `node_node_token` — токен вузла для хаба (секрет вузла).
    pub node_token: Option<String>,
    /// `node_replication_role` — роль реплікації (`replicator_<short>`).
    pub replication_role: Option<String>,
    /// `node_replication_password` — пароль ролі (plaintext, join віддає раз).
    pub replication_password: Option<String>,
    /// `node_replication_host` — адреса БД-джерела копії (legacy-ключ).
    pub replication_host: Option<String>,
    /// `node_replication_port` — порт primary.
    pub replication_port: Option<u16>,
    /// `node_replication_database` — база на primary (інформаційно).
    pub replication_database: Option<String>,
    /// `node_replication_slot` — replication slot (`standby_<short>`).
    pub replication_slot: Option<String>,
    /// URL primary-API для heartbeat: `node_server_url`, fallback —
    /// `server_url` (legacy sync-налаштування каси; join-екран зберігає
    /// `node_server_url`, див. NodeJoinPage.tsx).
    pub server_url: Option<String>,
}

/// Читає один ключ з таблиці settings (None — ключа немає).
fn read_key(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |r| r.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| {
        if e == rusqlite::Error::QueryReturnedNoRows {
            Ok(None)
        } else {
            Err(format!("SELECT settings.{key}: {e}"))
        }
    })
}

/// Читає standby-налаштування з ВІДКРИТОГО з'єднання. `Ok(None)`, якщо
/// `node_node_id` відсутній (join не виконано) — маркер «вузол не в мережі».
pub fn read_standby_settings_conn(conn: &Connection) -> Result<Option<StandbySettings>, String> {
    let st = StandbySettings {
        node_id: read_key(conn, "node_node_id")?,
        node_token: read_key(conn, "node_node_token")?,
        replication_role: read_key(conn, "node_replication_role")?,
        replication_password: read_key(conn, "node_replication_password")?,
        replication_host: read_key(conn, "node_replication_host")?,
        replication_port: read_key(conn, "node_replication_port")?
            .map(|p| {
                p.parse::<u16>()
                    .map_err(|e| format!("node_replication_port не u16 ('{p}'): {e}"))
            })
            .transpose()?,
        replication_database: read_key(conn, "node_replication_database")?,
        replication_slot: read_key(conn, "node_replication_slot")?,
        server_url: read_key(conn, "node_server_url")?.or(read_key(conn, "server_url")?),
    };
    if st.node_id.is_none() {
        return Ok(None); // join ще не виконано
    }
    Ok(Some(st))
}

/// Читає standby-налаштування зі стандартної SQLite-БД каси.
pub fn read_standby_settings() -> Result<Option<StandbySettings>, String> {
    let path = OfflineDatabase::default_db_path()?;
    let conn = sync_push::open_connection(&path)?;
    read_standby_settings_conn(&conn)
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести (чисті функції: без Tauri, без мережі)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory SQLite з таблицею settings (схема 0001 — key/value).
    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory БД");
        conn.execute_batch(
            "CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );",
        )
        .expect("таблиця settings");
        conn
    }

    fn set(conn: &Connection, key: &str, value: &str) {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .expect("insert settings");
    }

    #[test]
    fn no_node_id_means_join_not_done() {
        let conn = test_conn();
        set(&conn, "server_url", "http://primary:8000");
        let st = read_standby_settings_conn(&conn).expect("читання");
        assert_eq!(st, None, "без node_node_id — join не виконано → None");
    }

    #[test]
    fn reads_all_node_keys_and_falls_back_to_server_url() {
        let conn = test_conn();
        set(
            &conn,
            "node_node_id",
            "11111111-2222-3333-4444-555555555555",
        );
        set(&conn, "node_node_token", "tok-123");
        set(&conn, "node_replication_role", "replicator_ab12");
        set(&conn, "node_replication_password", "p@ss");
        set(&conn, "node_replication_host", "vps.example.com");
        set(&conn, "node_replication_port", "5432");
        set(&conn, "node_replication_database", "torgashka");
        set(&conn, "node_replication_slot", "standby_ab12");
        // node_server_url НЕ збережено → fallback на server_url.
        set(&conn, "server_url", "http://vps.example.com:8000");
        let st = read_standby_settings_conn(&conn)
            .expect("читання")
            .expect("join виконано");
        assert_eq!(
            st.node_id.as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(st.node_token.as_deref(), Some("tok-123"));
        assert_eq!(st.replication_host.as_deref(), Some("vps.example.com"));
        assert_eq!(st.replication_port, Some(5432));
        assert_eq!(
            st.server_url.as_deref(),
            Some("http://vps.example.com:8000")
        );
    }

    #[test]
    fn node_server_url_takes_priority_over_server_url() {
        let conn = test_conn();
        set(
            &conn,
            "node_node_id",
            "11111111-2222-3333-4444-555555555555",
        );
        set(&conn, "node_server_url", "http://node.example.com:8000");
        set(&conn, "server_url", "http://legacy.example.com:8000");
        let st = read_standby_settings_conn(&conn)
            .expect("читання")
            .expect("join виконано");
        assert_eq!(
            st.server_url.as_deref(),
            Some("http://node.example.com:8000")
        );
    }
}
