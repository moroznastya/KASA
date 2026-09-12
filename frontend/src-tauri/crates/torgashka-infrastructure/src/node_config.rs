//! NodeConfig — локальна конфігурація вузла мережі магазинів (секція `[node]`
//! у `db_sources.toml`).
//!
//! ADR-0008 (рівноправні read-write вузли): **режимів вузла більше немає**.
//! Видалено режими вузла (`Primary`/`Standby`), позначку repoint і ґейт
//! блокування push: кожен вузол має власну read-write БД, а транспорт між
//! вузлами — прикладний HTTP-протокол синку (`/api/v1/sync/*`).
//!
//! Що лишається в секції `[node]` (усе — опційні поля):
//!
//! ```toml
//! [node]
//! local_port = 5433           # порт ЛОКАЛЬНОЇ копії БД (default 5433)
//! primary_db_url = "postgresql://..."  # опційно; default — активне джерело
//! upstream_write_url = "postgresql://..."  # опційно: ціль адмін-запису
//! degrade_to_local = true     # дозволити локальний режим (default true)
//! ```
//!
//! Відсутність файлу/секції → [`NodeConfig::default`] — фасад стартує як
//! раніше, без жодних змін поведінки.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Порт локальної копії БД вузла (співпадає з `embedded_pg::EMBEDDED_PG_PORT`).
const LOCAL_PG_PORT: u16 = 5433;

/// Конфігурація вузла з секції `[node]` db_sources.toml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Порт ЛОКАЛЬНОЇ копії БД вузла (див. `/api/v1/local/*`).
    #[serde(default = "default_local_port")]
    pub local_port: u16,
    /// URL primary (куди push-иться черга). `None` → активне джерело
    /// db_sources.toml ([`crate::db_sources::active_source_url`]).
    #[serde(default)]
    pub primary_db_url: Option<String>,
    /// URL апстрім-запису (єдина ціль адмін-запису): `None`/порожньо →
    /// апстріму немає (адмін-запис → відмова).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_write_url: Option<String>,
    /// Дозволити деградацію в локальний режим, коли апстрім недоступний.
    #[serde(default = "default_degrade")]
    pub degrade_to_local: bool,
}

fn default_local_port() -> u16 {
    LOCAL_PG_PORT
}

fn default_degrade() -> bool {
    true
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            local_port: LOCAL_PG_PORT,
            primary_db_url: None,
            upstream_write_url: None,
            degrade_to_local: true,
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

    /// URL апстрім-запису: явне поле `upstream_write_url`; порожньо/відсутнє →
    /// `None`. НЕ падає на активне джерело (локальна копія ціллю запису не є).
    pub fn resolve_upstream_write_url(&self) -> Option<String> {
        self.upstream_write_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    /// Чи заданий АПСТРІМ (куди вузол має щось вивантажувати) — вирішальна
    /// умова доставності локальної черги чеків (`pos.rs`): якщо апстріму немає,
    /// черга належить ЦЬОМУ вузлу і застосовується локально (див. §3.1 плану,
    /// дефект «провальний чек виглядав як успіх»).
    ///
    /// `true`, якщо є хоч одне з:
    ///   * явний `[node] primary_db_url` (standalone POS, що пише на віддалений
    ///     сервер — норма);
    ///   * явний `upstream_write_url`;
    ///   * розв'язаний primary (явний або активне джерело db_sources.toml),
    ///     який НЕ вказує на ВЛАСНИЙ локальний кластер вузла.
    pub fn has_configured_upstream(&self) -> bool {
        if self
            .primary_db_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        {
            return true;
        }
        if self.resolve_upstream_write_url().is_some() {
            return true;
        }
        match self.resolve_primary_db_url() {
            Some(url) => !is_self_local_url(&url, self.local_port),
            None => false,
        }
    }

    /// ЯВНО записана секція `[node]` з диска: `Some(cfg)` лише коли файл
    /// існує І містить секцію `[node]`. Відрізняється від [`Self::load`], яка
    /// «дефолт Primary» віддає і без файлу (там ця різниця не важлива, тут —
    /// принципова: дефолт ≠ рішення вузла).
    pub fn load_explicit() -> Option<Self> {
        let path = crate::db_sources::existing_path()?;
        Self::load_explicit_from_path(&path)
    }

    /// Те саме з КОНКРЕТНОГО файлу (тести — без env/CWD).
    pub fn load_explicit_from_path(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Self::load_explicit_from_str(&content)
    }

    /// Те саме з тексту: `None`, якщо секції `[node]` немає або текст не TOML.
    pub fn load_explicit_from_str(content: &str) -> Option<Self> {
        let v: toml::Value = toml::from_str(content).ok()?;
        let node = v.get("node")?;
        node.clone().try_into().ok()
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

/// Чи URL вказує на ВЛАСНИЙ локальний кластер вузла (`127.0.0.1`/`localhost`/
/// `::1` + `local_port`). Використовується ґейтом push (Фаза 3.8):
/// «посилання на себе» — не апстрім, а ознака вузла-джерела істини.
pub fn is_self_local_url(url: &str, local_port: u16) -> bool {
    let Some((host, port)) = primary_endpoint(url) else {
        return false;
    };
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    let is_loopback = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1");
    is_loopback && port == local_port
}

/// Перевірка доступності апстріму: TCP-конект до host:port з таймаутом 2 с
/// (вимога контракту: 1-2 с). Мережа «є», якщо порт приймає з'єднання —
/// подальша автентифікація/каталожні запити робляться звичайним шляхом.
pub async fn primary_reachable(url: &str) -> bool {
    let Some((host, port)) = primary_endpoint(url) else {
        return false;
    };
    matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
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

/// URL джерела для секції `[node]` зі збережених креденшлів вузла:
/// `postgresql://{user}@{host}:{port}/{database}` (без пароля).
///
/// Саме з нього [`local_db_url`] виводить локальну адресу
/// `127.0.0.1:{local_port}/<реальна БД>`.
/// Без цього ім'я БД губилось і фасад підставляв вигадану «torgashka»
/// (дефект 2026-09: `/api/v1/setup/status` = 503 назавжди, postgres.log каси →
/// `FATAL: database "torgashka" does not exist`).
pub fn primary_db_url_from_parts(user: &str, host: &str, port: u16, database: &str) -> String {
    format!("postgresql://{user}@{host}:{port}/{database}")
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
    fn default_is_backward_compatible() {
        let cfg = NodeConfig::default();
        assert!(cfg.degrade_to_local);
        assert_eq!(cfg.local_port, 5433);
        assert_eq!(cfg.primary_db_url, None);
        assert_eq!(cfg.upstream_write_url, None);
    }

    #[test]
    fn empty_content_is_default() {
        assert_eq!(NodeConfig::load_from_str(""), NodeConfig::default());
        assert_eq!(
            NodeConfig::load_from_str("active = \"main\"\n[sources.main]\nhost=\"h\"\n"),
            NodeConfig::default()
        );
    }

    #[test]
    fn parses_primary_db_url_override() {
        let cfg = NodeConfig::load_from_str(
            r#"
[node]
primary_db_url = "postgresql://u:p@10.0.0.9:5432/pos"
degrade_to_local = false
"#,
        );
        assert!(!cfg.degrade_to_local);
        assert_eq!(
            cfg.primary_db_url.as_deref(),
            Some("postgresql://u:p@10.0.0.9:5432/pos")
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

    /// (в) `NodeConfig::load()`-частина: читання AppData-файлу (не CWD).
    #[test]
    fn load_reads_config_from_appdata_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Імітація %APPDATA%\Torgashka\db_sources.toml
        let appdata_cfg = dir.path().join("Torgashka").join("db_sources.toml");
        std::fs::create_dir_all(appdata_cfg.parent().unwrap()).expect("mkdir");
        std::fs::write(&appdata_cfg, "[node]\nlocal_port = 5433\n").expect("write");

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
        assert_eq!(cfg.primary_db_url, None);
        assert_eq!(
            cfg.local_port, 5433,
            "load() з AppData → власна секція [node]"
        );
    }

    #[test]
    fn primary_db_url_from_parts_derives_local_url_with_same_db() {
        // Критерій контракту: userinfo збережено, host:port → локальні,
        // ім'я БД збережено.
        let primary = primary_db_url_from_parts("postgres", "192.0.2.10", 5432, "pos_system_fresh");
        assert_eq!(
            primary,
            "postgresql://postgres@192.0.2.10:5432/pos_system_fresh"
        );
        assert_eq!(
            local_db_url(&primary, 5433).expect("локальний URL"),
            "postgresql://postgres@127.0.0.1:5433/pos_system_fresh"
        );
        assert_eq!(
            local_readonly_url(&primary, 5433).expect("локальний URL без пароля"),
            "postgresql://postgres@127.0.0.1:5433/pos_system_fresh"
        );
    }

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

    // ── P6/F1: upstream_write_url резолвиться ЛИШЕ з власного поля ──

    #[test]
    fn upstream_write_url_absent_is_none_and_never_falls_back() {
        let cfg = NodeConfig::load_from_str("[node]\nmode = \"standby\"\n");
        assert_eq!(
            cfg.resolve_upstream_write_url(),
            None,
            "без поля → None (НЕ падає на активне джерело: на standby воно = репліка, F5)"
        );
    }

    #[test]
    fn upstream_write_url_explicit_value_is_trimmed_and_empty_is_none() {
        let cfg = NodeConfig::load_from_str(
            "[node]\nmode = \"standby\"\nupstream_write_url = \"  postgresql://u@10.0.0.5:5432/pos  \"\n",
        );
        assert_eq!(
            cfg.resolve_upstream_write_url().as_deref(),
            Some("postgresql://u@10.0.0.5:5432/pos")
        );
        let cfg =
            NodeConfig::load_from_str("[node]\nmode = \"standby\"\nupstream_write_url = \"   \"\n");
        assert_eq!(cfg.resolve_upstream_write_url(), None, "пробіли = порожньо");
    }

    #[test]
    fn upstream_write_url_field_does_not_affect_primary_resolution() {
        // F2: mode=primary ігнорує поле — нові поля не змінюють резолв пулів.
        let cfg = NodeConfig::load_from_str(
            "[node]\nmode = \"primary\"\nprimary_db_url = \"postgresql://u@h:5432/db\"\n",
        );
        assert_eq!(
            cfg.resolve_primary_db_url().as_deref(),
            Some("postgresql://u@h:5432/db")
        );
        assert_eq!(cfg.resolve_upstream_write_url(), None);
    }
}
