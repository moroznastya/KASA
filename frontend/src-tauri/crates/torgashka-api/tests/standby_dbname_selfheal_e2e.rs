//! E2E: САМОЛІКУВАННЯ імені локальної БД на standby-касі (контракт «standby
//! фасад САМ визначає ім'я репліки», гілка sync-offline).
//!
//! Прод-ситуація: `[node] primary_db_url` НЕ зберігся (провіжн обірвався), env
//! `TORGASHKA_PG_DB` не задано, оператора немає → фасад раніше НЕ встановлював
//! `DATABASE_URL` (503 назавжди). Тепер добирає ім'я САМ:
//!   (c) SQLite settings `node_replication_database` (зберіг join-екран);
//!   (d) проба ЖИВОЇ репліки на `127.0.0.1:{local_port}`: підключення БЕЗ
//!       пароля до БД `postgres` + `SELECT datname FROM pg_database WHERE NOT
//!       datistemplate AND datname <> 'postgres' ORDER BY datname`; рівно одна
//!       БД → ім'я, 0 або ≥2 → чесна відмова з ПЕРЕЛІКОМ (жодних здогадок).
//!
//! АНОМАЛІЯ СЕРЕДОВИЩА (зафіксована, див. звіт): системні PG цієї машини
//! (127.0.0.1:5432 і :5433) вимагають пароль по TCP (pg_hba scram), тому
//! контрактний сценарій «підключення БЕЗ пароля» на них неможливий. Тест
//! піднімає ВЛАСНИЙ справжній PG-кластер (`initdb -A trust` + `pg_ctl start`,
//! бінарники postgresql-16/17 цієї машини) на ефемерному порту — саме так
//! виглядає прод-репліка (trust на localhost), і саме так перевіряються обидва
//! джерела (c) і (d) НА РЕАЛЬНОМУ PostgreSQL.
//!
//! Перевіряється:
//!   1. (c): план = `StandbyWithoutDbName` → `DATABASE_URL =
//!      postgresql://postgres@127.0.0.1:{local_port}/pos_system_fresh`, у
//!      `db_sources.toml` з'явився `[node] primary_db_url` з тим самим іменем БД;
//!   2. (d): без `node_replication_database` ім'я добирає ПРОБА живої репліки
//!      (рівно одна не-шаблонна БД);
//!   3. фасад на цьому URL реально обслуговує: `/api/v1/setup/status` ≠ 503;
//!   4. повторний запуск ідемпотентний: файл не зіпсовано, `mode="standby"`,
//!      той самий URL.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Справжній тимчасовий PG-кластер (trust на 127.0.0.1) — «локальна репліка»
/// тесту. Зупиняється у `Drop`, дані — у tmp-каталозі.
struct TempPg {
    _root: tempfile::TempDir,
    bin: PathBuf,
    data: PathBuf,
    sock: PathBuf,
    port: u16,
    started: bool,
}

impl TempPg {
    /// Каталог бінарників postgres: PATH → /usr/lib/postgresql/{17,16}/bin.
    fn bin_dir() -> PathBuf {
        let candidates: Vec<PathBuf> = vec![
            PathBuf::from("/usr/lib/postgresql/17/bin"),
            PathBuf::from("/usr/lib/postgresql/16/bin"),
            PathBuf::from("/usr/lib/postgresql/15/bin"),
        ];
        for c in &candidates {
            if c.join("initdb").is_file() {
                return c.clone();
            }
        }
        // PATH (напр. власна збірка embedded PG).
        if let Ok(path) = std::env::var("PATH") {
            for dir in path.split(':') {
                let p = Path::new(dir).join("initdb");
                if p.is_file() {
                    return PathBuf::from(dir);
                }
            }
        }
        panic!(
            "у середовищі немає справжніх PG-бінарників (initdb) — e2e самолікування \
             неможливий: перевірте /usr/lib/postgresql/*/bin"
        );
    }

    fn start() -> Self {
        let bin = Self::bin_dir();
        let root = tempfile::tempdir().expect("tempdir PG");
        // Ефемерний порт: біндимо :0 і звільняємо (кластер стартує за мс).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("ефемерний порт");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);

        let data = root.path().join("pgdata");
        let out = Command::new(bin.join("initdb"))
            .args([
                "-D",
                data.to_str().expect("path"),
                "-U",
                "postgres",
                "-A",
                "trust",
                "--no-sync",
                "-E",
                "UTF8",
            ])
            .output()
            .expect("initdb запущено");
        assert!(
            out.status.success(),
            "initdb провалився: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        // Сокет — у tmp (дефолтний /var/run/postgresql недоступний звичайному
        // користувачу); сам тест ходить TCP-ом на 127.0.0.1, як прод-репліка.
        let sock = root.path().join("sock");
        std::fs::create_dir_all(&sock).expect("каталог сокета");

        let pg = TempPg {
            _root: root,
            bin,
            data,
            sock,
            port,
            started: false,
        };
        let out = Command::new(pg.bin.join("pg_ctl"))
            .args([
                "-D",
                pg.data.to_str().expect("path"),
                "-o",
                &format!(
                    "-p {port} -c listen_addresses=127.0.0.1 -c unix_socket_directories={} -c fsync=off",
                    pg.sock.to_str().expect("path")
                ),
                "-l",
                pg._root.path().join("pg.log").to_str().expect("path"),
                "-w",
                "-t",
                "30",
                "start",
            ])
            .output();
        let mut pg = pg;
        pg.started = match out {
            Ok(o) => {
                if !o.status.success() {
                    let log =
                        std::fs::read_to_string(pg._root.path().join("pg.log")).unwrap_or_default();
                    panic!(
                        "pg_ctl start провалився: {}{} (лог: {log})",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                }
                true
            }
            Err(e) => panic!("pg_ctl не запущено: {e}"),
        };
        pg
    }

    /// Passwordless DSN до службової БД `postgres` (як у прод-репліці: trust).
    fn admin_dsn(&self) -> String {
        format!("postgresql://postgres@127.0.0.1:{}/postgres", self.port)
    }

    async fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(pool) = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(2))
                .connect(&self.admin_dsn())
                .await
            {
                let ok = sqlx::query_scalar::<_, i32>("SELECT 1")
                    .fetch_one(&pool)
                    .await
                    .is_ok();
                pool.close().await;
                if ok {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "PG-кластер не піднявся за 30 с");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

impl Drop for TempPg {
    fn drop(&mut self) {
        if self.started {
            let _ = Command::new(self.bin.join("pg_ctl"))
                .args([
                    "-D",
                    self.data.to_str().expect("path"),
                    "-m",
                    "immediate",
                    "-w",
                    "-t",
                    "20",
                    "stop",
                ])
                .output();
        }
    }
}

/// Створює БД `pos_system_fresh` на тестовому кластері + схему (сид e2e).
async fn create_replica_db(pg: &TempPg, name: &str) {
    assert!(
        name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "ім'я БД для CREATE DATABASE мусить бути простим: {name}"
    );
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&pg.admin_dsn())
        .await
        .expect("службова БД 'postgres' тестового кластера");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("CREATE DATABASE репліки");
    admin.close().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&format!(
            "postgresql://postgres@127.0.0.1:{}/{name}",
            pg.port
        ))
        .await
        .expect("БД репліки");
    torgashka_infrastructure::db::ensure_schema(&pool)
        .await
        .expect("ensure_schema на БД репліки");
    pool.close().await;
}

/// Сирий HTTP/1.1 GET (без зовнішніх залежностей, стиль facade_boot_gate_e2e).
fn raw_http_get(addr: &str, path: &str, timeout: Duration) -> (String, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).expect("TCP connect до фасаду");
    stream
        .set_read_timeout(Some(timeout))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(timeout))
        .expect("write timeout");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .expect("запит надіслано");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let status = text.lines().next().unwrap_or("<жодного байта>").to_string();
    (status, text)
}

/// Стаб-бінарник PG (щоб `ensure_local_replica_running` не торкався реальних
/// кластерів машини).
fn write_stub(path: &Path, label: &str, log: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\necho \"{label} $*\" >> \"{}\"\nexit 0\n",
        log.display()
    );
    std::fs::write(path, script).expect("стаб-скрипт");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod +x");
}

/// Записує standby-налаштування у SQLite settings каси (реальна offline.db).
fn seed_settings(path: &Path, rows: &[(&str, &str)]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("каталог offline.db");
    }
    // open_connection виконує прод-міграції → таблиця settings має прод-схему.
    let conn = torgashka_infrastructure::offline::sync_push::open_connection(path)
        .expect("offline.db + міграції");
    for (k, v) in rows {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            rusqlite::params![k, v],
        )
        .expect("settings");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn standby_selfheals_local_db_name_and_persists_primary_url() {
    // ── 0. Реальний локальний PG-«репліка» (trust, passwordless) ───────────
    let pg = TempPg::start();
    pg.wait_ready().await;
    let db_name = "pos_system_fresh";
    create_replica_db(&pg, db_name).await;
    // Той самий запит, що й у пробі: рівно одна не-шаблонна БД.
    let probe_names: Vec<String> = {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&pg.admin_dsn())
            .await
            .expect("admin pool");
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT datname FROM pg_database \
             WHERE NOT datistemplate AND datname <> 'postgres' ORDER BY datname",
        )
        .fetch_all(&pool)
        .await
        .expect("перелік БД репліки");
        pool.close().await;
        rows
    };
    eprintln!(
        "[selfheal-e2e] PG-світ: порт {}, БД [{}]",
        pg.port,
        probe_names.join(", ")
    );
    assert_eq!(probe_names, vec![db_name.to_string()]);

    // ── 1. Ізоляція: tmp-каталог; [node] mode=standby БЕЗ primary_db_url ────
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path().to_path_buf();
    std::env::set_var("XDG_DATA_HOME", &root_path);
    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("TEST_DATABASE_URL");
    std::env::remove_var("TORGASHKA_PG_DB");
    std::env::set_var("TORGASHKA_PG_USER", "postgres");

    let cfg_path = root_path.join("Torgashka/db_sources.toml");
    std::fs::create_dir_all(cfg_path.parent().expect("каталог конфіга")).expect("mkdir");
    std::fs::write(
        &cfg_path,
        format!("[node]\nmode = \"standby\"\nlocal_port = {}\n", pg.port),
    )
    .expect("db_sources.toml");
    std::env::set_var("TORGASHKA_DB_SOURCES", &cfg_path);

    // Стаби PG: гілка standby не має торкатись чужих кластерів.
    let bin = root_path.join("pgbin");
    std::fs::create_dir_all(&bin).expect("pgbin");
    let pg_calls = root_path.join("pg_calls.log");
    write_stub(&bin.join("initdb"), "initdb", &pg_calls);
    write_stub(&bin.join("pg_ctl"), "pg_ctl", &pg_calls);
    write_stub(&bin.join("psql"), "psql", &pg_calls);
    std::env::set_var("TORGASHKA_PG_DIR", &bin);
    std::env::set_var("TORGASHKA_PG_START_TIMEOUT_SECS", "2");
    let pgdata = root_path.join("Torgashka/pgdata");
    std::fs::create_dir_all(&pgdata).expect("pgdata");
    std::fs::write(pgdata.join("PG_VERSION"), "17\n").expect("PG_VERSION");

    // ── 2. (c): SQLite settings node_replication_database (join виконано) ───
    let offline_path = torgashka_infrastructure::offline::db::OfflineDatabase::default_db_path()
        .expect("шлях offline.db");
    seed_settings(
        &offline_path,
        &[
            ("node_node_id", "e2e-selfheal-node"),
            ("node_replication_database", db_name),
            ("node_replication_host", "127.0.0.1"),
            ("node_replication_port", &pg.port.to_string()),
        ],
    );
    {
        let st = torgashka_infrastructure::standby_heartbeat::read_standby_settings()
            .expect("читання settings")
            .expect("join виконано");
        assert_eq!(st.replication_database.as_deref(), Some(db_name));
    }
    let expected_url = format!("postgresql://postgres@127.0.0.1:{}/{db_name}", pg.port);

    // План: ні primary_db_url, ні TORGASHKA_PG_DB → StandbyWithoutDbName.
    let plan = torgashka_api::plan_db_startup(None, true, None);
    assert!(
        matches!(plan, torgashka_api::DbStartupPlan::StandbyWithoutDbName(_)),
        "очікували StandbyWithoutDbName, отримали {plan:?}"
    );
    let guard = torgashka_api::apply_db_startup_plan(plan).await;
    assert!(guard.is_none(), "standby-реплікою фасад не володіє");
    assert_eq!(
        std::env::var("DATABASE_URL").unwrap_or_default(),
        expected_url,
        "самолікування з (c) має дати локальний URL без пароля"
    );
    assert!(!expected_url.contains(':') || !expected_url.contains(":@"));

    // ── 3. [node] primary_db_url записано (mode/local_port не зіпсовано) ────
    let file_text = std::fs::read_to_string(&cfg_path).expect("db_sources.toml");
    eprintln!("[selfheal-e2e] db_sources.toml після (c):\n{file_text}");
    let toml_v: toml::Value = toml::from_str(&file_text).expect("валідний toml");
    let node = toml_v.get("node").expect("секція [node]");
    assert_eq!(node.get("mode").and_then(|v| v.as_str()), Some("standby"));
    assert_eq!(
        node.get("local_port").and_then(|v| v.as_integer()),
        Some(pg.port as i64)
    );
    let healed_primary = node
        .get("primary_db_url")
        .and_then(|v| v.as_str())
        .expect("primary_db_url записано")
        .to_string();
    assert!(
        healed_primary.ends_with(&format!("/{db_name}")),
        "primary_db_url мусить містити ТЕ САМЕ ім'я БД: {healed_primary}"
    );

    // ── 4. (d): без settings-імені працює ПРОБА живої репліки ───────────────
    std::env::remove_var("DATABASE_URL");
    std::fs::write(
        &cfg_path,
        format!("[node]\nmode = \"standby\"\nlocal_port = {}\n", pg.port),
    )
    .expect("скидання [node] primary_db_url");
    seed_settings(
        &offline_path,
        &[
            ("node_replication_database", ""),
            ("node_replication_host", ""),
        ],
    );
    let plan_probe = torgashka_api::plan_db_startup(None, true, None);
    assert!(matches!(
        plan_probe,
        torgashka_api::DbStartupPlan::StandbyWithoutDbName(_)
    ));
    torgashka_api::apply_db_startup_plan(plan_probe).await;
    assert_eq!(
        std::env::var("DATABASE_URL").unwrap_or_default(),
        expected_url,
        "ім'я БД мусить прийти з ПРОБИ живої репліки (рівно одна не-шаблонна БД)"
    );
    let after_probe = std::fs::read_to_string(&cfg_path).expect("db_sources.toml");
    let toml_probe: toml::Value = toml::from_str(&after_probe).expect("валідний toml");
    let healed_probe = toml_probe
        .get("node")
        .and_then(|n| n.get("primary_db_url"))
        .and_then(|v| v.as_str())
        .expect("primary_db_url після проби")
        .to_string();
    assert!(
        healed_probe.ends_with(&format!("/{db_name}")),
        "{healed_probe}"
    );

    // ── 5. Фасад на самолікованому URL обслуговує: /setup/status ≠ 503 ──────
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("бінд ефемерного порту");
    let addr = listener.local_addr().expect("local_addr").to_string();
    let handle = tokio::spawn(async move {
        let _ = torgashka_api::serve_listener(listener).await;
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let (last_status, last_body) = loop {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let addr_probe = addr.clone();
        let (status, body) = tokio::task::spawn_blocking(move || {
            raw_http_get(&addr_probe, "/api/v1/setup/status", Duration::from_secs(5))
        })
        .await
        .expect("проба фасаду");
        if status.contains("200") {
            break (status, body);
        }
        assert!(
            Instant::now() < deadline,
            "фасад не віддав /api/v1/setup/status ≠ 503: {status} {body}"
        );
    };
    eprintln!("[selfheal-e2e] /api/v1/setup/status → {last_status}");
    assert!(
        !last_status.contains("503"),
        "статус мусить бути ≠ 503 (ім'я БД відоме): {last_status} {last_body}"
    );
    handle.abort();

    // ── 6. Повторний запуск: ідемпотентно, файл не псується, URL той самий ──
    let before = std::fs::read_to_string(&cfg_path).expect("toml перед повторним запуском");
    std::env::remove_var("DATABASE_URL");
    let cfg_reload = torgashka_infrastructure::node_config::NodeConfig::load();
    assert!(cfg_reload.is_standby(), "mode=standby після перезапису");
    let primary = cfg_reload
        .resolve_primary_db_url()
        .expect("primary_db_url читається з файлу");
    assert_eq!(primary, healed_probe, "URL не змінився між запусками");
    let plan2 = torgashka_api::plan_db_startup(None, true, Some(primary.clone()));
    assert!(
        matches!(plan2, torgashka_api::DbStartupPlan::StandbyReplica(_)),
        "другий запуск іде гілкою репліки (ім'я вже відоме): {plan2:?}"
    );
    torgashka_api::apply_db_startup_plan(plan2).await;
    assert_eq!(
        std::env::var("DATABASE_URL").unwrap_or_default(),
        expected_url,
        "той самий DATABASE_URL на другому запуску"
    );
    let after = std::fs::read_to_string(&cfg_path).expect("toml після повторного запуску");
    assert_eq!(before, after, "файл не псується повторним запуском");
    let toml2: toml::Value = toml::from_str(&after).expect("валідний toml після повтору");
    assert_eq!(
        toml2
            .get("node")
            .and_then(|n| n.get("mode"))
            .and_then(|v| v.as_str()),
        Some("standby")
    );
    assert_eq!(
        toml2
            .get("node")
            .and_then(|n| n.get("primary_db_url"))
            .and_then(|v| v.as_str()),
        Some(healed_probe.as_str())
    );

    // ── 7. Репліку не чіпали bootstrap-ом (initdb заборонений) ──────────────
    let calls = std::fs::read_to_string(&pg_calls).unwrap_or_default();
    assert!(
        !calls.contains("initdb"),
        "standby: initdb на каталозі репліки ЗАБОРОНЕНИЙ: {calls}"
    );
    let _: PathBuf = cfg_path;
}
