//! Вбудований PostgreSQL (Windows-збірка Torgashka, де НЕМАЄ системного PG).
//!
//! Модуль знаходить бінарники PostgreSQL (`initdb`/`pg_ctl`), ініціалізує
//! data_dir, піднімає сервер на `127.0.0.1:5433` (фіксований порт — щоб не
//! конфліктувати з системним PG на 5432) та повертає `DATABASE_URL` для
//! підключення решти процесу.
//!
//! Шляхи пошуку бінарників (у порядку пріоритету):
//!   1. env `TORGASHKA_PG_DIR` (шлях до bin/);
//!   2. відносно exe: `../resources/postgres/bin` (Tauri-ресурси Windows);
//!   3. відносно `CARGO_MANIFEST_DIR`: `<ancestor>/.cache/pg/*/pgsql/bin`
//!      (дев-режим: завантажений postgresql-17.6-*-binaries.zip);
//!   4. Linux: `pg_config --bindir` (системний PG);
//!   5. Linux: `/usr/lib/postgresql/<ver>/bin` (17, 16, 15, ...).
//!
//! Логування — через `eprintln!` (як прийнято в крейті), логи сервера —
//! у `<data_dir>/postgres.log`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Порт вбудованого PostgreSQL (фіксований; уникає конфлікту з системним 5432).
pub const EMBEDDED_PG_PORT: u16 = 5433;
/// Таймаут очікування готовності сервера після `pg_ctl start`.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// Помилки модуля вбудованого PostgreSQL.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("бінарники PostgreSQL не знайдено (TORGASHKA_PG_DIR, resources/postgres, .cache/pg, pg_config)")]
    BinariesNotFound,
    #[error("не знайдено {0}")]
    Missing(String),
    #[error("помилка виконання {cmd}: {e}")]
    Command {
        cmd: String,
        #[source]
        e: std::io::Error,
    },
    #[error("{cmd} завершився з кодом {code}")]
    Exit { cmd: String, code: i32 },
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("сервер не став готовим за {0:?} (порт {1})")]
    StartTimeout(Duration, u16),
    #[error("БД '{db}' не вдалося створити: {why}")]
    CreateDb { db: String, why: String },
    #[error("пропущено: {0}")]
    Skipped(String),
}

// ── Імена бінарників ────────────────────────────────────────────────────────

fn initdb_name() -> &'static str {
    if cfg!(windows) { "initdb.exe" } else { "initdb" }
}

fn pg_ctl_name() -> &'static str {
    if cfg!(windows) { "pg_ctl.exe" } else { "pg_ctl" }
}

fn psql_name() -> &'static str {
    if cfg!(windows) { "psql.exe" } else { "psql" }
}

fn createdb_name() -> &'static str {
    if cfg!(windows) { "createdb.exe" } else { "createdb" }
}

/// Чи слухає щось 127.0.0.1:EMBEDDED_PG_PORT (TCP) — перевірка зайнятості
/// порту та готовності сервера.
fn port_is_open() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], EMBEDDED_PG_PORT));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Рекурсивний пошук `bin/` з `initdb` під коренем `.cache/pg` (глибина 2):
/// `<root>/postgresql-17.6-*-binaries/pgsql/bin` або `<root>/*/bin`.
fn find_pg_under(root: &Path) -> Option<PathBuf> {
    let l1 = std::fs::read_dir(root).ok()?;
    for e1 in l1.flatten() {
        let p1 = e1.path();
        for sub in ["pgsql/bin", "bin"] {
            let cand = p1.join(sub);
            if cand.join(initdb_name()).exists() {
                return Some(cand);
            }
        }
        if let Ok(l2) = std::fs::read_dir(&p1) {
            for e2 in l2.flatten() {
                let p2 = e2.path();
                for sub in ["pgsql/bin", "bin"] {
                    let cand = p2.join(sub);
                    if cand.join(initdb_name()).exists() {
                        return Some(cand);
                    }
                }
            }
        }
    }
    None
}

/// Типовий data_dir: `%APPDATA%/Torgashka/pgdata` (Windows) або
/// `$XDG_DATA_HOME/Torgashka/pgdata` (Linux, fallback `~/.local/share`).
pub fn data_dir_default() -> PathBuf {
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.trim().is_empty() {
            return PathBuf::from(appdata).join("Torgashka").join("pgdata");
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("Torgashka").join("pgdata");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join(".local/share/Torgashka/pgdata");
        }
    }
    PathBuf::from("pgdata")
}

// ── Менеджер ────────────────────────────────────────────────────────────────

/// Менеджер вбудованого PostgreSQL: володіє шляхами та станом запуску.
///
/// При `Drop` зупиняє сервер (`pg_ctl stop -m fast`), але ЛИШЕ якщо він був
/// запущений цим екземпляром (`started_by_us`) — чужий сервер на 5433 не
/// зупиняється.
#[derive(Debug, Clone)]
pub struct EmbeddedPostgres {
    bin_dir: PathBuf,
    data_dir: PathBuf,
    user: String,
    db: String,
    password: String,
    started_by_us: bool,
}

impl EmbeddedPostgres {
    /// Створює менеджер зі стандартним data_dir та env-перевизначенням
    /// креденшалів: `TORGASHKA_PG_USER` (дефолт `postgres`),
    /// `TORGASHKA_PG_DB` (дефолт `torgashka`), `TORGASHKA_PG_PASSWORD`
    /// (дефолт порожній — локальний trust auth).
    pub fn new(bin_dir: PathBuf) -> Self {
        Self::with_data_dir(bin_dir, data_dir_default())
    }

    /// Конструктор з явним data_dir (використовується в інтеграційних тестах).
    pub fn with_data_dir(bin_dir: PathBuf, data_dir: PathBuf) -> Self {
        let user = std::env::var("TORGASHKA_PG_USER").unwrap_or_else(|_| "postgres".to_string());
        let db = std::env::var("TORGASHKA_PG_DB").unwrap_or_else(|_| "torgashka".to_string());
        let password = std::env::var("TORGASHKA_PG_PASSWORD").unwrap_or_default();
        Self {
            bin_dir,
            data_dir,
            user,
            db,
            password,
            started_by_us: false,
        }
    }

    /// Знайти bin/ PostgreSQL (див. документацію модуля — порядок пріоритету).
    pub fn locate() -> Option<PathBuf> {
        // 1. env TORGASHKA_PG_DIR
        if let Ok(dir) = std::env::var("TORGASHKA_PG_DIR") {
            let p = PathBuf::from(dir);
            if p.join(initdb_name()).exists() {
                return Some(p);
            }
            // дозволяємо вказувати корінь розпакованого PG (bin всередині)
            if let Some(bin) = find_pg_under(&p) {
                return Some(bin);
            }
        }
        // 2. відносно exe: ../resources/postgres/bin (Tauri-ресурси Windows)
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let cand = parent.join("../resources/postgres/bin");
                if cand.join(initdb_name()).exists() {
                    return Some(cand);
                }
            }
        }
        // 3. відносно CARGO_MANIFEST_DIR: <ancestor>/.cache/pg (дев-режим)
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        for anc in manifest.ancestors() {
            let root = anc.join(".cache/pg");
            if root.is_dir() {
                if let Some(bin) = find_pg_under(&root) {
                    return Some(bin);
                }
            }
        }
        // 4. Linux: pg_config --bindir (системний PG)
        if let Ok(out) = Command::new("pg_config").arg("--bindir").output() {
            if out.status.success() {
                let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !dir.is_empty() && Path::new(&dir).join(initdb_name()).exists() {
                    return Some(PathBuf::from(dir));
                }
            }
        }
        // 5. Linux: стандартні шляхи дистрибутива
        for ver in ["17", "16", "15", "14", "13"] {
            let cand = PathBuf::from(format!("/usr/lib/postgresql/{ver}/bin"));
            if cand.join(initdb_name()).exists() {
                return Some(cand);
            }
        }
        None
    }

    /// Публічний getter data_dir (для логування).
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// DATABASE_URL для підключення до вбудованого сервера.
    pub fn database_url(&self) -> String {
        if self.password.is_empty() {
            format!("postgresql://{}@127.0.0.1:{}/{}", self.user, EMBEDDED_PG_PORT, self.db)
        } else {
            format!(
                "postgresql://{}:{}@127.0.0.1:{}/{}",
                self.user, self.password, EMBEDDED_PG_PORT, self.db
            )
        }
    }

    /// Ініціалізація data_dir (`initdb -D <dir> -U <user> -A trust --encoding=UTF8`).
    /// Ідемпотентно: якщо `PG_VERSION` вже існує — нічого не робить.
    pub fn ensure_initialized(&self) -> Result<(), Error> {
        if self.data_dir.join("PG_VERSION").exists() {
            return Ok(());
        }
        let initdb = self.bin_dir.join(initdb_name());
        if !initdb.exists() {
            return Err(Error::Missing(initdb.display().to_string()));
        }
        std::fs::create_dir_all(&self.data_dir)?;
        let cmd = "initdb";
        let status = Command::new(&initdb)
            .arg("-D")
            .arg(&self.data_dir)
            .arg("-U")
            .arg(&self.user)
            .arg("-A")
            .arg("trust")
            .arg("--encoding=UTF8")
            .status()
            .map_err(|e| Error::Command { cmd: cmd.to_string(), e })?;
        if !status.success() {
            return Err(Error::Exit { cmd: cmd.to_string(), code: status.code().unwrap_or(-1) });
        }
        eprintln!(
            "[torgashka-infrastructure] embedded PG: initdb завершено (data_dir: {})",
            self.data_dir.display()
        );
        Ok(())
    }

    /// Старт сервера: `pg_ctl -D <dir> -l <log> -o "-p 5433 -h 127.0.0.1" -w start`.
    /// Ідемпотентно: якщо 127.0.0.1:5433 вже слухає — не стартує другий.
    /// Після старту — poll готовності (TCP-конект, таймаут 30с).
    pub fn start(&mut self) -> Result<(), Error> {
        if port_is_open() {
            eprintln!(
                "[torgashka-infrastructure] embedded PG вже слухає 127.0.0.1:{EMBEDDED_PG_PORT} — старт пропущено"
            );
            return Ok(());
        }
        let pg_ctl = self.bin_dir.join(pg_ctl_name());
        if !pg_ctl.exists() {
            return Err(Error::Missing(pg_ctl.display().to_string()));
        }
        std::fs::create_dir_all(&self.data_dir)?;
        let log = self.data_dir.join("postgres.log");
        let cmd = "pg_ctl start";
        let status = Command::new(&pg_ctl)
            .arg("-D")
            .arg(&self.data_dir)
            .arg("-l")
            .arg(&log)
            .arg("-o")
            .arg(format!("-p {EMBEDDED_PG_PORT} -h 127.0.0.1"))
            .arg("-w")
            .arg("start")
            .status()
            .map_err(|e| Error::Command { cmd: cmd.to_string(), e })?;
        if !status.success() {
            return Err(Error::Exit { cmd: cmd.to_string(), code: status.code().unwrap_or(-1) });
        }
        self.started_by_us = true;
        // Poll готовності (страховка поверх pg_ctl -w): TCP до 127.0.0.1:5433.
        let deadline = Instant::now() + START_TIMEOUT;
        while Instant::now() < deadline {
            if port_is_open() {
                eprintln!(
                    "[torgashka-infrastructure] embedded PG запущено на 127.0.0.1:{EMBEDDED_PG_PORT} (log: {})",
                    log.display()
                );
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err(Error::StartTimeout(START_TIMEOUT, EMBEDDED_PG_PORT))
    }

    /// Зупинка сервера: `pg_ctl -D <dir> -m fast stop`. Ідемпотентно.
    pub fn stop(&self) -> Result<(), Error> {
        if !port_is_open() {
            return Ok(());
        }
        let pg_ctl = self.bin_dir.join(pg_ctl_name());
        if !pg_ctl.exists() {
            return Err(Error::Missing(pg_ctl.display().to_string()));
        }
        let cmd = "pg_ctl stop";
        let status = Command::new(&pg_ctl)
            .arg("-D")
            .arg(&self.data_dir)
            .arg("-m")
            .arg("fast")
            .arg("stop")
            .status()
            .map_err(|e| Error::Command { cmd: cmd.to_string(), e })?;
        if !status.success() {
            return Err(Error::Exit { cmd: cmd.to_string(), code: status.code().unwrap_or(-1) });
        }
        eprintln!("[torgashka-infrastructure] embedded PG зупинено");
        Ok(())
    }

    /// Створення БД `db`, якщо її немає (`psql SELECT 1` → `createdb`).
    pub fn ensure_database(&self) -> Result<(), Error> {
        let psql = self.bin_dir.join(psql_name());
        let createdb = self.bin_dir.join(createdb_name());
        if !psql.exists() {
            return Err(Error::Missing(psql.display().to_string()));
        }
        let check = Command::new(&psql)
            .args(["-h", "127.0.0.1", "-p", &EMBEDDED_PG_PORT.to_string()])
            .args(["-U", &self.user, "-d", "postgres", "-tAc"])
            .arg(format!("SELECT 1 FROM pg_database WHERE datname = '{}'", self.db))
            .output()
            .map_err(|e| Error::Command { cmd: "psql".to_string(), e })?;
        let exists = check.status.success()
            && String::from_utf8_lossy(&check.stdout).trim() == "1";
        if exists {
            return Ok(());
        }
        if !createdb.exists() {
            return Err(Error::Missing(createdb.display().to_string()));
        }
        let status = Command::new(&createdb)
            .args(["-h", "127.0.0.1", "-p", &EMBEDDED_PG_PORT.to_string()])
            .args(["-U", &self.user])
            .arg(&self.db)
            .status()
            .map_err(|e| Error::Command { cmd: "createdb".to_string(), e })?;
        if !status.success() {
            return Err(Error::CreateDb {
                db: self.db.clone(),
                why: format!("createdb exit {:?}", status.code()),
            });
        }
        eprintln!(
            "[torgashka-infrastructure] embedded PG: БД '{}' створено",
            self.db
        );
        Ok(())
    }

    /// Повний bootstrap: знайти бінарники → initdb (якщо треба) → старт →
    /// створити БД → встановити `DATABASE_URL` для процесу.
    ///
    /// Повертає менеджера — викликач має тримати його живим (Drop зупинить
    /// сервер при завершенні).
    pub fn bootstrap_if_needed() -> Result<Self, Error> {
        if let Ok(url) = std::env::var("DATABASE_URL") {
            if !url.trim().is_empty() {
                return Err(Error::Skipped(
                    "DATABASE_URL задано — embedded PG не потрібен".to_string(),
                ));
            }
        }
        let bin_dir = Self::locate().ok_or(Error::BinariesNotFound)?;
        let mut pg = Self::new(bin_dir);
        pg.ensure_initialized()?;
        pg.start()?;
        pg.ensure_database()?;
        std::env::set_var("DATABASE_URL", pg.database_url());
        eprintln!(
            "[torgashka-infrastructure] embedded PG: DATABASE_URL встановлено ({})",
            pg.database_url()
        );
        Ok(pg)
    }
}

/// Вільна обгортка: знайти бінарники → initdb → старт → БД → DATABASE_URL.
/// (Викликається з torgashka-api serve_listener перед підключенням до БД.)
pub fn bootstrap_if_needed() -> Result<EmbeddedPostgres, Error> {
    EmbeddedPostgres::bootstrap_if_needed()
}

impl Drop for EmbeddedPostgres {
    fn drop(&mut self) {
        if self.started_by_us {
            match self.stop() {
                Ok(()) => {}
                Err(e) => eprintln!(
                    "[torgashka-infrastructure] попередження: embedded PG stop: {e}"
                ),
            }
        }
    }
}

// ── Тести ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Тимчасовий data_dir в temp (унікальний на процес+час).
    fn temp_data_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "torgashka_pg_test_{}_{nanos}",
            std::process::id()
        ))
    }

    /// Менеджер для тестів; None → PG не знайдено (тест має зробити skip).
    fn test_pg() -> Option<EmbeddedPostgres> {
        let bin_dir = EmbeddedPostgres::locate()?;
        Some(EmbeddedPostgres::with_data_dir(bin_dir, temp_data_dir()))
    }

    #[test]
    fn locate_finds_postgres_binaries() {
        // Linux (dev): системний PG 17 через pg_config або /usr/lib/postgresql/*/bin
        let found = EmbeddedPostgres::locate();
        assert!(
            found.is_some(),
            "locate() не знайшов PostgreSQL; задайте TORGASHKA_PG_DIR або встановіть системний PG"
        );
        let bin = found.expect("checked");
        assert!(
            bin.join(initdb_name()).exists(),
            "bin має містити initdb: {}",
            bin.display()
        );
        assert!(
            bin.join(pg_ctl_name()).exists(),
            "bin має містити pg_ctl: {}",
            bin.display()
        );
    }

    #[test]
    fn data_dir_default_resolves() {
        // Завжди дає якийсь шлях (APPDATA/XDG/HOME/CWD)
        let dir = data_dir_default();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn ensure_initialized_is_idempotent() {
        let Some(pg) = test_pg() else {
            eprintln!("SKIP: PostgreSQL бінарники не знайдено");
            return;
        };
        pg.ensure_initialized().expect("initdb має пройти");
        assert!(
            pg.data_dir().join("PG_VERSION").exists(),
            "PG_VERSION має створитись після initdb"
        );
        // Ідемпотентність: повторний виклик без помилки і без повторного initdb
        pg.ensure_initialized().expect("повторний initdb не має падати");
        let _ = std::fs::remove_dir_all(pg.data_dir());
    }

    #[test]
    fn start_stop_roundtrip() {
        let Some(pg) = test_pg() else {
            eprintln!("SKIP: PostgreSQL бінарники не знайдено");
            return;
        };
        if port_is_open() {
            eprintln!("SKIP: порт 127.0.0.1:{EMBEDDED_PG_PORT} вже зайнятий");
            return;
        }
        let mut pg = pg;
        pg.ensure_initialized().expect("initdb");
        pg.start().expect("pg_ctl start має підняти сервер");
        assert!(port_is_open(), "сервер має слухати 127.0.0.1:5433");
        pg.ensure_database().expect("БД має створитись");
        pg.stop().expect("pg_ctl stop має зупинити сервер");
        // fast stop — порт має звільнитись (poll до 4с)
        for _ in 0..40 {
            if !port_is_open() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!port_is_open(), "порт має звільнитись після stop");
        let _ = std::fs::remove_dir_all(pg.data_dir());
    }
}
