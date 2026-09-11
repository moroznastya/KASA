//! ДЕФЕКТ 6: standby-вузол НЕ виконує primary-bootstrap (initdb / CREATE
//! DATABASE) на каталозі репліки — лише ідемпотентний старт локальної репліки
//! і `DATABASE_URL` на неї (без пароля).
//!
//! Доведення: `apply_db_startup_plan(StandbyReplica(url))` — та сама функція,
//! яку викликає ініціалізація фасаду — запускається зі СТАБ-бінарниками PG
//! (`initdb`/`psql`/`pg_ctl` пишуть кожен виклик у лог). Отже «жодного
//! initdb/CREATE DATABASE» — це факт реального виконання, а не читання коду.
//!
//! Чому не через `serve_listener`: на цій машині `resolve_database_url()`
//! ЗАВЖДИ знаходить робочий URL у `<repo>/backend/.env` (абсолютний кандидат
//! `CARGO_MANIFEST_DIR/../../../../backend/.env`), тож план стає `ExternalUrl`
//! і гілка standby не виконується. На проді (Windows-каса) db_sources.toml і
//! backend/.env ВІДСУТНІ (зафіксовано: `where /r C:\Users\Admin db_sources.toml`
//! → не знайдено), тому там resolve дає `Err` → план `StandbyReplica`.
//! Ланцюг «режим standby + resolve Err → StandbyReplica» покритий юніт-тестом
//! `standby_without_external_url_never_bootstraps`.
use std::path::Path;

fn write_stub(path: &Path, label: &str, log: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\necho \"{label} $*\" >> \"{}\"\nexit 0\n",
        log.display()
    );
    std::fs::write(path, script).expect("стаб-скрипт");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod +x");
}

fn port_5433_open() -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 5433)),
        std::time::Duration::from_millis(300),
    )
    .is_ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn standby_plan_never_runs_initdb_or_create_database() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path().to_path_buf();

    // ── Ізоляція: XDG_DATA_HOME → temp (data_dir_default + torgashka.log) ──
    std::env::set_var("XDG_DATA_HOME", &root_path);

    // ── Каталог репліки: PG_VERSION є (pg_basebackup уже виконано) ─────────
    let pgdata = root_path.join("Torgashka/pgdata");
    std::fs::create_dir_all(&pgdata).expect("pgdata");
    std::fs::write(pgdata.join("PG_VERSION"), "17\n").expect("PG_VERSION");
    assert!(
        pgdata.join("PG_VERSION").is_file(),
        "каталог репліки готовий"
    );

    // ── Стаб-бінарники PG: фіксують будь-який виклик ──────────────────────
    let bin = root_path.join("pgbin");
    std::fs::create_dir_all(&bin).expect("pgbin");
    let log = root_path.join("pg_calls.log");
    write_stub(&bin.join("initdb"), "initdb", &log);
    write_stub(&bin.join("pg_ctl"), "pg_ctl", &log);
    write_stub(&bin.join("psql"), "psql", &log);
    std::env::set_var("TORGASHKA_PG_DIR", &bin);
    std::env::set_var("TORGASHKA_PG_START_TIMEOUT_SECS", "2");
    std::env::set_var("TORGASHKA_PG_USER", "repuser");
    std::env::set_var("TORGASHKA_PG_DB", "repdb");
    std::env::remove_var("DATABASE_URL");

    // ── Виконання тієї самої дії, що в ініціалізації фасаду для standby ────
    let url = "postgresql://repuser@127.0.0.1:5433/repdb".to_string();
    let guard = torgashka_api::apply_db_startup_plan(torgashka_api::DbStartupPlan::StandbyReplica(
        url.clone(),
    ))
    .await;
    assert!(
        guard.is_none(),
        "standby не володіє реплікою — Drop фасаду не має її зупиняти"
    );

    // ── 1. DATABASE_URL → локальна репліка, БЕЗ пароля ────────────────────
    assert_eq!(
        std::env::var("DATABASE_URL").unwrap_or_default(),
        url,
        "standby-вузол читає локальну репліку (URL без пароля)"
    );

    // ── 2. ГОЛОВНЕ (дефект 6): жодного initdb і жодного CREATE DATABASE ────
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !calls.contains("initdb"),
        "на standby-вузлі initdb ЗАБОРОНЕНИЙ (каталог репліки, read-only): {calls}"
    );
    assert!(
        !calls.contains("psql"),
        "на standby-вузлі CREATE DATABASE через psql ЗАБОРОНЕНИЙ (hot standby = read-only): {calls}"
    );

    // ── 3. Репліку піднімає pg_ctl — з обмеженим таймаутом (якщо порт вільний;
    //      якщо 5433 уже слухає, ensure_local_replica_running — no-op за задумом).
    if port_5433_open() {
        eprintln!("[test] 5433 зайнятий → ensure_local_replica_running = no-op (очікувано)");
        assert!(!calls.contains("pg_ctl"), "{calls}");
    } else {
        assert!(
            calls.contains("pg_ctl"),
            "репліку має піднімати pg_ctl: {calls}"
        );
        assert!(
            calls.contains("-w -t 2"),
            "pg_ctl мусить бути обмежений нашим таймаутом (-w -t 2): {calls}"
        );
    }
}
