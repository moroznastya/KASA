//! Фоновий heartbeat-цикл standby-вузла мережі магазинів (рішення Творця,
//! додаток до `network-replication-etap15-20.md`).
//!
//! Після `join` (фронт зберігає `node_*` у SQLite settings) і
//! [`crate::standby_provision::provision_standby`] вузол має РЕГУЛЯРНО
//! сповіщати primary про свою доступність:
//!
//! ```text
//! PUT {server_url}/api/v1/network-nodes/{node_id}/heartbeat
//! Authorization: Bearer {node_token}
//! {"status":"active","app_version":"..."}
//! ```
//!
//! Без heartbeat primary через 7 днів позначить вузол stale і відхилятиме
//! його (ЕТАП 20, `requires_force_resync`). Цикл ідемпотентний (один на
//! процес, AtomicBool — патерн push/pull-циклів `offline/commands.rs`).
//!
//! Модуль не залежить від Tauri-команд: чиста інфраструктура. Старт — з
//! будь-якого контексту (setup `src/lib.rs`, Tauri-команда
//! `start_standby_provision`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::{params, Connection};

use crate::offline::db::OfflineDatabase;
use crate::offline::sync_push;

/// Інтервал heartbeat-циклу (секунди). 60 с — компроміс між свіжістю
/// `last_seen_at` на primary і навантаженням на інтернет-канал standby.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 60;

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

/// Один heartbeat-цикл на процес (idempotent-прапорець).
static STANDBY_HEARTBEAT_STARTED: AtomicBool = AtomicBool::new(false);

// ─────────────────────────────────────────────────────────────────────────────
// SQLite settings standby-вузла (ключі `node_*`, зберігає NodeJoinPage)
// ─────────────────────────────────────────────────────────────────────────────

/// Налаштування standby-вузла з SQLite (settings.key = `node_*`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StandbySettings {
    /// `node_node_id` — uuid вузла на primary.
    pub node_id: Option<String>,
    /// `node_node_token` — Bearer-токен heartbeat (секрет вузла).
    pub node_token: Option<String>,
    /// `node_replication_role` — роль реплікації (`replicator_<short>`).
    pub replication_role: Option<String>,
    /// `node_replication_password` — пароль ролі (plaintext, join віддає раз).
    pub replication_password: Option<String>,
    /// `node_replication_host` — адреса primary для pg_basebackup.
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
// Формування heartbeat-запиту (чиста функція — тестована)
// ─────────────────────────────────────────────────────────────────────────────

/// Конфіг heartbeat-циклу (всі поля обов'язкові, валідовані при старті).
#[derive(Debug, Clone)]
pub struct StandbyHeartbeatConfig {
    pub server_url: String,
    pub node_id: String,
    pub node_token: String,
}

/// Формує (url, Authorization) для heartbeat-запиту standby-вузла.
///
/// - URL: `{server_url}/api/v1/network-nodes/{node_id}/heartbeat` (server_url
///   без trailing `/`);
/// - заголовок: `Bearer {node_token}`.
///
/// Помилка — якщо будь-який параметр порожній (не можна формувати запит).
pub fn build_heartbeat_request(
    server_url: &str,
    node_id: &str,
    node_token: &str,
) -> Result<(String, String), String> {
    let base = server_url.trim().trim_end_matches('/');
    let id = node_id.trim();
    let token = node_token.trim();
    if base.is_empty() {
        return Err("server_url порожній — heartbeat неможливий".to_string());
    }
    if id.is_empty() {
        return Err("node_id порожній — heartbeat неможливий".to_string());
    }
    if token.is_empty() {
        return Err("node_token порожній — heartbeat неможливий".to_string());
    }
    Ok((
        format!("{base}/api/v1/network-nodes/{id}/heartbeat"),
        format!("Bearer {token}"),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Циклічний heartbeat (tokio task, один на процес)
// ─────────────────────────────────────────────────────────────────────────────

/// Запустити фоновий heartbeat-цикл standby, якщо:
///   * налаштування вже збережені (node_node_id + node_node_token +
///     node_server_url/server_url) і
///   * цикл ще не запущено (один на процес).
///
/// Викликається з setup (src/lib.rs) і після успішного провіжину
/// (start_standby_provision). Не-помилка, якщо налаштувань немає: Ok(false),
/// цикл стартує пізніше (після join + провіжину).
pub fn start_standby_heartbeat() -> Result<bool, String> {
    if STANDBY_HEARTBEAT_STARTED.load(Ordering::Relaxed) {
        return Ok(false); // уже запущено
    }
    let Some(st) = read_standby_settings()? else {
        return Ok(false); // join не виконано — цикл пізніше
    };
    let Some(server_url) = st.server_url else {
        return Ok(false); // немає адреси primary-API
    };
    let Some(node_id) = st.node_id else {
        return Ok(false);
    };
    let Some(node_token) = st.node_token else {
        return Ok(false);
    };
    if server_url.trim().is_empty() || node_id.trim().is_empty() || node_token.trim().is_empty() {
        return Ok(false); // неповні налаштування
    }
    if STANDBY_HEARTBEAT_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(false); // хтось уже запустив
    }
    let cfg = StandbyHeartbeatConfig {
        server_url,
        node_id,
        node_token,
    };
    // tauri::async_runtime::spawn гарантує tokio-контекст з будь-якого потоку.
    // drop(JoinHandle) НЕ abort'ить задачу в tokio — цикл живе до завершення
    // процесу; drop лише від'єднує handle (патерн spawn_push_task).
    std::mem::drop(tauri::async_runtime::spawn(async move {
        heartbeat_loop(cfg).await;
    }));
    Ok(true)
}

/// Цикл: кожні HEARTBEAT_INTERVAL_SECS — PUT heartbeat. Помилки мережі
/// логуються (eprintln) і НЕ зупиняють цикл (primary може бути тимчасово
/// недоступний — standby продовжує працювати в локальному режимі).
async fn heartbeat_loop(cfg: StandbyHeartbeatConfig) {
    let client = reqwest::Client::new();
    loop {
        tokio::time::sleep(Duration::from_secs(HEARTBEAT_INTERVAL_SECS)).await;
        match send_heartbeat(&client, &cfg).await {
            Ok(()) => {}
            Err(e) => eprintln!("[standby_heartbeat] {e}"),
        }
    }
}

/// Один heartbeat-запит до primary. Body — мінімальне: status=active +
/// app_version (db_size/lag на standby без прямого доступу до primary
/// не вимірюються; None — як дозволяє контракт heartbeat).
async fn send_heartbeat(
    client: &reqwest::Client,
    cfg: &StandbyHeartbeatConfig,
) -> Result<(), String> {
    let (url, auth_header) =
        build_heartbeat_request(&cfg.server_url, &cfg.node_id, &cfg.node_token)?;
    let body = format!(
        r#"{{"status":"active","app_version":"{}"}}"#,
        env!("CARGO_PKG_VERSION")
    );
    let resp = client
        .put(&url)
        .header(reqwest::header::AUTHORIZATION, auth_header)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("PUT {url}: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let text = resp
        .text()
        .await
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect::<String>();
    Err(format!("PUT {url}: HTTP {status}: {text}"))
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

    #[test]
    fn build_url_and_bearer_header() {
        let (url, auth) = build_heartbeat_request(
            "http://vps.example.com:8000/",
            "11111111-2222-3333-4444-555555555555",
            "tok-123",
        )
        .expect("валідні параметри");
        assert_eq!(
            url,
            "http://vps.example.com:8000/api/v1/network-nodes/11111111-2222-3333-4444-555555555555/heartbeat"
        );
        assert_eq!(auth, "Bearer tok-123");
    }

    #[test]
    fn build_heartbeat_rejects_empty_parts() {
        assert!(build_heartbeat_request("", "id", "tok").is_err());
        assert!(build_heartbeat_request("http://x", " ", "tok").is_err());
        assert!(build_heartbeat_request("http://x", "id", "").is_err());
    }
}
