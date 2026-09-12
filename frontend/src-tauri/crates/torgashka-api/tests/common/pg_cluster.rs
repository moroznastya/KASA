//! Спільний каркас e2e, яким потрібен ВЛАСНИЙ тимчасовий кластер PostgreSQL 17.
//!
//! Навіщо окремий файл (а не `common/mod.rs`): `mod common;` підключають 34
//! тест-бінарники, і будь-який новий публічний елемент там стає dead-code у
//! тих, хто його не вживає (а `clippy -D warnings` це ловить). Патерн репозиторію
//! для спільних e2e-хелперів — окремий файл у `tests/common/`, який тест
//! підключає як `#[path = "common/pg_cluster.rs"] mod pg_cluster;` — саме так
//! зроблено з `hub_env.rs` і `sync_schema.rs` (див. шапку `sync_schema.rs`:
//! «common/mod.rs не чіпається — він лише force_test_db»).
//!
//! Що дає:
//!   * [`resolve_port`] — вибір вільного порту для кластера тесту: явний
//!     `TORGASHKA_PG_TEST_PORT` → як є (зайнятий = аномалія вгору з доказами);
//!     інакше порт продукту (5433), якщо вільний; інакше АВТОМАТИЧНО вільний
//!     порт із діапазону 5434..=5500 (жодної паніки — зайнятий 5433 не привід
//!     червонити тест). Порти заявляються в межах процесу, тому два тести
//!     одного бінарника ніколи не ділять номер і можуть іти паралельно.
//!   * [`TestCluster`] — реальний кластер `initdb` + `pg_ctl` у `/tmp`
//!     (trust-auth, сокет у каталозі тесту), створення додаткових БД, `psql`
//!     тим самим способом підключення, що в проді (`psql_conn_args`/`env`),
//!     і доказові рядки в лог.

#![allow(dead_code)] // кожен e2e бере свою підмножину хелперів

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use torgashka_infrastructure::embedded_pg::{
    pg_ctl_start_args, pg_ctl_stop_args, psql_conn_args, psql_conn_env, EmbeddedPostgres,
};

/// БД, яку [`TestCluster::start`] створює одразу (робоча БД тесту).
pub const MAIN_DB: &str = "torgashka";
/// Верхня межа перебору власних портів тесту (5433 зайнятий — беремо 5434..5500).
pub const PORT_SCAN_END: u16 = 5500;

/// Рядок доказової лінії тесту: `[r2][evidence] …` / `[r3][evidence] …`.
pub fn evidence(tag: &str, line: &str) {
    eprintln!("{tag}[evidence] {line}");
}

/// Шлях до інструмента в каталозі бінарників (той самий `.exe`-контракт, що в
/// `embedded_pg`: Windows-збірки PG кладуть `.exe`).
pub fn tool(bin_dir: &Path, base: &str) -> PathBuf {
    if cfg!(windows) {
        bin_dir.join(format!("{base}.exe"))
    } else {
        bin_dir.join(base)
    }
}

/// Версія інструмента одним рядком (для доказу «саме 17.x»).
pub fn tool_version(program: &Path) -> String {
    match Command::new(program).arg("--version").output() {
        Ok(o) => format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout).trim(),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => format!("не запустився: {e}"),
    }
}

/// Чи слухає хтось `127.0.0.1:<port>` (та сама перевірка, що `port_is_open`).
pub fn port_open(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Доказова вибірка про власника порту — для ANOMALY-блоку (не для гадання).
pub fn port_evidence(port: u16) -> String {
    let mut out = String::new();
    for (label, cmd, args) in [
        ("ss -ltnp", "ss", vec!["-ltnp"]),
        ("pg_lsclusters", "pg_lsclusters", vec![]),
    ] {
        let output = Command::new(cmd).args(&args).output();
        match output {
            Ok(o) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                let filtered: Vec<&str> = text
                    .lines()
                    .filter(|l| l.contains(&port.to_string()) || label == "pg_lsclusters")
                    .collect();
                out.push_str(&format!("\n  {label}:\n    {}\n", filtered.join("\n    ")));
            }
            Err(e) => out.push_str(&format!("\n  {label}: недоступно ({e})\n")),
        }
    }
    // Хто саме тримає сокет: /proc/net/tcp дає uid власника (доказ, а не здогад).
    if let Ok(tcp) = std::fs::read_to_string("/proc/net/tcp") {
        let hex = format!("{port:04X}");
        for line in tcp.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() > 7 && cols[1].ends_with(&format!(":{hex}")) {
                out.push_str(&format!(
                    "  /proc/net/tcp: local={} uid={} inode={}\n",
                    cols[1], cols[7], cols[9]
                ));
            }
        }
    }
    out
}

/// Порти, заявлені тестами цього бінарника (стан у межах процесу): паралельні
/// тести ніколи не ділять один номер порту.
static CLAIMED_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());
/// Рядок `[tag][info]` про вибір власного порту друкуємо рівно один раз.
static FALLBACK_REPORTED: AtomicBool = AtomicBool::new(false);

/// Порт тестового кластера (порядок вибору — у шапці файла).
///
/// * `tag` — префікс рядків (`"[r2]"`/`"[r3]"`);
/// * `port_env` — env із ЯВНИМ портом (`TORGASHKA_PG_TEST_PORT`); заданий і
///   зайнятий = аномалія вгору (паніка з доказами);
/// * `default_port` — порт продукту (5433), який беремо, якщо вільний.
///
/// Паніка в гілці автопідбору неможлива, поки є хоч один вільний порт
/// у `default_port+1..=PORT_SCAN_END`; якщо вільного немає взагалі — це
/// аномалія вгору з вибіркою власників портів.
pub fn resolve_port(tag: &str, port_env: &str, default_port: u16) -> u16 {
    let explicit = std::env::var(port_env)
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok());
    let mut claimed = CLAIMED_PORTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(port) = explicit {
        if port_open(port) {
            let ev = port_evidence(port);
            panic!(
                "{tag}[ANOMALY] порт {port} зайнятий, але його задано ЯВНО через {port_env} — \
                 продовжувати на чужому порту небезпечно (під ним може бути робочий кластер). \
                 Власник порту:{ev}\n\
                 Задайте вільний порт (напр. {port_env}=5434) або приберіть змінну — тоді тест \
                 підбере вільний порт сам."
            );
        }
        claimed.push(port);
        return port;
    }

    if !claimed.contains(&default_port) && !port_open(default_port) {
        claimed.push(default_port);
        return default_port;
    }

    // Порт продукту недоступний (зайнятий ззовні або вже заявлений сусіднім
    // тестом) — беремо власний вільний порт. Паніки тут НЕМАЄ.
    let scan_end = PORT_SCAN_END.max(default_port + 1);
    let picked = (default_port + 1..=scan_end)
        .find(|candidate| !claimed.contains(candidate) && !port_open(*candidate))
        .unwrap_or_else(|| {
            panic!(
                "{tag}[ANOMALY] жоден порт {}..={scan_end} не вільний — власний кластер тесту \
                 підняти неможливо. Власники зайнятих портів:{}{}{}",
                default_port + 1,
                port_evidence(default_port),
                port_evidence(default_port + 1),
                port_evidence(scan_end),
            )
        });
    claimed.push(picked);
    if !FALLBACK_REPORTED.swap(true, Ordering::SeqCst) {
        if port_open(default_port) {
            eprintln!(
                "{tag}[info] {default_port} зайнятий (системний кластер) → власний кластер тесту на порту {picked}"
            );
        } else {
            eprintln!(
                "{tag}[info] порт {default_port} закріплено за іншим тестом цього бінарника → власний кластер тесту на порту {picked}"
            );
        }
    }
    picked
}

/// Власний тимчасовий кластер PG 17 (initdb + pg_ctl) для наскрізних тестів.
pub struct TestCluster {
    /// Префікс доказових рядків тесту (`[r2]`, `[r3]` …).
    tag: &'static str,
    pub bin_dir: PathBuf,
    pub data_dir: PathBuf,
    pub log: PathBuf,
    pub port: u16,
}

impl TestCluster {
    /// Піднімає кластер у `/tmp/<dir_prefix>_<pid>_<port>` і створює [`MAIN_DB`].
    pub fn start(tag: &'static str, dir_prefix: &str, bin_dir: PathBuf, port: u16) -> Self {
        let data_dir =
            std::env::temp_dir().join(format!("{dir_prefix}_{}_{port}", std::process::id()));
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir).expect("прибирання старого каталогу тесту");
        }
        std::fs::create_dir_all(&data_dir).expect("каталог даних тесту");
        let log = data_dir.join("pg_ctl.log");

        let initdb = tool(&bin_dir, "initdb");
        let out = Command::new(&initdb)
            .args([
                "-D",
                &data_dir.to_string_lossy(),
                "-U",
                "postgres",
                "-A",
                "trust",
                "--encoding=UTF8",
            ])
            .output()
            .unwrap_or_else(|e| panic!("initdb {}: {e}", initdb.display()));
        assert!(
            out.status.success(),
            "initdb не вдався: код {:?}\nstdout:\n{}\nstderr:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );

        // Ті самі аргументи pg_ctl, що в проді (`pg_ctl_start_args`), плюс
        // `-k <data_dir>`: сокет — у каталозі тесту, щоб не сваритися з
        // системними кластерами.
        let opts = format!("-p {port} -h 127.0.0.1 -k {}", data_dir.display());
        let args = pg_ctl_start_args(&data_dir, &log, &opts, 60);
        let pg_ctl = tool(&bin_dir, "pg_ctl");
        let out = Command::new(&pg_ctl)
            .args(&args)
            .output()
            .unwrap_or_else(|e| panic!("pg_ctl {}: {e}", pg_ctl.display()));
        assert!(
            out.status.success(),
            "pg_ctl start не вдався: код {:?}\nstderr:\n{}\nлог {}:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
            log.display(),
            std::fs::read_to_string(&log).unwrap_or_default()
        );

        let cluster = Self {
            tag,
            bin_dir,
            data_dir,
            log,
            port,
        };
        // Робоча БД — створюємо так, як це робить застосунок (psql + psql_conn_args).
        let (code, out_text, err_text) =
            cluster.psql("postgres", &format!("CREATE DATABASE {MAIN_DB}"));
        assert_eq!(
            code,
            Some(0),
            "CREATE DATABASE {MAIN_DB}: код {code:?}\n{out_text}\n{err_text}"
        );
        cluster
    }

    pub fn url(&self) -> String {
        self.url_for(MAIN_DB)
    }

    /// URL довільної БД того самого кластера тесту.
    pub fn url_for(&self, db: &str) -> String {
        format!("postgresql://postgres@127.0.0.1:{}/{db}", self.port)
    }

    /// Створює додаткову БД у кластері тесту (напр. для фасаду-«хаба», якому
    /// потрібна власна робоча схема й аж ніяк не БД вузла).
    pub fn create_db(&self, name: &str) {
        let (code, out, err) = self.psql("postgres", &format!("CREATE DATABASE {name}"));
        assert_eq!(code, Some(0), "CREATE DATABASE {name}: {out} {err}");
    }

    /// `psql` тим самим способом підключення, що в проді (`psql_conn_args`/`env`).
    pub fn psql(&self, db: &str, sql: &str) -> (Option<i32>, String, String) {
        let psql = tool(&self.bin_dir, "psql");
        let mut command = Command::new(psql);
        command.args(psql_conn_args("postgres", db, self.port));
        for (k, v) in psql_conn_env() {
            command.env(k, v);
        }
        command.arg("-c").arg(sql);
        match command.output() {
            Ok(o) => (
                o.status.code(),
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
            ),
            Err(e) => (None, String::new(), format!("psql не запустився: {e}")),
        }
    }

    /// Виконує SQL у [`MAIN_DB`] і повертає сирий вивід у лог (доказова лінія).
    pub fn psql_evidence(&self, title: &str, sql: &str) {
        let (code, out, err) = self.psql(MAIN_DB, sql);
        evidence(
            self.tag,
            &format!("{title}\n    SQL: {sql}\n    psql exit={code:?}"),
        );
        for line in out.trim_end().lines() {
            evidence(self.tag, &format!("    {line}"));
        }
        if !err.trim().is_empty() {
            evidence(self.tag, &format!("    stderr: {}", err.trim()));
        }
    }

    pub fn stop(&self) {
        let pg_ctl = tool(&self.bin_dir, "pg_ctl");
        let args = pg_ctl_stop_args(&self.data_dir, 30);
        match Command::new(pg_ctl).args(&args).output() {
            Ok(o) => evidence(
                self.tag,
                &format!("зупинка тестового кластера: код {:?}", o.status.code()),
            ),
            Err(e) => evidence(self.tag, &format!("зупинка тестового кластера: {e}")),
        }
    }
}

impl Drop for TestCluster {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.data_dir);
        let _ = std::fs::remove_file(&self.log);
    }
}

/// Реальний каталог бінарників PG + вибірка версій інструментів.
///
/// `required_major` (напр. `"17."`) перевіряється ЖОРСТКО: дамп `Dump Version
/// 1.16` системний `pg_restore` 16.x не читає, тому збірка «не те, що треба» —
/// це аномалія, а не «тест сам якось розбереться».
pub fn locate_binaries(tag: &str, required_major: &str) -> PathBuf {
    let Some(bin_dir) = EmbeddedPostgres::locate() else {
        panic!(
            "{tag}[ANOMALY] бінарники PostgreSQL не знайдено (задайте \
             TORGASHKA_PG_DIR=/usr/lib/postgresql/17/bin)"
        );
    };
    let initdb = tool(&bin_dir, "initdb");
    let pg_restore = tool(&bin_dir, "pg_restore");
    let pg_dump = tool(&bin_dir, "pg_dump");
    let initdb_v = tool_version(&initdb);
    let restore_v = tool_version(&pg_restore);
    evidence(
        tag,
        &format!("каталог бінарників PG: {}", bin_dir.display()),
    );
    evidence(tag, &format!("initdb:    {initdb_v}"));
    evidence(tag, &format!("pg_restore: {restore_v}"));
    evidence(tag, &format!("pg_dump:   {}", tool_version(&pg_dump)));
    assert!(
        restore_v.contains(required_major) && initdb_v.contains(required_major),
        "{tag}[ANOMALY] потрібні бінарники PG {required_major} (той самий major, що в сервера): \
         initdb='{initdb_v}', pg_restore='{restore_v}'"
    );
    bin_dir
}
