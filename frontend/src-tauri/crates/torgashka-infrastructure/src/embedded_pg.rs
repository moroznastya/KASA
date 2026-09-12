//! Вбудований PostgreSQL (Windows-збірка Torgashka, де НЕМАЄ системного PG).
//!
//! Модуль знаходить бінарники PostgreSQL (`initdb`/`pg_ctl`), ініціалізує
//! data_dir, піднімає сервер на `127.0.0.1:5433` (фіксований порт — щоб не
//! конфліктувати з системним PG на 5432) та повертає `DATABASE_URL` для
//! підключення решти процесу.
//!
//! Шляхи пошуку бінарників (у порядку пріоритету):
//!   1. env `TORGASHKA_PG_DIR` (шлях до bin/);
//!   2. відносно exe: `resources/postgres/bin` (Tauri-ресурси Windows;
//!      ФІКС 2026-08-21: на Windows Tauri v2 resource_dir() = exe_dir, тож
//!      `../resources` давало `C:\Program Files\resources\...` — промах повз
//!      папку застосунку, PG не знаходився → auth-роути не монтувались (410));
//!   3. відносно `CARGO_MANIFEST_DIR`: `<ancestor>/.cache/pg/*/pgsql/bin`
//!      (дев-режим: завантажений postgresql-17.6-*-binaries.zip);
//!   4. Linux: `pg_config --bindir` (системний PG);
//!   5. Linux: `/usr/lib/postgresql/<ver>/bin` (17, 16, 15, ...).
//!
//! ФІКС 2026-08-21 (не-ASCII шлях): initdb на Windows падає, якщо data_dir
//! містить не-ASCII (кириличне ім'я користувача -> C:\Users\Вася\...).
//! [`data_dir_default`] приймає лише ASCII-кандидати: APPDATA -> LOCALAPPDATA
//! -> temp_dir -> C:\ProgramData\Torgashka\pgdata.//!
//! Логування: `eprintln!` (консоль/dev) + файл поряд з data_dir
//! (`%APPDATA%/Torgashka/torgashka.log`, ФІКС 2026-08-21 — на Windows
//! `windows_subsystem=windows` приховує stderr, тож файл — єдиний видимий
//! канал діагностики старту/помилок embedded PG). Логи сервера —
//! у `<data_dir>/postgres.log`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Порт вбудованого PostgreSQL (фіксований; уникає конфлікту з системним 5432).
pub const EMBEDDED_PG_PORT: u16 = 5433;
/// Таймаут очікування готовності сервера після `pg_ctl start`.
/// ФІКС 2026-08-21 (0xC000013A): Windows — 60s. Після Ctrl+C postgres.exe
/// потребує crash recovery; на повільних дисках він перевищує 30s
/// (checkpoint write 4.6s у логах користувача) → StartTimeout.
fn start_timeout() -> Duration {
    start_timeout_from(std::env::var(START_TIMEOUT_ENV).ok().as_deref())
}

/// Чиста логіка таймауту (тестована без env-гонок).
pub fn start_timeout_from(env_value: Option<&str>) -> Duration {
    if let Some(v) = env_value {
        if let Ok(secs) = v.trim().parse::<u64>() {
            // Обмеження 1..=600 с: 0 зробив би pg_ctl -t 0 (нескінченно),
            // завелике значення повертає «вічне» очікування.
            return Duration::from_secs(secs.clamp(1, 600));
        }
    }
    if cfg!(windows) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(30)
    }
}

/// Env-перевизначення [`start_timeout`] у секундах (тести/оператори).
pub const START_TIMEOUT_ENV: &str = "TORGASHKA_PG_START_TIMEOUT_SECS";

/// Таймаут з'єднання `psql` (PGCONNECT_TIMEOUT, дефект 5б): psql НЕ має права
/// висіти на TCP-конекті/старті з'єднання безмежно.
pub const PSQL_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Ім'я файлу stderr `pg_ctl` у data_dir (дефект 5б): діагностика pg_ctl
/// зберігається, але НЕ через pipe — демон `postgres.exe` не може успадкувати
/// pipe і заблокувати батька назавжди.
pub const PG_CTL_LOG_NAME: &str = "pg_ctl.log";

/// Аргументи `psql` для ЛОКАЛЬНОГО підключення (чиста функція — тестована).
///
/// `-w` (`--no-password`) — обов'язковий: застосунок це GUI-процес без
/// консолі; якщо pg_hba репліки вимагає пароль, psql показав би запит пароля
/// і застиг НАЗАВЖДИ (саме це блокувало фасад :8000 — дефект 5). З `-w` psql
/// завершується помилкою замість очікування вводу.
pub fn psql_conn_args(user: &str, db: &str, port: u16) -> Vec<String> {
    vec![
        "-w".to_string(),
        "-h".to_string(),
        "127.0.0.1".to_string(),
        "-p".to_string(),
        port.to_string(),
        "-U".to_string(),
        user.to_string(),
        "-d".to_string(),
        db.to_string(),
    ]
}

/// Env для `psql`: PGCONNECT_TIMEOUT (секунди) — межа очікування з'єднання.
pub fn psql_conn_env() -> Vec<(&'static str, String)> {
    vec![("PGCONNECT_TIMEOUT", PSQL_CONNECT_TIMEOUT_SECS.to_string())]
}

/// Аргументи `pg_ctl start` (чиста функція — тестована).
///
/// `-w -t <secs>`: pg_ctl чекає готовності, але НЕ довше нашого таймауту
/// (дефолт pg_ctl — 60 с, і без `-t` це ще одна сліпа зона очікування).
pub fn pg_ctl_start_args(
    data_dir: &Path,
    log: &Path,
    opts: &str,
    timeout_secs: u64,
) -> Vec<std::ffi::OsString> {
    vec![
        "-D".into(),
        data_dir.as_os_str().into(),
        "-l".into(),
        log.as_os_str().into(),
        "-o".into(),
        opts.into(),
        "-w".into(),
        "-t".into(),
        timeout_secs.to_string().into(),
        "start".into(),
    ]
}

/// Аргументи `pg_ctl stop` (чиста функція — тестована): `-w -t <secs>`.
pub fn pg_ctl_stop_args(data_dir: &Path, timeout_secs: u64) -> Vec<std::ffi::OsString> {
    vec![
        "-D".into(),
        data_dir.as_os_str().into(),
        "-m".into(),
        "fast".into(),
        "-w".into(),
        "-t".into(),
        timeout_secs.to_string().into(),
        "stop".into(),
    ]
}

/// Рядок аргументів для логу (діагностика: що саме виконано).
fn args_line<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> String {
    args.iter()
        .map(|a| a.as_ref().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

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
    #[error("{cmd} завершився з кодом {code}: {stderr}")]
    ExitWithOutput {
        cmd: String,
        code: i32,
        stderr: String,
    },
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("сервер не став готовим за {0:?} (порт {1})")]
    StartTimeout(Duration, u16),
    #[error("БД '{db}' не вдалося створити: {why}")]
    CreateDb { db: String, why: String },
    #[error("пропущено: {0}")]
    Skipped(String),
}

// ── Файлове логування ───────────────────────────────────────────────────────

/// Шлях до файлу діагностичного логу: поряд з data_dir
/// (`%APPDATA%/Torgashka/torgashka.log` або еквівалент за платформою).
/// На Windows консоль прихована (`windows_subsystem=windows`) — цей файл
/// єдиний видимий канал діагностики embedded PG.
pub fn log_file_path() -> PathBuf {
    let base = data_dir_default();
    if let Some(parent) = base.parent() {
        if !parent.as_os_str().is_empty() {
            return parent.join("torgashka.log");
        }
    }
    std::env::temp_dir().join("torgashka.log")
}

/// Поточний час (UTC) у форматі `YYYY-MM-DD HH:MM:SS` (без зовнішніх крейтів).
fn timestamp_str() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil_from_days (H. Hinnant): epoch-days -> (y,m,d)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Запис діагностичного повідомлення: дублює в stderr (консоль/dev) і дописує
/// у файл [`log_file_path`]. Використовується для критичних подій embedded PG.
pub fn pg_log(level: &str, msg: &str) {
    eprintln!("[{level}] {msg}");
    let path = log_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(f, "[{}] [{level}] {msg}", timestamp_str());
    }
}

/// Хвіст `postgres.log` у каталозі даних (діагностика невдалого старту PG —
/// після `pg_ctl: could not start server` сама причина є ЛИШЕ тут).
pub fn postgres_log_tail(data_dir: &Path, n: usize) -> String {
    read_log_tail(&data_dir.join("postgres.log"), n)
}

/// Останні `n` рядків файлу (для діагностики postgres.log при таймауті старту).
fn read_log_tail(path: &Path, n: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => content
            .lines()
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("(не вдалося прочитати {}: {e})", path.display()),
    }
}

// ── Імена бінарників ────────────────────────────────────────────────────────

fn initdb_name() -> &'static str {
    if cfg!(windows) {
        "initdb.exe"
    } else {
        "initdb"
    }
}

fn pg_ctl_name() -> &'static str {
    if cfg!(windows) {
        "pg_ctl.exe"
    } else {
        "pg_ctl"
    }
}

/// Ім'я `pg_restore` у ТОМУ САМОМУ каталозі бінарників, що `pg_ctl`/`psql`.
///
/// Навіщо саме так: `pg_restore` мусить бути ТІЄЇ Ж версії, що сервер і решта
/// інструментів (дамп v1.16 читає лише PG 17.x; системний `/usr/bin/pg_restore`
/// 16.x його не читає). Каталог резолвить `EmbeddedPostgres::locate()` —
/// TORGASHKA_PG_DIR → resources/postgres → .cache/pg → pg_config → системні
/// шляхи; хардкодити `/usr/lib/postgresql/...` не можна.
pub fn pg_restore_name() -> &'static str {
    if cfg!(windows) {
        "pg_restore.exe"
    } else {
        "pg_restore"
    }
}

/// Ім'я `pg_dump` (та сама логіка каталогу й `.exe`, що [`pg_restore_name`]).
pub fn pg_dump_name() -> &'static str {
    if cfg!(windows) {
        "pg_dump.exe"
    } else {
        "pg_dump"
    }
}

/// Готує `Command` для `pg_ctl` зі stdio, які НЕ може успадкувати демон.
///
/// ДЕФЕКТ 5б (жива Windows-каса): `pg_ctl start` лишає жити `postgres.exe`
/// (postmaster + бекенди). Якщо stdout/stderr батька — pipe (`.output()`), демон
/// успадковує write-end і тримає його, доки живий сервер; читання pipe ніколи
/// не бачить EOF → `.output()` блокується НАЗАВЖДИ (журнал обривався на
/// `data_dir`, 5× postgres.exe живі, pg_ctl.exe відсутній, фасад :8000 без
/// `axum::serve`). Тому:
///   * `stdout` → `Stdio::null()` — вивід СЕРВЕРА все одно йде в
///     `-l <data_dir>/postgres.log` (аргумент збережено);
///   * `stderr` → файл `<data_dir>/pg_ctl.log` — діагностика pg_ctl не губиться
///     (раніше осідала в пам'яті `.output()`), читається у текст помилки;
///   * `stdin` → `Stdio::null()` — демон не тримає ввід/консоль батька.
///
/// Якщо `pg_ctl.log` не створити — `Stdio::null()` (блокування неможливе за
/// жодних умов) + WARN у torgashka.log. `CREATE_NO_WINDOW |
/// CREATE_NEW_PROCESS_GROUP` (дефект Ctrl+C/0xC000013A) збережено.
fn pg_ctl_command(pg_ctl: &Path, data_dir: &Path) -> Command {
    let mut c = Command::new(pg_ctl);
    // Демон не має успадковувати жодного pipe батька.
    c.stdin(Stdio::null()).stdout(Stdio::null());
    let log = data_dir.join(PG_CTL_LOG_NAME);
    match std::fs::File::create(&log) {
        Ok(f) => {
            c.stderr(Stdio::from(f));
        }
        Err(e) => {
            pg_log(
                "WARN",
                &format!("{} не створено ({e}) — stderr pg_ctl → null", log.display()),
            );
            c.stderr(Stdio::null());
        }
    }
    #[cfg(windows)]
    {
        // CREATE_NEW_PROCESS_GROUP (0x200) | CREATE_NO_WINDOW (0x0800_0000):
        // 1) postgres.exe НЕ отримує CTRL_C_EVENT разом із консоллю застосунку
        //    (0xC000013A);
        // 2) GUI-процес без консолі спавнить console-процес pg_ctl.exe → Windows
        //    створює ВИДИМЕ чорне вікно. Юзер закриває його → CTRL_CLOSE_EVENT
        //    → postgres.exe падає з 0xC000013A → crash recovery 30-60 с на
        //    наступному старті (лог: "database system was not properly shut
        //    down"). CREATE_NO_WINDOW ховає вікно — прибрати джерело crash.
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0200);
    }
    c
}

fn psql_name() -> &'static str {
    if cfg!(windows) {
        "psql.exe"
    } else {
        "psql"
    }
}

/// Хвіст файлу, якщо він існує (інакше «(відсутній)») — без шуму в діагностиці.
fn log_tail_if_exists(path: &Path, n: usize) -> String {
    if path.exists() {
        read_log_tail(path, n)
    } else {
        "(відсутній)".to_string()
    }
}

/// Контекст невдалого `pg_ctl` для повідомлення про помилку (дефект 5б, п.3 —
/// stderr не «проковтувати»): stderr самого pg_ctl тепер у `pg_ctl.log`,
/// причини від сервера — у `postgres.log` (після `pg_ctl: could not start
/// server` деталі є ЛИШЕ там).
fn pg_ctl_failure_context(data_dir: &Path) -> String {
    format!(
        "{} (хвіст): {}; postgres.log (хвіст): {}",
        PG_CTL_LOG_NAME,
        log_tail_if_exists(&data_dir.join(PG_CTL_LOG_NAME), 20),
        log_tail_if_exists(&data_dir.join("postgres.log"), 20),
    )
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
///
/// ФІКС 2026-08-21 (Windows): initdb падає, якщо шлях data_dir містить
/// не-ASCII символи (кириличне ім'я користувача -> `C:\Users\Вася\AppData\...`).
/// Кандидати приймаються лише ASCII, інакше — fallback: LOCALAPPDATA ->
/// temp_dir -> `C:\ProgramData\Torgashka\pgdata` (системний шлях завжди
/// латиниця, пишеться звичайним користувачем).
pub fn data_dir_default() -> PathBuf {
    #[cfg(windows)]
    {
        for cand in [std::env::var("APPDATA"), std::env::var("LOCALAPPDATA")] {
            if let Ok(dir) = cand {
                let t = dir.trim();
                if !t.is_empty() && t.is_ascii() {
                    return PathBuf::from(t).join("Torgashka").join("pgdata");
                }
            }
        }
        let tmp = std::env::temp_dir();
        if tmp.as_os_str().is_ascii() {
            return tmp.join("Torgashka").join("pgdata");
        }
        // Останній ASCII-кандидат. eprintln! замість pg_log — щоб уникнути
        // рекурсії (pg_log -> log_file_path -> data_dir_default).
        eprintln!(
            "[WARN] APPDATA/LOCALAPPDATA/temp містять не-ASCII — data_dir: C:\\ProgramData\\Torgashka\\pgdata"
        );
        return PathBuf::from(r"C:\ProgramData")
            .join("Torgashka")
            .join("pgdata");
    }
    #[cfg(not(windows))]
    {
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
    /// Створює менеджера зі стандартним data_dir та env-перевизначенням
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
        // 2. відносно exe: resources/postgres/bin (Tauri-ресурси Windows).
        //    ФІКС 2026-08-21: Tauri v2 на Windows resource_dir() = exe_dir, тому
        //    бандлер кладе ресурси в <exe_dir>/resources/... . Шлях `../resources`
        //    давав C:\Program Files\resources\... (промах) → BinariesNotFound →
        //    embedded PG не стартував → auth=None → users-list не монтувався (410).
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                // Основний (правильний) шлях: <exe_dir>/resources/postgres/bin
                let cand = parent.join("resources/postgres/bin");
                if cand.join(initdb_name()).exists() {
                    return Some(cand);
                }
                // Fallback для нестандартних layout (старий інсталятор тощо)
                let legacy = parent.join("../resources/postgres/bin");
                if legacy.join(initdb_name()).exists() {
                    return Some(legacy);
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
            format!(
                "postgresql://{}@127.0.0.1:{}/{}",
                self.user, EMBEDDED_PG_PORT, self.db
            )
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
        let out = Command::new(&initdb)
            .arg("-D")
            .arg(&self.data_dir)
            .arg("-U")
            .arg(&self.user)
            .arg("-A")
            .arg("trust")
            // ФІКС 2026-08-21 (Windows): без --locale initdb падає на українській
            // локалі ("could not find suitable text search configuration for
            // locale Ukrainian_Ukraine.1251", код 1). --locale=C сумісний з
            // --encoding=UTF8 — це валідна комбінація (C locale, UTF-8 кодування).
            .arg("--locale")
            .arg("C")
            .arg("--encoding=UTF8")
            .output()
            .map_err(|e| Error::Command {
                cmd: cmd.to_string(),
                e,
            })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            pg_log(
                "ERROR",
                &format!(
                    "initdb завершився з кодом {:?}; data_dir: {}; stderr: {}",
                    out.status.code(),
                    self.data_dir.display(),
                    stderr
                ),
            );
            return Err(Error::ExitWithOutput {
                cmd: cmd.to_string(),
                code: out.status.code().unwrap_or(-1),
                stderr,
            });
        }
        pg_log(
            "INFO",
            &format!("initdb завершено (data_dir: {})", self.data_dir.display()),
        );
        Ok(())
    }

    /// Старт сервера: `pg_ctl -D <dir> -l <log> -o "-p 5433 -h 127.0.0.1" -w start`.
    /// Ідемпотентно: якщо 127.0.0.1:5433 вже слухає — не стартує другий.
    /// Після старту — poll готовності (TCP-конект, таймаут 30с).
    /// ФІКС 2026-08-21 (Windows 0xC000013A): postgres.exe стартує в НОВІЙ
    /// process group (CREATE_NEW_PROCESS_GROUP) — Ctrl+C у консолі застосунку
    /// більше не розсилається дочірньому postgres.exe (раніше: CTRL_C_EVENT
    /// → аварійне завершення → crash recovery при наступному старті).
    /// Після crash прибираємо залишок postmaster.pid мертвого процесу і
    /// повторюємо старт (до 2 спроб; recovery на повільних дисках може
    /// перевищити перший таймаут).
    pub fn start(&mut self) -> Result<(), Error> {
        if port_is_open() {
            pg_log(
                "INFO",
                &format!("embedded PG вже слухає 127.0.0.1:{EMBEDDED_PG_PORT} — старт пропущено"),
            );
            return Ok(());
        }
        // Після crash у data_dir лишається postmaster.pid мертвого процесу —
        // без його прибирання pg_ctl start не підніме сервер
        // ("another server might be running").
        self.cleanup_stale_pid();
        let mut last_err = None;
        for attempt in 1..=2 {
            match self.start_once() {
                Ok(()) => return Ok(()),
                Err(e @ Error::StartTimeout(..)) => {
                    pg_log(
                        "WARN",
                        &format!("спроба {attempt}: сервер не готовий ({e}); повторюю..."),
                    );
                    last_err = Some(e);
                    self.cleanup_stale_pid();
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| Error::StartTimeout(start_timeout(), EMBEDDED_PG_PORT)))
    }

    /// Стартує сервер і «віддає володіння» рівню застосунку: після успішного
    /// старту цей екземпляр БІЛЬШЕ НЕ зупиняє сервер при `Drop`.
    ///
    /// Потрібно там, де життєвий цикл PG керує викликач (дефект 1 провіжна
    /// standby): guard створюється лише щоб підняти сервер, а жити він має
    /// поза ним. Зупинка — [`stop_running_instance`] (RunEvent::Exit).
    pub fn start_detached(mut self) -> Result<(), Error> {
        self.start()?;
        self.started_by_us = false;
        pg_log(
            "INFO",
            "embedded PG: володіння віддано — Drop не зупинить сервер",
        );
        Ok(())
    }

    /// Одна спроба `pg_ctl start` + poll готовності (таймаут — з env).
    fn start_once(&mut self) -> Result<(), Error> {
        self.start_once_with_timeout(start_timeout())
    }

    /// Те саме, але з ЯВНИМ таймаутом: детерміновані тести без env-гонок.
    fn start_once_with_timeout(&mut self, timeout: Duration) -> Result<(), Error> {
        let pg_ctl = self.bin_dir.join(pg_ctl_name());
        if !pg_ctl.exists() {
            return Err(Error::Missing(pg_ctl.display().to_string()));
        }
        std::fs::create_dir_all(&self.data_dir)?;
        // B3: зовнішній доступ (listen/SSL/HBA з env) — перед кожним стартом.
        // Помилка застосування НЕ валить старт: pg_ctl дасть точну діагностику,
        // а torgashka.log — наш WARN.
        if let Err(e) = apply_external_config(&self.data_dir) {
            pg_log(
                "WARN",
                &format!("зовнішній конфіг PG (SSL/HBA) не застосовано: {e}"),
            );
        }
        let log = self.data_dir.join("postgres.log");
        let cmd = "pg_ctl start";
        // ДЕФЕКТ 5б: stdio батька НЕ pipe — інакше демон postgres.exe
        // успадковує write-end і читання ніколи не бачить EOF (див.
        // pg_ctl_command; CREATE_NO_WINDOW переїхав туди ж).
        let mut c = pg_ctl_command(&pg_ctl, &self.data_dir);
        // Дефект 5б: `-w -t <наш таймаут>` — pg_ctl не чекає довше за нас.
        let args = pg_ctl_start_args(&self.data_dir, &log, &pg_ctl_opts(), timeout.as_secs());
        let started = Instant::now();
        pg_log("INFO", &format!("{cmd}: початок — {}", args_line(&args)));
        // ДЕФЕКТ 5б: `.status()` (лише код виходу) замість `.output()`: pipe
        // для захоплення виводу тут смертельний — демон успадковує write-end
        // і тримає його, доки живий сервер (див. pg_ctl_command).
        let status = c.args(&args).status().map_err(|e| Error::Command {
            cmd: cmd.to_string(),
            e,
        })?;
        pg_log(
            "INFO",
            &format!(
                "{cmd}: завершено, код {:?} ({} мс)",
                status.code(),
                started.elapsed().as_millis()
            ),
        );
        if !status.success() {
            // Діагностика не губиться: stderr pg_ctl → pg_ctl.log (+ хвіст
            // postgres.log), і саме вона йде в текст помилки.
            let why = pg_ctl_failure_context(&self.data_dir);
            pg_log(
                "ERROR",
                &format!("pg_ctl start: код {:?}; {why}", status.code()),
            );
            return Err(Error::ExitWithOutput {
                cmd: cmd.to_string(),
                code: status.code().unwrap_or(-1),
                stderr: why,
            });
        }
        self.started_by_us = true;
        // Poll готовності (страховка поверх pg_ctl -w): TCP до 127.0.0.1:5433.
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if port_is_open() {
                pg_log(
                    "INFO",
                    &format!(
                        "embedded PG запущено на 127.0.0.1:{EMBEDDED_PG_PORT} (log: {})",
                        log.display()
                    ),
                );
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        // Таймаут: найцінніша діагностика — хвіст postgres.log (чому сервер
        // не піднявся: локаль, права, порт, конфіг).
        let tail = read_log_tail(&log, 40);
        pg_log(
            "ERROR",
            &format!(
                "сервер не став готовим за {:?} (порт {EMBEDDED_PG_PORT}); postgres.log (хвіст):\n{tail}",
                timeout
            ),
        );
        Err(Error::StartTimeout(timeout, EMBEDDED_PG_PORT))
    }

    /// Прибирає залишки crash: postmaster.pid мертвого процесу блокує старт.
    /// Живий процес (напр. crash recovery) НЕ чіпаємо — це безпечно.
    fn cleanup_stale_pid(&self) {
        let pid_file = self.data_dir.join("postmaster.pid");
        let Ok(content) = std::fs::read_to_string(&pid_file) else {
            return;
        };
        let Ok(pid) = content.lines().next().unwrap_or("").trim().parse::<u32>() else {
            return;
        };
        if process_alive(pid) {
            return; // процес живий — не втручаємось
        }
        let _ = std::fs::remove_file(&pid_file);
        let _ = std::fs::remove_file(self.data_dir.join("postmaster.opts"));
        pg_log(
            "INFO",
            &format!("прибрано залишок postmaster.pid (процес {pid} мертвий — crash)"),
        );
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
        // ДЕФЕКТ 5б: stdio НЕ успадковується демоном (див. pg_ctl_command);
        // pg_ctl stop теж говорить із сервером → діагностика у файл.
        let mut c = pg_ctl_command(&pg_ctl, &self.data_dir);
        let args = pg_ctl_stop_args(&self.data_dir, start_timeout().as_secs());
        let started = Instant::now();
        pg_log("INFO", &format!("{cmd}: {}", args_line(&args)));
        let status = c.args(&args).status().map_err(|e| Error::Command {
            cmd: cmd.to_string(),
            e,
        })?;
        pg_log(
            "INFO",
            &format!(
                "{cmd}: код {:?} ({} мс)",
                status.code(),
                started.elapsed().as_millis()
            ),
        );
        if !status.success() {
            let e = Error::ExitWithOutput {
                cmd: cmd.to_string(),
                code: status.code().unwrap_or(-1),
                stderr: pg_ctl_failure_context(&self.data_dir),
            };
            pg_log("ERROR", &format!("pg_ctl stop: {e}"));
            return Err(e);
        }
        pg_log("INFO", "embedded PG зупинено");
        Ok(())
    }

    /// Створення БД `db`, якщо її немає (`psql SELECT 1` → `psql CREATE DATABASE`).
    ///
    /// ФІКС 2026-08-22 (Windows, корінь бага "немає зв'язку з БД"):
    /// slim-бандл `resources/postgres/bin` НЕ містить `createdb.exe` (лише
    /// initdb/pg_ctl/psql/postgres + DLL) — попередній код падав з
    /// `Error::Missing(createdb.exe)`, bootstrap обривався, `DATABASE_URL` не
    /// встановлювався → auth=None → `/api/v1/auth/users-list` не монтувався →
    /// фронтенд отримував 410/404 замість списку користувачів.
    /// Створення БД виконуємо через `psql -c "CREATE DATABASE ..."` — psql
    /// гарантовано присутній у бандлі (використовується і для перевірки).
    pub fn ensure_database(&self) -> Result<(), Error> {
        let psql = self.bin_dir.join(psql_name());
        if !psql.exists() {
            return Err(Error::Missing(psql.display().to_string()));
        }
        // Дефект 5б: `-w` (ніколи не питати пароль) + PGCONNECT_TIMEOUT.
        // Без них psql на GUI-процесі без консолі застигає назавжди.
        let started = Instant::now();
        let check_sql = format!("SELECT 1 FROM pg_database WHERE datname = '{}'", self.db);
        let args = psql_conn_args(&self.user, "postgres", EMBEDDED_PG_PORT);
        let mut c = Command::new(&psql);
        c.args(&args).arg("-tAc").arg(&check_sql);
        for (k, v) in psql_conn_env() {
            c.env(k, v);
        }
        if let Ok(pw) = std::env::var("TORGASHKA_PG_PASSWORD") {
            if !pw.is_empty() {
                c.env("PGPASSWORD", pw);
            }
        }
        let check = c.output().map_err(|e| Error::Command {
            cmd: "psql".to_string(),
            e,
        })?;
        pg_log(
            "INFO",
            &format!(
                "ensure_database: перевірка наявності БД (psql {} -tAc SELECT… , {} мс)",
                args_line(&args),
                started.elapsed().as_millis()
            ),
        );
        let exists = check.status.success() && String::from_utf8_lossy(&check.stdout).trim() == "1";
        if exists {
            return Ok(());
        }
        // CREATE DATABASE через psql (createdb.exe відсутній у slim-бандлі).
        let create_sql = format!("CREATE DATABASE \"{}\"", self.db);
        let started_create = Instant::now();
        let mut c = Command::new(&psql);
        c.args(psql_conn_args(&self.user, "postgres", EMBEDDED_PG_PORT))
            .arg("-c")
            .arg(&create_sql);
        for (k, v) in psql_conn_env() {
            c.env(k, v);
        }
        if let Ok(pw) = std::env::var("TORGASHKA_PG_PASSWORD") {
            if !pw.is_empty() {
                c.env("PGPASSWORD", pw);
            }
        }
        let status = c.status().map_err(|e| Error::Command {
            cmd: "psql CREATE DATABASE".to_string(),
            e,
        })?;
        pg_log(
            "INFO",
            &format!(
                "ensure_database: CREATE DATABASE (psql -w, {} мс)",
                started_create.elapsed().as_millis()
            ),
        );
        if !status.success() {
            let why = format!("psql CREATE DATABASE exit {:?}", status.code());
            pg_log(
                "ERROR",
                &format!("створення БД '{}' не вдалося: {why}", self.db),
            );
            return Err(Error::CreateDb {
                db: self.db.clone(),
                why,
            });
        }
        pg_log("INFO", &format!("БД '{}' створено", self.db));
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
        pg_log(
            "INFO",
            "bootstrap: DATABASE_URL не задано — запускаємо вбудований PostgreSQL",
        );
        let t_locate = Instant::now();
        let bin_dir = match Self::locate() {
            Some(b) => {
                pg_log(
                    "INFO",
                    &format!(
                        "bootstrap: крок 1/4 — бінарники PG знайдено: {} ({} мс)",
                        b.display(),
                        t_locate.elapsed().as_millis()
                    ),
                );
                b
            }
            None => {
                pg_log(
                    "ERROR",
                    "бінарники PG НЕ знайдено (TORGASHKA_PG_DIR, resources/postgres, .cache/pg, pg_config)",
                );
                return Err(Error::BinariesNotFound);
            }
        };
        let mut pg = Self::new(bin_dir);
        // Діагностика (дефект 5в): журнал обривався тут — далі кожен крок із часом.
        pg_log(
            "INFO",
            &format!("bootstrap: data_dir: {}", pg.data_dir().display()),
        );
        let t_init = Instant::now();
        if let Err(e) = pg.ensure_initialized() {
            pg_log("ERROR", &format!("initdb не виконано: {e}"));
            return Err(e);
        }
        pg_log(
            "INFO",
            &format!(
                "bootstrap: крок 2/4 — initdb/перевірка каталогу ({} мс)",
                t_init.elapsed().as_millis()
            ),
        );
        let t_start = Instant::now();
        if let Err(e) = pg.start() {
            pg_log("ERROR", &format!("pg_ctl start не виконано: {e}"));
            return Err(e);
        }
        pg_log(
            "INFO",
            &format!(
                "bootstrap: крок 3/4 — сервер запущено ({} мс)",
                t_start.elapsed().as_millis()
            ),
        );
        let t_db = Instant::now();
        if let Err(e) = pg.ensure_database() {
            pg_log("ERROR", &format!("створення БД не виконано: {e}"));
            return Err(e);
        }
        pg_log(
            "INFO",
            &format!(
                "bootstrap: крок 4/4 — БД готова ({} мс)",
                t_db.elapsed().as_millis()
            ),
        );
        std::env::set_var("DATABASE_URL", pg.database_url());
        pg_log(
            "INFO",
            &format!("DATABASE_URL встановлено ({})", pg.database_url()),
        );
        Ok(pg)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Конфігурація зовнішнього доступу (B3, рішення Творця): TORGASHKA_PG_LISTEN_
// ADDRESSES, TORGASHKA_PG_SSL_CERT/KEY, TORGASHKA_PG_HBA_EXTRA. Чисті функції
// (без env) — покриті юніт-тестами; env-обгортки делегують їм.
// ─────────────────────────────────────────────────────────────────────────────

/// Аргументи `-o` для `pg_ctl start` за значенням listen-адрес.
///
/// * `127.0.0.1` (або порожньо) — поточна поведінка: `-p 5433 -h 127.0.0.1`;
/// * інакше — `-p 5433 -c listen_addresses='<val>'` БЕЗ `-h` (val може бути
///   `*` або список IP через кому — PG слухає всі зазначені).
fn pg_ctl_opts_for(listen: &str) -> String {
    let l = listen.trim();
    if l.is_empty() || l == "127.0.0.1" {
        format!("-p {EMBEDDED_PG_PORT} -h 127.0.0.1")
    } else {
        format!("-p {EMBEDDED_PG_PORT} -c listen_addresses='{l}'")
    }
}

/// Env-обгортка [`pg_ctl_opts_for`] (TORGASHKA_PG_LISTEN_ADDRESSES).
fn pg_ctl_opts() -> String {
    pg_ctl_opts_for(&std::env::var("TORGASHKA_PG_LISTEN_ADDRESSES").unwrap_or_default())
}

/// Дописує рядки у конфіг-файл без дублювання (ідемпотентно): наявні
/// (trim-співпадіння) рядки пропускаються, порожні — ігноруються.
/// Повертає true, якщо файл змінено.
fn append_conf_lines(path: &Path, lines: &[String]) -> Result<bool, std::io::Error> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = existing;
    let mut changed = false;
    for line in lines {
        let needle = line.trim();
        if needle.is_empty() {
            continue;
        }
        if out.lines().any(|l| l.trim() == needle) {
            continue; // уже є — не дублюємо
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
        changed = true;
    }
    if changed {
        std::fs::write(path, out.as_bytes())?;
    }
    Ok(changed)
}

/// Застосовує конфігурацію зовнішнього доступу до конфігів `data_dir`
/// (чиста функція — env читає [`apply_external_config`]):
///
/// * `ssl = Some((cert, key))` → postgresql.conf: `ssl=on`,
///   `ssl_cert_file='<cert>'`, `ssl_key_file='<key>'`;
/// * `hba_extra = Some(...)` (рядки через `\n`) → pg_hba.conf (напр.
///   `hostssl replication replicator_xxx <IP_standby>/32 md5` для віддаленої
///   реплікації зі standby через інтернет).
///
/// Ідемпотентно: повторне застосування не дублює рядки. None — файли не
/// чіпаються (повна зворотна сумісність). Викликається перед КОЖНИМ стартом
/// сервера (start_once) — зміни підхоплює наступний запуск.
fn apply_external_config_files(
    data_dir: &Path,
    ssl: Option<(&str, &str)>,
    hba_extra: Option<&str>,
) -> Result<(), std::io::Error> {
    if let Some((cert, key)) = ssl {
        let cert = cert.trim();
        let key = key.trim();
        if !cert.is_empty() && !key.is_empty() {
            let conf = data_dir.join("postgresql.conf");
            let lines = vec![
                "# --- Torgashka B3: зовнішній SSL (TORGASHKA_PG_SSL_CERT/KEY) ---".to_string(),
                "ssl = on".to_string(),
                format!("ssl_cert_file = '{cert}'"),
                format!("ssl_key_file = '{key}'"),
            ];
            let _ = append_conf_lines(&conf, &lines)?;
        }
    }
    if let Some(extra) = hba_extra {
        let lines: Vec<String> = extra
            .split('\n')
            .map(|l| l.trim_end().to_string())
            .collect();
        if lines.iter().any(|l| !l.trim().is_empty()) {
            let hba = data_dir.join("pg_hba.conf");
            let _ = append_conf_lines(&hba, &lines)?;
        }
    }
    Ok(())
}

/// Env-обгортка [`apply_external_config_files`]:
/// TORGASHKA_PG_SSL_CERT + TORGASHKA_PG_SSL_KEY (обидва обов'язкові) та
/// TORGASHKA_PG_HBA_EXTRA.
fn apply_external_config(data_dir: &Path) -> Result<(), Error> {
    // cert/key — Option<String>, живуть до кінця функції: &str-позики валідні.
    let cert = std::env::var("TORGASHKA_PG_SSL_CERT")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let key = std::env::var("TORGASHKA_PG_SSL_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let hba = std::env::var("TORGASHKA_PG_HBA_EXTRA").ok();
    let (cert_ref, key_ref) = (cert.as_deref(), key.as_deref());
    let ssl = match (cert_ref, key_ref) {
        (Some(c), Some(k)) if !c.trim().is_empty() && !k.trim().is_empty() => Some((c, k)),
        (None, None) => None,
        _ => {
            pg_log(
                "WARN",
                "TORGASHKA_PG_SSL_CERT/TORGASHKA_PG_SSL_KEY: задано лише один з двох — SSL-конфіг пропущено (потрібні обидва)",
            );
            None
        }
    };
    apply_external_config_files(data_dir, ssl, hba.as_deref()).map_err(Error::Io)
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
                Err(e) => pg_log("ERROR", &format!("embedded PG stop: {e}")),
            }
        }
    }
}

/// Зупинити вбудований PostgreSQL (якщо він наш і запущений). Викликається з
/// RunEvent::Exit застосунку — страхівка поверх Drop таска serve_listener
/// (tokio abort() не гарантує миттєвого Drop до завершення процесу).
/// Ідемпотентно й безпечно: якщо data_dir не ініціалізований або порт 5433
/// не слухає — без дій; чужий сервер (інший data_dir на 5433) не чіпається
/// (pg_ctl -D зупиняє лише сервер цього data_dir).
pub fn stop_running_instance() {
    // data_dir ще не ініціалізовано (PG_VERSION нема) — нічого зупиняти.
    let data_dir = data_dir_default();
    if !data_dir.join("PG_VERSION").exists() {
        return;
    }
    let Some(bin_dir) = EmbeddedPostgres::locate() else {
        pg_log("WARN", "stop_running_instance: бінарники PG не знайдено");
        return;
    };
    let pg = EmbeddedPostgres::new(bin_dir);
    match pg.stop() {
        Ok(()) => pg_log("INFO", "stop_running_instance: embedded PG зупинено"),
        Err(e) => pg_log(
            "WARN",
            &format!("stop_running_instance: {e} (можливо, уже зупинено)"),
        ),
    }
}

/// Ідемпотентно піднімає ЛОКАЛЬНУ embedded-репліку (standby-вузол).
///
/// Викликається при старті застосунку ДО бінда фасаду `:8000` у режимі
/// standby («Дефект 2»): без цього після перезапуску/ребуту каса в режимі
/// standby не має локальної БД (`init_local_standby` → `None`,
/// `/api/v1/local/*` не змонтовано, UI вічно висить на «Підключення до
/// сервера…»). Primary-режим цю функцію не викликає взагалі.
///
/// Гілки (ідемпотентно, БЕЗ `initdb`):
/// * `127.0.0.1:5433` уже слухає → `Ok(false)` (no-op);
/// * каталогу даних немає або без `PG_VERSION` → `Ok(false)` (вузол ще не
///   провіжнено — primary-шлях його ініціалізує окремо);
/// * інакше → `pg_ctl start` через [`EmbeddedPostgres::start_detached`]
///   (сервер лишається запущеним) → `Ok(true)`.
pub fn ensure_local_replica_running() -> Result<bool, Error> {
    let data_dir = data_dir_default();
    let bin_dir = EmbeddedPostgres::locate();
    ensure_local_replica_running_at(bin_dir.as_deref(), &data_dir)
}

/// Чиста (без env) реалізація [`ensure_local_replica_running`] — для тестів.
fn ensure_local_replica_running_at(bin_dir: Option<&Path>, data_dir: &Path) -> Result<bool, Error> {
    if port_is_open() {
        pg_log(
            "INFO",
            &format!(
                "ensure_local_replica_running: 127.0.0.1:{EMBEDDED_PG_PORT} уже слухає — no-op"
            ),
        );
        return Ok(false);
    }
    if !data_dir.join("PG_VERSION").exists() {
        pg_log(
            "INFO",
            &format!(
                "ensure_local_replica_running: {} без PG_VERSION — вузол не провіжнено, старт пропущено",
                data_dir.display()
            ),
        );
        return Ok(false);
    }
    let bin_dir = bin_dir.ok_or(Error::BinariesNotFound)?;
    let mgr = EmbeddedPostgres::with_data_dir(bin_dir.to_path_buf(), data_dir.to_path_buf());
    mgr.start_detached()?;
    pg_log(
        "INFO",
        &format!(
            "ensure_local_replica_running: локальну репліку піднято ({})",
            data_dir.display()
        ),
    );
    Ok(true)
}

/// Перевірка, чи процес з PID живий (без додаткових крейтів).
#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    // tasklist — стандартна утиліта Windows.
    match std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()),
        Err(_) => true, // не можемо перевірити — не чіпаємо pid
    }
}

#[cfg(not(windows))]
fn process_alive(pid: u32) -> bool {
    // Linux: /proc/<pid> існує для живого процесу.
    Path::new("/proc").join(pid.to_string()).exists()
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
        std::env::temp_dir().join(format!("torgashka_pg_test_{}_{nanos}", std::process::id()))
    }

    /// Менеджер для тестів; None → PG не знайдено (тест має зробити skip).
    fn test_pg() -> Option<EmbeddedPostgres> {
        let bin_dir = EmbeddedPostgres::locate()?;
        Some(EmbeddedPostgres::with_data_dir(bin_dir, temp_data_dir()))
    }

    // ── ensure_local_replica_running (дефект 2: старт репліки) ────────────

    #[test]
    fn ensure_local_replica_is_noop_when_port_open() {
        // Порт зайнятий (у CI/dev — системний PG або embedded) → no-op.
        // Якщо зайняти не вдалося (порт уже кимось слухається) — умова та
        // сама: no-op очікується в обох випадках.
        let _listener = std::net::TcpListener::bind(("127.0.0.1", EMBEDDED_PG_PORT)).ok();
        let dir = temp_data_dir();
        assert!(!dir.join("PG_VERSION").exists());
        let started = ensure_local_replica_running_at(None, &dir).expect("no-op не помиляється");
        assert!(
            !started,
            "порт {EMBEDDED_PG_PORT} зайнятий → старт має бути пропущено"
        );
    }

    #[test]
    fn ensure_local_replica_noop_without_pg_version() {
        // Каталог без PG_VERSION (вузол ще не провіжнено) → Ok(false),
        // жодного pg_ctl/initdb (bin_dir = None доводить, що не викликається).
        let dir = temp_data_dir();
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert!(!dir.join("PG_VERSION").exists());
        let started = ensure_local_replica_running_at(None, &dir).expect("no-op");
        assert!(!started, "без PG_VERSION старт репліки не виконується");
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
    fn data_dir_default_rejects_non_ascii() {
        // ФІКС 2026-08-21: не-ASCII кандидати (кириличне ім'я користувача
        // Windows) відкидаються. Імітуємо ланцюг: не-ASCII APPDATA →
        // LOCALAPPDATA (ASCII) має перемогти; якщо всі не-ASCII — ProgramData.
        #[cfg(windows)]
        {
            std::env::set_var("APPDATA", "C:\\Users\\Вася\\AppData\\Roaming");
            std::env::set_var("LOCALAPPDATA", "C:\\Users\\Admin\\AppData\\Local");
            let dir = data_dir_default();
            let s = dir.to_string_lossy().to_string();
            assert!(s.is_ascii(), "data_dir має бути ASCII: {s}");
            assert!(s.contains("Admin"), "має обрати ASCII LOCALAPPDATA: {s}");
        }
        // На Linux просто перевіряємо, що резолв не падає.
        #[cfg(not(windows))]
        {
            let _ = data_dir_default();
        }
    }

    #[test]
    fn pg_log_writes_to_file() {
        // Файлове логування: після виклику pg_log файл поряд з data_dir
        // існує і містить маркер. (Windows: stderr приховано — це єдиний
        // видимий канал діагностики embedded PG.)
        let marker = format!("pg_log_test_{}", std::process::id());
        pg_log("TEST", &marker);
        let path = log_file_path();
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("лог має читатись {}: {e}", path.display()));
        assert!(
            content.contains(&marker),
            "лог {} має містити маркер {marker}",
            path.display()
        );
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
        pg.ensure_initialized()
            .expect("повторний initdb не має падати");
        let _ = std::fs::remove_dir_all(pg.data_dir());
    }

    #[test]
    #[ignore = "CI: /dev/shm обмеження в GitHub runner — pg_ctl start падає. Локально: cargo test -p torgashka-infrastructure embedded_pg"]
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
        pg.ensure_database()
            .expect("БД має створитись через psql CREATE DATABASE");
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

    /// ФІКС 2026-08-22 (Windows): БД створюється через psql CREATE DATABASE
    /// (createdb.exe відсутній у slim-бандлі). Тест перевіряє, що ensure_database
    /// НЕ залежить від createdb і створює БД на живому сервері.
    #[test]
    #[ignore = "CI: /dev/shm обмеження в GitHub runner — pg_ctl start падає. Локально: cargo test -p torgashka-infrastructure embedded_pg"]
    fn ensure_database_creates_without_createdb_binary() {
        let Some(pg) = test_pg() else {
            eprintln!("SKIP: PostgreSQL бінарники не знайдено");
            return;
        };
        if port_is_open() {
            eprintln!("SKIP: порт 127.0.0.1:{EMBEDDED_PG_PORT} вже зайнятий");
            return;
        }
        let mut pg = pg;
        // Імітація slim-бандлу Windows: у bin/ НЕМАЄ createdb — ensure_database
        // має працювати через psql. (На Linux createdb може існувати — тест
        // валідний у будь-якому разі, бо ensure_database більше не викликає його.)
        pg.ensure_initialized().expect("initdb");
        pg.start().expect("pg_ctl start");
        pg.ensure_database()
            .expect("БД має створитись без createdb");
        // Повторний виклик — ідемпотентність (БД вже існує).
        pg.ensure_database()
            .expect("повторний ensure_database не має падати");
        pg.stop().expect("stop");
        for _ in 0..40 {
            if !port_is_open() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = std::fs::remove_dir_all(pg.data_dir());
    }

    // ── B3: формування -o (listen) ──────────────────────────────────────

    #[test]
    fn pg_ctl_opts_default_is_localhost_only() {
        assert_eq!(
            pg_ctl_opts_for("127.0.0.1"),
            format!("-p {EMBEDDED_PG_PORT} -h 127.0.0.1")
        );
        assert_eq!(
            pg_ctl_opts_for(""),
            format!("-p {EMBEDDED_PG_PORT} -h 127.0.0.1")
        );
        assert_eq!(
            pg_ctl_opts_for("   "),
            format!("-p {EMBEDDED_PG_PORT} -h 127.0.0.1")
        );
    }

    #[test]
    fn pg_ctl_opts_external_listen_uses_listen_addresses() {
        assert_eq!(
            pg_ctl_opts_for("*"),
            format!("-p {EMBEDDED_PG_PORT} -c listen_addresses='*'")
        );
        assert_eq!(
            pg_ctl_opts_for("0.0.0.0"),
            format!("-p {EMBEDDED_PG_PORT} -c listen_addresses='0.0.0.0'")
        );
        assert_eq!(
            pg_ctl_opts_for("127.0.0.1,192.168.1.5"),
            format!("-p {EMBEDDED_PG_PORT} -c listen_addresses='127.0.0.1,192.168.1.5'")
        );
    }

    // ── B3: SSL + HBA у data_dir (ідемпотентність) ──────────────────────

    fn fake_data_dir() -> PathBuf {
        let dir = temp_data_dir();
        std::fs::create_dir_all(&dir).expect("data_dir");
        std::fs::write(
            dir.join("postgresql.conf"),
            "# PostgreSQL configuration\nlisten_addresses = 'localhost'\n",
        )
        .expect("conf");
        std::fs::write(
            dir.join("pg_hba.conf"),
            "# TYPE DATABASE USER ADDRESS METHOD\nhost all all 127.0.0.1/32 trust\n",
        )
        .expect("hba");
        dir
    }

    #[test]
    fn apply_external_config_writes_ssl_and_hba() {
        let dir = fake_data_dir();
        apply_external_config_files(
            &dir,
            Some(("/certs/server.crt", "/certs/server.key")),
            Some(
                "hostssl replication replicator_ab12 203.0.113.5/32 md5\nhost all all 10.0.0.0/8 scram-sha-256",
            ),
        )
        .expect("apply");

        let conf = std::fs::read_to_string(dir.join("postgresql.conf")).expect("conf");
        assert!(conf.contains("ssl = on"), "ssl=on має бути: {conf}");
        assert!(
            conf.contains("ssl_cert_file = '/certs/server.crt'"),
            "cert: {conf}"
        );
        assert!(
            conf.contains("ssl_key_file = '/certs/server.key'"),
            "key: {conf}"
        );
        // оригінальний вміст недоторканий
        assert!(
            conf.contains("listen_addresses = 'localhost'"),
            "оригінал: {conf}"
        );

        let hba = std::fs::read_to_string(dir.join("pg_hba.conf")).expect("hba");
        assert!(
            hba.contains("hostssl replication replicator_ab12 203.0.113.5/32 md5"),
            "hba: {hba}"
        );
        assert!(
            hba.contains("host all all 10.0.0.0/8 scram-sha-256"),
            "hba: {hba}"
        );
    }

    #[test]
    fn apply_external_config_does_not_duplicate_on_restart() {
        let dir = fake_data_dir();
        let ssl = Some(("/certs/server.crt", "/certs/server.key"));
        let hba = Some("hostssl replication replicator_ab12 203.0.113.5/32 md5");
        apply_external_config_files(&dir, ssl, hba).expect("перше застосування");
        apply_external_config_files(&dir, ssl, hba).expect("повторне застосування (рестарт)");
        apply_external_config_files(&dir, ssl, hba).expect("третє (рестарт)");

        let conf = std::fs::read_to_string(dir.join("postgresql.conf")).expect("conf");
        assert_eq!(
            conf.matches("ssl = on").count(),
            1,
            "ssl=on не дублюється: {conf}"
        );
        assert_eq!(
            conf.matches("ssl_cert_file").count(),
            1,
            "ssl_cert_file не дублюється: {conf}"
        );
        assert_eq!(
            conf.matches("ssl_key_file").count(),
            1,
            "ssl_key_file не дублюється: {conf}"
        );

        let hba = std::fs::read_to_string(dir.join("pg_hba.conf")).expect("hba");
        assert_eq!(
            hba.matches("hostssl replication replicator_ab12").count(),
            1,
            "hba не дублюється: {hba}"
        );
    }

    #[test]
    fn apply_external_config_noop_when_nothing_configured() {
        let dir = fake_data_dir();
        let conf_before = std::fs::read_to_string(dir.join("postgresql.conf")).expect("before");
        let hba_before = std::fs::read_to_string(dir.join("pg_hba.conf")).expect("hba before");
        apply_external_config_files(&dir, None, None).expect("noop");
        let conf_after = std::fs::read_to_string(dir.join("postgresql.conf")).expect("after");
        let hba_after = std::fs::read_to_string(dir.join("pg_hba.conf")).expect("hba after");
        assert_eq!(conf_before, conf_after, "без env конфіг не чіпається");
        assert_eq!(hba_before, hba_after, "без env hba не чіпається");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Дефект 5: жоден виклик PG не має права висіти безмежно (psql `-w` +
    // PGCONNECT_TIMEOUT; pg_ctl `-w -t <наш таймаут>`).
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn restore_and_dump_binaries_follow_the_same_naming_rules_as_pg_ctl() {
        let expected = |base: &str| {
            if cfg!(windows) {
                format!("{base}.exe")
            } else {
                base.to_string()
            }
        };
        assert_eq!(pg_restore_name(), expected("pg_restore"));
        assert_eq!(pg_dump_name(), expected("pg_dump"));
    }

    #[test]
    fn psql_conn_args_contain_no_password_flag() {
        let args = psql_conn_args("repuser", "postgres", EMBEDDED_PG_PORT);
        let line = args_line(&args);
        assert!(
            args.contains(&"-w".to_string()),
            "psql ОБОВ'ЯЗКОВО з -w (--no-password): без нього запит пароля на GUI-процесі без консолі застигає назавжди: {line}"
        );
        assert!(line.contains("-h 127.0.0.1"), "{line}");
        assert!(line.contains("-U repuser"), "{line}");
        assert!(line.contains("-d postgres"), "{line}");
        assert!(line.contains("-p 5433"), "{line}");
    }

    #[test]
    fn psql_conn_env_sets_pgconnect_timeout() {
        let env = psql_conn_env();
        assert!(
            env.iter()
                .any(|(k, v)| *k == "PGCONNECT_TIMEOUT"
                    && *v == PSQL_CONNECT_TIMEOUT_SECS.to_string()),
            "psql має отримувати PGCONNECT_TIMEOUT={PSQL_CONNECT_TIMEOUT_SECS}: {env:?}"
        );
        // Значення беремо з env-конфігурації виклику (не константа) —
        // перевіряємо, що саме воно піде у процес psql.
        assert_eq!(
            env.iter()
                .find(|(k, _)| *k == "PGCONNECT_TIMEOUT")
                .map(|(_, v)| v.clone()),
            Some(PSQL_CONNECT_TIMEOUT_SECS.to_string()),
            "PGCONNECT_TIMEOUT має дорівнювати таймауту з'єднання"
        );
    }

    #[test]
    fn pg_ctl_start_args_are_time_bounded() {
        let args = pg_ctl_start_args(
            Path::new("/tmp/pgdata"),
            Path::new("/tmp/postgres.log"),
            "-p 5433 -h 127.0.0.1",
            7,
        );
        let line = args_line(&args);
        assert!(
            line.contains("-w"),
            "pg_ctl має чекати готовності (-w): {line}"
        );
        assert!(
            line.contains("-t 7"),
            "pg_ctl не має чекати довше за наш таймаут (дефолт 60с — сліпа зона): {line}"
        );
        assert!(line.ends_with("start"), "{line}");
        assert!(line.contains("-D /tmp/pgdata"), "{line}");
        assert!(line.contains("-o -p 5433 -h 127.0.0.1"), "{line}");
    }

    #[test]
    fn pg_ctl_stop_args_are_time_bounded() {
        let line = args_line(&pg_ctl_stop_args(Path::new("/tmp/pgdata"), 5));
        assert!(line.contains("-m fast"), "{line}");
        assert!(line.contains("-w -t 5"), "{line}");
        assert!(line.ends_with("stop"), "{line}");
    }

    #[test]
    fn start_timeout_clamps_and_defaults() {
        assert_eq!(start_timeout_from(Some("3")), Duration::from_secs(3));
        // 0 → pg_ctl -t 0 = чекати безмежно → заборонено (clamp у 1с)
        assert_eq!(start_timeout_from(Some("0")), Duration::from_secs(1));
        assert_eq!(start_timeout_from(Some("99999")), Duration::from_secs(600));
        assert_eq!(start_timeout_from(Some("сміття")), start_timeout_from(None));
        assert!(start_timeout_from(None) >= Duration::from_secs(30));
    }

    /// РЕАЛЬНЕ виконання: стаб-`psql` записує фактичну командну лінію та env.
    #[cfg(unix)]
    #[test]
    fn ensure_database_real_psql_call_has_no_password_and_connect_timeout() {
        let dir = temp_data_dir();
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let log = dir.join("psql.log");
        write_stub(
            &bin.join(psql_name()),
            &format!(
                "#!/bin/sh\necho \"argv: $* | PGCONNECT_TIMEOUT=${{PGCONNECT_TIMEOUT:-}} | PGPASSWORD=${{PGPASSWORD:-}}\" >> \"{}\"\nexit 0\n",
                log.display()
            ),
        );
        let pg = EmbeddedPostgres::with_data_dir(bin, dir.join("pgdata"));
        pg.ensure_database()
            .expect("стаб-psql завершується успішно");
        let recorded = std::fs::read_to_string(&log).expect("лог викликів psql");
        assert!(
            recorded.contains(" -w "),
            "реальний виклик psql мусить містити -w: {recorded}"
        );
        assert!(
            recorded.contains(&format!("PGCONNECT_TIMEOUT={PSQL_CONNECT_TIMEOUT_SECS}")),
            "реальний виклик psql мусить мати PGCONNECT_TIMEOUT: {recorded}"
        );
        assert!(recorded.contains("-U postgres"), "{recorded}");
        assert!(
            recorded.lines().count() >= 2,
            "ensure_database = перевірка + CREATE DATABASE: {recorded}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// РЕАЛЬНЕ виконання: стаб-`pg_ctl` фіксує аргументи `-w -t <n>`.
    #[cfg(unix)]
    #[test]
    fn pg_ctl_start_real_call_is_time_bounded() {
        // Таймаут 1 с — щоб поллінг готовності в start_once не тривав 30 с.
        std::env::set_var(START_TIMEOUT_ENV, "1");
        let dir = temp_data_dir();
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let log = dir.join("pg_ctl.log");
        write_stub(
            &bin.join(pg_ctl_name()),
            &format!(
                "#!/bin/sh\necho \"pg_ctl $*\" >> \"{}\"\nexit 0\n",
                log.display()
            ),
        );
        // Крихкість (виявлено в CI): `start_once` після стаб-`pg_ctl` поллить TCP
        // до 127.0.0.1:5433 і на вільному порту віддає `StartTimeout`, тобто
        // `started_by_us` лишається true, але тест залежав від того, чи вже
        // слухає 5433 ЩОСЬ на машині (локально — випадково проходив, у CI —
        // падав). Тримаємо порт самі: заглушка-`TcpListener` без `accept`
        // достатня, бо `port_is_open()` робить лише `connect` (backlog).
        let _fake_ready_server = std::net::TcpListener::bind(("127.0.0.1", EMBEDDED_PG_PORT)).ok();
        let mut pg = EmbeddedPostgres::with_data_dir(bin, dir.join("pgdata"));
        // start_once — напряму (start() пропустив би старт, якби 5433 слухав).
        let _ = pg.start_once();
        let recorded = std::fs::read_to_string(&log).expect("лог викликів pg_ctl");
        assert!(recorded.contains("start"), "{recorded}");
        assert!(
            recorded.contains("-w") && recorded.contains("-t 1"),
            "pg_ctl start мусить бути обмежений нашим таймаутом (-w -t 1): {recorded}"
        );
        drop(pg); // Drop → pg_ctl stop (стаб) — теж обмежений
        let recorded = std::fs::read_to_string(&log).expect("лог викликів pg_ctl (stop)");
        assert!(
            recorded.contains("stop") && recorded.contains("-t 1"),
            "pg_ctl stop мусить бути обмежений таймаутом: {recorded}"
        );
        std::env::remove_var(START_TIMEOUT_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── ДЕФЕКТ 5б: pg_ctl НЕ має висіти на pipe, який тримає демон ─────────

    /// РЕГРЕСІЯ (дефект 5б; Windows-семантика, відтворена на Linux).
    ///
    /// Стаб-`pg_ctl` спавнить ФОНОВИЙ довгоживучий процес, який успадковує
    /// stdio батька, і одразу виходить з кодом 0 — точна модель `pg_ctl start`
    /// (він лишає жити `postgres.exe`, який тримає успадкований pipe).
    /// Зі старим `.output()` (piped stdio) читання pipe НІКОЛИ не бачить EOF →
    /// блокування назавжди (на живій Windows-касі: журнал обривався на
    /// `data_dir`, фасад :8000 без `axum::serve`).
    /// З фіксом (stdout=null, stderr=файл) виклик повертається за секунди.
    #[cfg(unix)]
    #[test]
    fn pg_ctl_start_does_not_block_on_daemon_holding_stdio() {
        let dir = temp_data_dir();
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        // `sleep 30 &` успадковує stdio стаба і живе далі після його виходу.
        write_stub(
            &bin.join(pg_ctl_name()),
            "#!/bin/sh\nsleep 30 &\necho \"pg_ctl $*\"\nexit 0\n",
        );
        let mut pg = EmbeddedPostgres::with_data_dir(bin, dir.join("pgdata"));
        let started = Instant::now();
        // Реальний код-шлях: стаб → код 0 → поллінг готовності (1 с) → вихід.
        let _ = pg.start_once_with_timeout(Duration::from_secs(1));
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "start_once завис на {elapsed:?}: демон успадкував pipe (дефект 5б)"
        );
        drop(pg); // Drop → stop(): порт відкритий → pg_ctl stop (стаб, null-stdio)
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// stderr `pg_ctl` ПОТРАПЛЯЄ у `<data_dir>/pg_ctl.log` (stdio → файл) і
    /// читається у текст помилки (дефект 5б, п.3 — stderr не «проковтувати»).
    #[cfg(unix)]
    #[test]
    fn pg_ctl_stderr_is_captured_in_log_and_error() {
        let dir = temp_data_dir();
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        write_stub(
            &bin.join(pg_ctl_name()),
            "#!/bin/sh\necho 'pg_ctl: could not start server' >&2\nexit 1\n",
        );
        let pgdata = dir.join("pgdata");
        let mut pg = EmbeddedPostgres::with_data_dir(bin, pgdata.clone());
        let err = pg
            .start_once_with_timeout(Duration::from_secs(1))
            .expect_err("стаб-pg_ctl виходить з кодом 1");
        let log = std::fs::read_to_string(pgdata.join(PG_CTL_LOG_NAME)).expect("pg_ctl.log");
        assert!(
            log.contains("pg_ctl: could not start server"),
            "stderr pg_ctl мусить бути у pg_ctl.log: {log}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("pg_ctl: could not start server"),
            "хвіст pg_ctl.log мусить бути у тексті помилки: {msg}"
        );
        assert!(
            matches!(err, Error::ExitWithOutput { code: 1, .. }),
            "{err:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Записує виконуваний стаб-бінарник (Unix).
    #[cfg(unix)]
    fn write_stub(path: &Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, content).expect("стаб-скрипт");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod +x");
    }
}
