//! NodeConfig — конфігурація режиму роботи фасаду на вузлі мережі магазинів
//! (ЕТАП 18, `network-replication-etap15-20.md`; підсумок: локальний роутинг
//! читання/запису для standby-вузла).
//!
//! Фасад каси може працювати у двох режимах:
//!
//! - [`NodeMode::Primary`] — каса = незалежна БД (звичайний режим, повна
//!   зворотна сумісність: db_sources.toml БЕЗ секції `[node]` → Primary);
//! - [`NodeMode::Standby`] — каса = репліка primary (embedded PostgreSQL на
//!   порту [`DEFAULT_LOCAL_PG_PORT`]=5433, створена `standby_provision` ЕТАП 16).
//!   Коли primary недоступний і `degrade_to_local=true` — читання йдуть з
//!   локальної репліки, записи — у SQLite-чергу (`offline/*`, ЕТАП 3-5).
//!
//! Конфігурація читається з `db_sources.toml`, секція `[node]`:
//!
//! ```toml
//! [node]
//! mode = "standby"            # "primary" (default) | "standby"
//! local_port = 5433           # порт локальної репліки (default 5433)
//! primary_db_url = "postgresql://..."  # опційно; default — активне джерело
//! degrade_to_local = true     # дозволити локальний режим (default true)
//! ```
//!
//! Відсутність файлу/секції → [`NodeConfig::default`] (Primary) — фасад
//! стартує як раніше, без жодних змін поведінки.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::offline::db::OfflineDatabase;

/// Порт локальної embedded PostgreSQL репліки (співпадає з
/// `embedded_pg::EMBEDDED_PG_PORT`).
pub const DEFAULT_LOCAL_PG_PORT: u16 = 5433;
/// Таймаут TCP-перевірки доступності primary (вимога контракту: 1-2 с).
pub const PRIMARY_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Режим роботи вузла.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NodeMode {
    /// Незалежна БД (звичайна каса / сервер магазину). Поведінка — як раніше.
    #[default]
    Primary,
    /// Репліка primary (embedded PG, порт local_port): локальний роутинг.
    Standby,
}

/// Позначка «repoint primary» (ЕТАП 19, `POST /api/v1/local/repoint-primary`).
///
/// Оператор вказав НОВИЙ primary (host:port); фактичний `pg_basebackup`
/// виконується вручну (MVP) — позначка лишається в конфізі як нагадування
/// та історія. Не впливає на режим роботи фасаду.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepointPending {
    /// Новий host primary (як вказав оператор).
    pub new_primary_host: String,
    /// Новий port primary.
    pub new_primary_port: u16,
    /// Час запиту, RFC3339 UTC.
    pub requested_at: String,
}

/// Конфігурація вузла з секції `[node]` db_sources.toml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Режим роботи. Дефолт — Primary.
    #[serde(default)]
    pub mode: NodeMode,
    /// Порт локальної репліки (використовується у режимі Standby).
    #[serde(default = "default_local_port")]
    pub local_port: u16,
    /// URL primary (куди push-иться черга). `None` → активне джерело
    /// db_sources.toml ([`crate::db_sources::active_source_url`]).
    #[serde(default)]
    pub primary_db_url: Option<String>,
    /// Дозволити деградацію в локальний режим, коли primary недоступний.
    #[serde(default = "default_degrade")]
    pub degrade_to_local: bool,
    /// Позначка «новий primary вказано вручну» (див. [`RepointPending`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repoint_pending: Option<RepointPending>,
}

fn default_local_port() -> u16 {
    DEFAULT_LOCAL_PG_PORT
}

fn default_degrade() -> bool {
    true
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            mode: NodeMode::Primary,
            local_port: DEFAULT_LOCAL_PG_PORT,
            primary_db_url: None,
            degrade_to_local: true,
            repoint_pending: None,
        }
    }
}

impl NodeConfig {
    /// Завантажує конфігурацію з db_sources.toml (той самий шлях, що й
    /// db_sources: env `TORGASHKA_DB_SOURCES` → CWD → manifest).
    ///
    /// Файлу/секції `[node]` немає → `Ok(default)` (Primary). Помилка читання/
    /// парсингу → `Ok(default)` + eprintln (stability_first: вузол НЕ падає
    /// через зіпсовану опційну секцію; primary-режим безпечний завжди).
    pub fn load() -> Self {
        let Some(path) = crate::db_sources::existing_path() else {
            return Self::default();
        };
        Self::load_from_path(&path)
    }

    /// Читає конфігурацію з КОНКРЕТНОГО файлу (env/CWD-незалежно — для
    /// recovery і тестів). Файлу немає/помилка → `Ok(default)` (Primary) +
    /// eprintln (stability_first: вузол не падає через опційну секцію).
    pub fn load_from_path(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => Self::load_from_str(&content),
            Err(e) => {
                eprintln!(
                    "[torgashka-infrastructure] node_config: {} не читається ({e}) — режим Primary",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Парсинг конфігурації з toml-тексту (для тестів і CLI).
    pub fn load_from_str(content: &str) -> Self {
        let v: toml::Value = match toml::from_str(content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[torgashka-infrastructure] node_config: db_sources.toml не парситься ({e}) — режим Primary"
                );
                return Self::default();
            }
        };
        let Some(node) = v.get("node") else {
            return Self::default();
        };
        let cfg: NodeConfig = match node.clone().try_into() {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "[torgashka-infrastructure] node_config: секція [node] некоректна ({e}) — режим Primary"
                );
                Self::default()
            }
        };
        cfg
    }

    /// Чи вузол працює у standby-режимі.
    pub fn is_standby(&self) -> bool {
        self.mode == NodeMode::Standby
    }

    /// URL primary для синхронізації: явний `primary_db_url` → інакше активне
    /// джерело db_sources.toml (той самий механізм, що й db.rs
    /// `resolve_database_url`).
    pub fn resolve_primary_db_url(&self) -> Option<String> {
        if let Some(u) = self
            .primary_db_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(u.to_string());
        }
        crate::db_sources::active_source_url().ok().flatten()
    }

    /// Копія зі зміненим режимом.
    pub fn with_mode(mut self, mode: NodeMode) -> Self {
        self.mode = mode;
        self
    }

    /// Перехід у режим Primary (ЕТАП 19, promote): цей вузол стає джерелом
    /// істини — `primary_db_url` очищується (більше НЕМАЄ primary: push-черги
    /// та деградація вимкнені, щоб не лити дані в чужий/мертвий сервер).
    /// Позначка `repoint_pending` зберігається (історія).
    pub fn into_promoted_primary(mut self) -> Self {
        self.mode = NodeMode::Primary;
        self.primary_db_url = None;
        self
    }

    /// Зберігає секцію `[node]` у db_sources.toml (round-trip: секції
    /// `[active]`/`[sources.*]` та інші зберігаються як є — на відміну від
    /// `db_sources::save`, який знає лише свої секції). Атомарний запис,
    /// права 0600 (unix). Файлу немає → створюється з однією секцією `[node]`.
    pub fn save_to_disk(&self) -> Result<PathBuf, String> {
        self.save_to_path(&crate::db_sources::write_path())
    }

    /// Те саме, але у КОНКРЕТНИЙ файл (recovery/тести): атомарний запис,
    /// права 0600 (unix), решта секцій файлу зберігаються як є.
    pub fn save_to_path(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        // файлу ще немає — починаємо з [node]
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let mut root: toml::Value = if content.trim().is_empty() {
            toml::Value::Table(toml::map::Map::new())
        } else {
            toml::from_str(&content).map_err(|e| format!("db_sources.toml не парситься: {e}"))?
        };
        let node_value = toml::Value::try_from(self.clone())
            .map_err(|e| format!("секція [node] не серіалізується: {e}"))?;
        root.as_table_mut()
            .ok_or_else(|| "корінь db_sources.toml не таблиця".to_string())?
            .insert("node".to_string(), node_value);
        let text = toml::to_string_pretty(&root).map_err(|e| e.to_string())?;
        let tmp = path.with_extension(format!("toml.tmp{}", std::process::id()));
        std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(path.to_path_buf())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Recovery standby-режиму після рестарту (дефект 3)
// ─────────────────────────────────────────────────────────────────────────────
//
// `NodeConfig::save_to_disk()` викликається ЛИШЕ в
// `start_standby_provision` (src/commands/standby.rs) — тобто тільки ПІСЛЯ
// успішного провіжингу. Якщо провіжн обірвався (немає кроків 6-7) або
// застосунок перезапустили ДО запису — `[node] mode="standby"` у
// db_sources.toml відсутній, `NodeConfig::load()` віддає Primary, і каса
// втрачає standby-режим НАЗАВЖДИ (немає жодного іншого шляху відновлення).
//
// Recovery відновлює режим за НАЯВНИМИ слідами, без здогадок:
//   * PG_VERSION у `data_dir_default()` — провіжн реально виконано
//     (pg_basebackup створив кластер); файлу немає → no-op, НІЧОГО не
//     чіпаємо і НЕ робимо повторний pg_basebackup;
//   * `node_node_id` у SQLite settings (join виконано) АБО `[node]` уже
//     standby — підстава вважати вузол standby-касою;
//   * обидві умови виконано → у СТАБІЛЬНИЙ db_sources.toml (поряд із pgdata;
//     не в CWD — див. дефект 4) дописується `mode = "standby"` зі збереженням
//     решти полів/секцій.

/// Recovery standby-режиму вузла (ідемпотентно; прод-обгортка).
///
/// Повертає `Ok(true)`, якщо вузол — standby (уже був або щойно відновлено),
/// `Ok(false)` — якщо підстав немає (провіжн не виконано / join не виконано).
/// Помилки файлових операцій → `Err` (виклик при старті логує, не падає).
pub fn recover_standby_if_needed() -> Result<bool, String> {
    let data_dir = crate::embedded_pg::data_dir_default();
    let join_done = join_performed();
    // Запис — у СТАБІЛЬНИЙ шлях (env → поряд із pgdata → CWD → manifest).
    // Саме в CWD-залежності був дефект 4: писати в «наявний» файл небезпечно —
    // на проді CWD нестабільний, тож туди потрапив би випадковий/застарілий
    // файл, а не конфіг каси.
    let cfg_path = crate::db_sources::write_path();
    recover_standby_at(join_done, &data_dir, &cfg_path)
}

/// Ядро recovery — чиста функція (тестована без env/SQLite):
/// `join_done` — чи є `node_node_id` у SQLite settings; `data_dir` — каталог
/// кластера (PG_VERSION = провіжн виконано); `cfg_path` — файл db_sources.toml.
pub fn recover_standby_at(
    join_done: bool,
    data_dir: &Path,
    cfg_path: &Path,
) -> Result<bool, String> {
    if !data_dir.join("PG_VERSION").is_file() {
        // Провіжн не виконано (немає кластера) — не здогадуємось, нічого
        // не створюємо: вторинний pg_basebackup НЕ запускаємо.
        return Ok(false);
    }
    let cfg = if cfg_path.is_file() {
        load_quiet(cfg_path)
    } else {
        NodeConfig::default()
    };
    if cfg.is_standby() {
        return Ok(true); // режим уже standby — у файл не пишемо (ідемпотентно)
    }
    if !join_done {
        // Кластер є, але слідів join немає — режим вузла невідомий, не вгадуємо.
        return Ok(false);
    }
    cfg.with_mode(NodeMode::Standby).save_to_path(cfg_path)?;
    Ok(true)
}

/// Читання конфіга без eprintln-шуму (файл перевірено на існування).
fn load_quiet(path: &Path) -> NodeConfig {
    match std::fs::read_to_string(path) {
        Ok(content) => NodeConfig::load_from_str(&content),
        Err(_) => NodeConfig::default(),
    }
}

/// Чи виконано join вузла: `node_node_id` у SQLite settings каси.
///
/// Без побічних ефектів: якщо файлу SQLite ще немає — join точно не виконано
/// (БД каси не створюємо). Будь-яка помилка читання → `false` + eprintln
/// (stability_first: старт не падає).
fn join_performed() -> bool {
    let path = match OfflineDatabase::default_db_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[node_config] recovery: шлях SQLite каси невідомий ({e}) — join вважаємо невиконаним");
            return false;
        }
    };
    if !path.is_file() {
        return false;
    }
    match crate::standby_heartbeat::read_standby_settings() {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(e) => {
            eprintln!(
                "[node_config] recovery: SQLite settings не читаються ({e}) — join вважаємо невиконаним"
            );
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Доступність primary (ЕТАП 18.B)
// ─────────────────────────────────────────────────────────────────────────────

/// Парсить host:port з `postgresql://[user[:pass]@]host[:port]/db` —
/// мінімальний парсер без зовнішніх крейтів (URL генеруються
/// db_sources/embedded_pg: host — IP або ім'я, без path-квотування).
pub fn primary_endpoint(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("postgresql://")
        .or_else(|| url.strip_prefix("postgres://"))?;
    // Відкинути userinfo до '@'.
    let after_at = match rest.find('@') {
        Some(i) => &rest[i + 1..],
        None => rest,
    };
    // host:port до '/' (db) або '?' (query).
    let hostport = after_at.split(['/', '?']).next().unwrap_or(after_at);
    if hostport.is_empty() {
        return None;
    }
    // IPv6: [::1]:5432.
    if let Some(rest6) = hostport.strip_prefix('[') {
        let (host6, after) = rest6.split_once(']')?;
        let port = after
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(5432);
        return Some((format!("[{host6}]"), port));
    }
    match hostport.rsplit_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse::<u16>().ok()?)),
        None => Some((hostport.to_string(), 5432)),
    }
}

/// Перевірка доступності primary: TCP-конект до host:port з таймаутом
/// [`PRIMARY_CHECK_TIMEOUT`]. Мережа «є», якщо порт приймає з'єднання —
/// подальша автентифікація/каталожні запити робляться звичайним шляхом.
pub async fn primary_reachable(url: &str) -> bool {
    let Some((host, port)) = primary_endpoint(url) else {
        return false;
    };
    matches!(
        tokio::time::timeout(
            PRIMARY_CHECK_TIMEOUT,
            tokio::net::TcpStream::connect((host.as_str(), port)),
        )
        .await,
        Ok(Ok(_))
    )
}

/// URL ЛОКАЛЬНОЇ репліки: той самий user/password/db, що й primary, але
/// host → 127.0.0.1, port → `local_port`. Використовується фасадом для
/// пулу читання на standby-вузлі (replіка слухає лише localhost).
pub fn local_db_url(primary_url: &str, local_port: u16) -> Option<String> {
    let prefix = if primary_url.starts_with("postgresql://") {
        "postgresql://"
    } else if primary_url.starts_with("postgres://") {
        "postgres://"
    } else {
        return None;
    };
    let rest = &primary_url[prefix.len()..];
    // userinfo (до '@') зберігаємо як є.
    let (userinfo, after) = match rest.find('@') {
        Some(i) => (&rest[..=i], &rest[i + 1..]),
        None => ("", rest),
    };
    // db-частина: '/' або '?' або кінець.
    let cut = after.find(['/', '?']).unwrap_or(after.len());
    let dbpart = &after[cut..];
    Some(format!("{prefix}{userinfo}127.0.0.1:{local_port}{dbpart}"))
}

/// URL локальної репліки БЕЗ пароля: `postgresql://<user>@127.0.0.1:<port>/<db>`.
///
/// Використовується як `DATABASE_URL` у standby-режимі: локальна репліка
/// слухає лише localhost (pg_hba локального кластера — trust), а пароль у
/// URL шкідливий — psql/sqlx тоді намагаються автентифікуватись паролем і
/// можуть застигнути на запиті пароля (дефект 5: GUI-процес без консолі).
pub fn local_readonly_url(primary_url: &str, local_port: u16) -> Option<String> {
    Some(strip_password(&local_db_url(primary_url, local_port)?))
}

/// Прибирає `:<password>` з userinfo URL: `postgresql://u:p@h/db` →
/// `postgresql://u@h/db`. URL без '@' або без ':' у userinfo — без змін.
pub fn strip_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://").map(|i| i + 3) else {
        return url.to_string();
    };
    let head = &url[..scheme_end];
    let rest = &url[scheme_end..];
    // '@' шукаємо ПІСЛЯ схеми; ':' у userinfo — розділювач пароля.
    let Some(at) = rest.find('@') else {
        return url.to_string();
    };
    let userinfo = &rest[..at];
    let tail = &rest[at..];
    match userinfo.find(':') {
        Some(c) => format!("{head}{}{tail}", &userinfo[..c]),
        None => url.to_string(),
    }
}

/// Дефолтний локальний URL, якщо primary URL не резолвиться: postgres-дефолт
/// на 127.0.0.1:local_port (бд/user беруться з env/embedded pg дефолтів).
pub fn fallback_local_url(local_port: u16, db: &str, user: &str) -> String {
    format!("postgresql://{user}@127.0.0.1:{local_port}/{db}")
}

/// Переписує host:port у `postgresql://` URL, зберігаючи user/password/db.
/// Використовується repoint-primary (ЕТАП 19): новий primary — той самий
/// кластер-джерело з іншої адреси, креденшалі/БД не змінюються.
pub fn rewrite_host_port(url: &str, host: &str, port: u16) -> Option<String> {
    let prefix = if url.starts_with("postgresql://") {
        "postgresql://"
    } else if url.starts_with("postgres://") {
        "postgres://"
    } else {
        return None;
    };
    let rest = &url[prefix.len()..];
    let (userinfo, after) = match rest.find('@') {
        Some(i) => (&rest[..=i], &rest[i + 1..]),
        None => ("", rest),
    };
    let cut = after.find(['/', '?']).unwrap_or(after.len());
    let dbpart = &after[cut..];
    Some(format!("{prefix}{userinfo}{host}:{port}{dbpart}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_primary_backward_compatible() {
        let cfg = NodeConfig::default();
        assert_eq!(cfg.mode, NodeMode::Primary);
        assert!(cfg.degrade_to_local);
        assert_eq!(cfg.local_port, 5433);
        assert!(!cfg.is_standby());
    }

    #[test]
    fn empty_content_is_primary() {
        assert_eq!(NodeConfig::load_from_str(""), NodeConfig::default());
        assert_eq!(
            NodeConfig::load_from_str("active = \"main\"\n[sources.main]\nhost=\"h\"\n"),
            NodeConfig::default()
        );
    }

    #[test]
    fn parses_standby_section() {
        let cfg = NodeConfig::load_from_str(
            r#"
active = "main"
[sources.main]
host = "10.0.0.5"
port = 5432
database = "pos"
user = "postgres"

[node]
mode = "standby"
local_port = 5433
degrade_to_local = true
"#,
        );
        assert!(cfg.is_standby());
        assert_eq!(cfg.local_port, 5433);
        assert!(cfg.degrade_to_local);
        assert_eq!(cfg.primary_db_url, None);
    }

    #[test]
    fn parses_primary_db_url_override() {
        let cfg = NodeConfig::load_from_str(
            r#"
[node]
mode = "standby"
primary_db_url = "postgresql://u:p@10.0.0.9:5432/pos"
degrade_to_local = false
"#,
        );
        assert!(cfg.is_standby());
        assert!(!cfg.degrade_to_local);
        assert_eq!(
            cfg.primary_db_url.as_deref(),
            Some("postgresql://u:p@10.0.0.9:5432/pos")
        );
    }

    #[test]
    fn unknown_mode_falls_back_to_primary() {
        let cfg = NodeConfig::load_from_str("[node]\nmode = \"bogus\"\n");
        assert_eq!(
            cfg.mode,
            NodeMode::Primary,
            "невідомий mode — Primary (безпечно)"
        );
    }

    #[test]
    fn endpoint_parses_host_port() {
        assert_eq!(
            primary_endpoint("postgresql://postgres@10.0.0.5:5432/pos").unwrap(),
            ("10.0.0.5".to_string(), 5432)
        );
        assert_eq!(
            primary_endpoint("postgresql://u:pw@db.example.com/pos").unwrap(),
            ("db.example.com".to_string(), 5432)
        );
        assert_eq!(
            primary_endpoint("postgres://u@[::1]:5433/x").unwrap(),
            ("[::1]".to_string(), 5433)
        );
        assert_eq!(primary_endpoint("http://x"), None);
    }

    #[test]
    fn local_url_rewrites_host_port_keeps_creds_db() {
        let url = "postgresql://postgres:secret@10.0.0.5:5432/pos?sslmode=disable";
        assert_eq!(
            local_db_url(url, 5433).unwrap(),
            "postgresql://postgres:secret@127.0.0.1:5433/pos?sslmode=disable"
        );
        let no_pass = local_db_url("postgresql://postgres@10.0.0.5/pos", 5433).unwrap();
        assert_eq!(no_pass, "postgresql://postgres@127.0.0.1:5433/pos");
    }

    #[test]
    fn primary_reachable_negative_is_fast() {
        // Порт 1 на localhost майже ніколи не відкритий; перевірка — повертає
        // false без зависання (таймаут 2с усередині).
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ok = rt.block_on(primary_reachable("postgresql://u@127.0.0.1:1/x"));
        assert!(!ok);
    }

    #[test]
    fn rewrite_host_port_keeps_creds_swaps_endpoint() {
        let url = "postgresql://replicator:p%40ss@10.0.0.9:5432/torgashka?sslmode=disable";
        assert_eq!(
            rewrite_host_port(url, "192.168.1.50", 5432).unwrap(),
            "postgresql://replicator:p%40ss@192.168.1.50:5432/torgashka?sslmode=disable"
        );
        // Без userinfo.
        assert_eq!(
            rewrite_host_port("postgres://10.0.0.9:5433/db", "new", 5555).unwrap(),
            "postgres://new:5555/db"
        );
        assert_eq!(rewrite_host_port("http://x", "h", 1), None);
    }

    #[test]
    fn promoted_primary_clears_primary_ref_keeps_pending() {
        let cfg = NodeConfig {
            mode: NodeMode::Standby,
            local_port: 5433,
            primary_db_url: Some("postgresql://u@10.0.0.5:5432/pos".into()),
            degrade_to_local: true,
            repoint_pending: Some(RepointPending {
                new_primary_host: "10.0.0.9".into(),
                new_primary_port: 5432,
                requested_at: "2026-09-10T00:00:00Z".into(),
            }),
        };
        let promoted = cfg.clone().into_promoted_primary();
        assert_eq!(promoted.mode, NodeMode::Primary);
        assert_eq!(promoted.primary_db_url, None, "primary-посилання очищено");
        assert!(promoted.repoint_pending.is_some(), "позначка зберігається");
        assert_eq!(cfg.mode, NodeMode::Standby, "оригінал не мутується");
    }

    // ─────────────────────────────────────────────────────────────────────
    // Recovery standby-режиму (дефект 3) + AppData-конфіг (дефект 4)
    // ─────────────────────────────────────────────────────────────────────

    /// Хелпер: тимчасовий каталог + шляхи data_dir / db_sources.toml.
    fn tmp_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("Torgashka").join("pgdata");
        let cfg_path = dir.path().join("Torgashka").join("db_sources.toml");
        std::fs::create_dir_all(&data_dir).expect("mkdir data_dir");
        (dir, data_dir, cfg_path)
    }

    /// (б) Recovery пише `[node] mode="standby"` ЛИШЕ за наявності PG_VERSION.
    #[test]
    fn recovery_writes_standby_only_when_pg_version_present() {
        let (_d, data_dir, cfg_path) = tmp_paths();

        // PG_VERSION немає (провіжн не виконано) → no-op, файл не створюється.
        assert_eq!(recover_standby_at(true, &data_dir, &cfg_path), Ok(false));
        assert!(!cfg_path.exists(), "без PG_VERSION файл не чіпаємо");

        // PG_VERSION є + join виконано → [node] mode="standby" записано.
        std::fs::write(data_dir.join("PG_VERSION"), "17").expect("PG_VERSION");
        assert_eq!(recover_standby_at(true, &data_dir, &cfg_path), Ok(true));
        let raw = std::fs::read_to_string(&cfg_path).expect("read cfg");
        assert!(raw.contains("mode = \"standby\""), "raw: {raw}");
        assert!(NodeConfig::load_from_path(&cfg_path).is_standby());

        // Ідемпотентно: режим уже standby → true, повторного запису не робимо
        // (файл не переписується — mtime не змінюється).
        let mtime_before = std::fs::metadata(&cfg_path)
            .expect("meta")
            .modified()
            .expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(recover_standby_at(true, &data_dir, &cfg_path), Ok(true));
        let mtime_after = std::fs::metadata(&cfg_path)
            .expect("meta")
            .modified()
            .expect("mtime");
        assert_eq!(
            mtime_before, mtime_after,
            "standby-конфіг не переписується даремно"
        );

        // PG_VERSION є, але join НЕ виконано і режим не standby → no-op.
        let cfg_other = data_dir.parent().unwrap().join("other.toml");
        assert_eq!(recover_standby_at(false, &data_dir, &cfg_other), Ok(false));
        assert!(!cfg_other.exists(), "без слідів join не вгадуємо режим");
    }

    /// Recovery зберігає решту полів `[node]` і секції файлу (формат не змінюємо).
    #[test]
    fn recovery_preserves_existing_fields_and_sections() {
        let (_d, data_dir, cfg_path) = tmp_paths();
        std::fs::write(data_dir.join("PG_VERSION"), "17").expect("PG_VERSION");
        std::fs::create_dir_all(cfg_path.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &cfg_path,
            "active = \"primary\"\n\n[sources.primary]\nlabel = \"Основна\"\nhost = \"10.0.0.1\"\nport = 5432\ndatabase = \"pos_system\"\nuser = \"postgres\"\n\n[node]\nmode = \"primary\"\nlocal_port = 5433\nprimary_db_url = \"postgresql://10.0.0.1:5432/pos_system\"\ndegrade_to_local = true\n",
        )
        .expect("write cfg");

        assert_eq!(recover_standby_at(true, &data_dir, &cfg_path), Ok(true));
        let raw = std::fs::read_to_string(&cfg_path).expect("read cfg");
        assert!(raw.contains("mode = \"standby\""), "raw: {raw}");
        assert!(raw.contains("[sources.primary]"), "секції збережено: {raw}");
        assert!(raw.contains("10.0.0.1"), "дані джерела збережено: {raw}");
        let cfg = NodeConfig::load_from_path(&cfg_path);
        assert!(cfg.is_standby());
        assert_eq!(
            cfg.primary_db_url.as_deref(),
            Some("postgresql://10.0.0.1:5432/pos_system"),
            "primary_db_url не загублено"
        );
        assert!(cfg.degrade_to_local);
    }

    /// (в) `NodeConfig::load()`-частина: читання AppData-файлу (не CWD).
    #[test]
    fn load_reads_config_from_appdata_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Імітація %APPDATA%\Torgashka\db_sources.toml
        let appdata_cfg = dir.path().join("Torgashka").join("db_sources.toml");
        std::fs::create_dir_all(appdata_cfg.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &appdata_cfg,
            "[node]\nmode = \"standby\"\nlocal_port = 5433\n",
        )
        .expect("write");

        // Кандидати як у проді: стабільний (AppData) → CWD → manifest.
        let cands = crate::db_sources::path_candidates_with(
            None,
            Some(&appdata_cfg),
            Path::new("db_sources.toml"),
            Path::new("/repo/frontend/src-tauri/db_sources.toml"),
        );
        let chosen = crate::db_sources::existing_path_in(&cands).expect("AppData-файл знайдено");
        assert_eq!(chosen, appdata_cfg);

        let cfg = NodeConfig::load_from_path(&chosen);
        assert!(cfg.is_standby(), "load() з AppData → standby");
        assert_eq!(cfg.local_port, 5433);
    }

    /// E2E: реальні `data_dir_default()`/`write_path()`/`load()`.
    /// Запуск: `XDG_DATA_HOME=<tmp> cargo test -p torgashka-infrastructure --lib
    /// e2e_recovery -- --ignored --nocapture` (окремий процес — щоб env не
    /// вплинув на інші тести).
    #[test]
    #[ignore = "потребує env XDG_DATA_HOME на тимчасовий каталог (див. doc)"]
    fn e2e_recovery_writes_stable_config_and_load_sees_standby() {
        let data_dir = crate::embedded_pg::data_dir_default();
        let cfg_path = crate::db_sources::write_path();
        assert_eq!(
            cfg_path,
            crate::db_sources::stable_config_path().expect("стабільний шлях"),
            "запис іде у стабільний каталог (поряд із pgdata), не в CWD"
        );
        std::fs::create_dir_all(&data_dir).expect("mkdir data_dir");
        std::fs::write(data_dir.join("PG_VERSION"), "17").expect("PG_VERSION");

        // join виконано (у проді — node_node_id у SQLite settings)
        assert_eq!(recover_standby_at(true, &data_dir, &cfg_path), Ok(true));
        let raw = std::fs::read_to_string(&cfg_path).expect("read cfg");
        assert!(raw.contains("mode = \"standby\""), "raw: {raw}");

        // Головне: РЕАЛЬНИЙ load() бачить standby → фасад змонтує /api/v1/local/*
        assert!(
            NodeConfig::load().is_standby(),
            "після recovery load() має бачити mode=standby"
        );
        // Ідемпотентність прод-обгортки (join з SQLite може бути відсутнім у тесті).
        assert_eq!(recover_standby_if_needed(), Ok(true));
    }

    /// E2E прод-шляху БЕЗ інʼєкцій: recovery читає `node_node_id` з реальної
    /// SQLite каси (`OfflineDatabase::default_db_path`), бачить `PG_VERSION` у
    /// `data_dir_default()` і пише `[node] mode="standby"`; потім звичайний
    /// `NodeConfig::load()` його бачить. Запуск — окремим процесом:
    /// `XDG_DATA_HOME=<tmp> cargo test -p torgashka-infrastructure --lib
    /// e2e_recovery_from_sqlite -- --ignored --nocapture`.
    #[test]
    #[ignore = "потребує env XDG_DATA_HOME на тимчасовий каталог (див. doc)"]
    fn e2e_recovery_from_sqlite_settings_end_to_end() {
        let data_dir = crate::embedded_pg::data_dir_default();
        std::fs::create_dir_all(&data_dir).expect("mkdir data_dir");
        std::fs::write(data_dir.join("PG_VERSION"), "17").expect("PG_VERSION");

        // SQLite каси з ознакою виконаного join (node_node_id).
        let db_path = OfflineDatabase::default_db_path().expect("db path");
        std::fs::create_dir_all(db_path.parent().expect("parent")).expect("mkdir db dir");
        let conn = crate::offline::sync_push::open_connection(&db_path).expect("open sqlite");
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('node_node_id', 'e2e-node-1')",
            [],
        )
        .expect("insert node_node_id");
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('node_replication_host', '10.0.0.5')",
            [],
        )
        .expect("insert node_replication_host");
        drop(conn);

        // Прод-обгортка: без інʼєкцій, усі шляхи — реальні.
        assert_eq!(recover_standby_if_needed(), Ok(true));
        assert!(
            NodeConfig::load().is_standby(),
            "після recovery звичайний load() бачить standby"
        );
        let cfg_path = crate::db_sources::write_path();
        assert!(
            cfg_path.is_absolute() && !cfg_path.starts_with("."),
            "шлях запису мусить бути абсолютним (стабільний каталог), а не CWD: {cfg_path:?}"
        );
        let raw = std::fs::read_to_string(&cfg_path).expect("read cfg");
        assert!(raw.contains("mode = \"standby\""), "raw: {raw}");
        // Повторний старт (рестарт каси) — ідемпотентно, без змін у файлі.
        assert_eq!(recover_standby_if_needed(), Ok(true));
    }

    // ── Дефект 5: URL локальної репліки БЕЗ пароля ──────────────────────────

    #[test]
    fn local_readonly_url_drops_password() {
        let url = local_readonly_url("postgresql://repuser:s3cret@10.0.0.5:5432/pos_net", 5433)
            .expect("URL має перебудуватись");
        assert_eq!(url, "postgresql://repuser@127.0.0.1:5433/pos_net");
        assert!(
            !url.contains("s3cret"),
            "пароль у DATABASE_URL не має лишатись: {url}"
        );
    }

    #[test]
    fn strip_password_edge_cases() {
        // без пароля — без змін
        assert_eq!(
            strip_password("postgresql://u@127.0.0.1:5433/db"),
            "postgresql://u@127.0.0.1:5433/db"
        );
        // без userinfo — без змін
        assert_eq!(
            strip_password("postgresql://127.0.0.1:5433/db"),
            "postgresql://127.0.0.1:5433/db"
        );
        // postgres:// (коротка схема) + query-параметри
        assert_eq!(
            strip_password("postgres://u:p@h:5432/db?sslmode=require"),
            "postgres://u@h:5432/db?sslmode=require"
        );
        // не-URL — без паніки
        assert_eq!(strip_password("не-url"), "не-url");
    }

    #[test]
    fn local_readonly_url_is_none_for_non_postgres_scheme() {
        assert!(local_readonly_url("mysql://u:p@h/db", 5433).is_none());
    }
}
