//! ДЕФЕКТ 7 (boot-gate фасаду): `serve_listener` біндить порт і СИНХРОННО
//! виконує ініціалізацію БД ДО `axum::serve` — порт LISTENING, TCP-handshake
//! проходить, а HTTP-відповіді немає НІКОЛИ (прод: `curl: (28) Operation
//! timed out ... 0 bytes received`).
//!
//! Доводиться реальним сирим HTTP/1.1 по TCP на ефемерному порту, у дві фази:
//!   ФАЗА 1 (поточний код, boot-gate) — ініціалізація навмисно довга й
//!     блокуюча: `TORGASHKA_PG_DIR` вказує на стаб-скрипти `initdb/pg_ctl/psql`,
//!     кожен з яких спить 10 с (і пише свій виклик у лог). Проба через 300 мс
//!     після старту МУСИТЬ отримати байти за <2 с (байдуже, 200/404/503).
//!   ФАЗА 2 (контроль «до фіксу») — той самий довгий блокуючий крок виконано
//!     ДО старту обслуговування (старий порядок): слухач уже прив'язаний, але
//!     відповіді за ті самі 2 с НЕМАЄ (0 байт). Це і є дефект 7 — якщо
//!     прибрати spawn-boot-gate у `serve_listener`, фаза 1 перетворюється на
//!     фазу 2 і тест падає.
//!
//! План ініціалізації тут — власний bootstrap (URL не резолвиться): `db_sources.toml` у
//! tmp-каталозі має `active`, якого немає серед джерел → `resolve_database_url()`
//! → Err (чесно, без fallback), тож фасад реально йде у крок embedded-PG.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// Стаб-бінарник PG: фіксує виклик у лог і СПИТЬ 10 с — імітація довгої
/// блокуючої ініціалізації (initdb/pg_ctl/psql — це subprocess-и).
fn write_slow_stub(path: &Path, label: &str, log: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\necho \"{label} $*\" >> \"{}\"\nsleep 10\nexit 0\n",
        log.display()
    );
    std::fs::write(path, script).expect("стаб-скрипт");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod +x");
}

/// Сирий HTTP/1.1 GET по TCP: повертає (отримано-байт, час-до-першого-байта,
/// перший рядок відповіді). Таймаут читання = 0 байт (без паніки) — саме так
/// виглядає дефект 7 для клієнта.
fn raw_http_get(addr: &str, path: &str, timeout: Duration) -> (usize, Duration, String) {
    let t0 = Instant::now();
    let mut stream = TcpStream::connect(addr).expect("TCP connect (порт мусить бути відкритий)");
    stream
        .set_read_timeout(Some(timeout))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(timeout))
        .expect("write timeout");
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).expect("запит надіслано");
    let mut buf = vec![0u8; 4096];
    // Таймаут читання → Err → 0 байт (дефект 7: сервер мовчить).
    let n = stream.read(&mut buf).unwrap_or(0);
    let elapsed = t0.elapsed();
    let head = String::from_utf8_lossy(&buf[..n])
        .lines()
        .next()
        .unwrap_or("<жодного байта>")
        .to_string();
    (n, elapsed, head)
}

/// Env-обгортка фасаду: ізоляція в tmp + «неможливий» db_sources.toml
/// (active → неіснуюче джерело) → resolve дає Err → план «bootstrap власної БД».
fn isolate_env(root: &Path) -> std::path::PathBuf {
    std::env::set_var("XDG_DATA_HOME", root);
    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("TEST_DATABASE_URL");
    let cfg = root.join("Torgashka/db_sources.toml");
    std::fs::create_dir_all(cfg.parent().expect("каталог конфіга")).expect("mkdir Torgashka");
    std::fs::write(&cfg, "active = \"ghost\"\n").expect("db_sources.toml");
    std::env::set_var("TORGASHKA_DB_SOURCES", &cfg);

    let bin = root.join("pgbin");
    std::fs::create_dir_all(&bin).expect("pgbin");
    let log = root.join("pg_calls.log");
    write_slow_stub(&bin.join("initdb"), "initdb", &log);
    write_slow_stub(&bin.join("pg_ctl"), "pg_ctl", &log);
    write_slow_stub(&bin.join("psql"), "psql", &log);
    std::env::set_var("TORGASHKA_PG_DIR", &bin);
    std::env::set_var("TORGASHKA_PG_START_TIMEOUT_SECS", "2");
    std::env::set_var("TORGASHKA_PG_USER", "postgres");
    std::env::set_var("TORGASHKA_PG_DB", "torgashka");
    log
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_answers_within_2s_while_boot_init_blocks() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path().to_path_buf();
    let calls_log = isolate_env(&root_path);

    // ── ФАЗА 1: boot-gate — HTTP з першої секунди при 10-секундній ініціалізації ──
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("бінд ефемерного порту");
    let addr = listener.local_addr().expect("local_addr").to_string();
    let handle = tokio::spawn(async move {
        let _ = torgashka_api::serve_listener(listener).await;
    });
    // Проба «як curl на касі»: через 300 мс після старту, таймаут 2 с.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (n, elapsed, head) = {
        let addr = addr.clone();
        tokio::task::spawn_blocking(move || {
            raw_http_get(&addr, "/api/v1/setup/status", Duration::from_secs(2))
        })
        .await
        .expect("проба")
    };
    let calls = std::fs::read_to_string(&calls_log).unwrap_or_default();
    eprintln!(
        "[boot-gate] ФАЗА 1: {n} байт за {elapsed:?} (ліміт 2 с); перший рядок: {head}; \
         PG-стаби на момент проби: {}",
        calls.replace('\n', " | ")
    );
    assert!(
        n > 0,
        "дефект 7 живий: 0 байт за {elapsed:?} при ініціалізації у фоні ({head})"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "HTTP-відповідь мусить прийти <2 с, а була {elapsed:?}"
    );
    assert!(
        !calls.trim().is_empty(),
        "ініціалізація МУСИТЬ бути в блокуючому PG-кроці (інакше тест не доводить нічого)"
    );
    handle.abort();

    // ── ФАЗА 2: контроль — той самий блокуючий крок, але ДО обслуговування ────
    // Слухач уже прив'язаний (як у проді: бінд у lib.rs до ініціалізації), але
    // ніхто не викликає accept → TCP-handshake проходить, HTTP-відповіді немає.
    std::fs::remove_file(&calls_log).ok();
    std::env::remove_var("DATABASE_URL");
    let listener_old = std::net::TcpListener::bind("127.0.0.1:0").expect("бінд (старий порядок)");
    let addr_old = listener_old.local_addr().expect("local_addr").to_string();
    let probe_addr = addr_old.clone();
    let probe = tokio::task::spawn_blocking(move || {
        raw_http_get(&probe_addr, "/api/v1/setup/status", Duration::from_secs(2))
    });
    // Стара поведінка: синхронний блокуючий bootstrap ДО старту обслуговування.
    // (E7: `DbStartupPlan` замінено на `apply_db_startup` — режимів вузла немає.)
    let _ = torgashka_api::apply_db_startup(None).await;
    let (n_old, elapsed_old, head_old) = probe.await.expect("контрольна проба");
    eprintln!(
        "[boot-gate] ФАЗА 2 (контроль, старий порядок): {n_old} байт за {elapsed_old:?}; \
         перший рядок: {head_old}"
    );
    assert_eq!(
        n_old, 0,
        "контроль мав показати дефект 7 (0 байт), а отримав {n_old} байт: {head_old}"
    );

    // ── ФАЗА 3: той самий слухач після ініціалізації — обслуговування стартує,
    //    і та сама проба отримує байти (різниця лише в ПОРЯДКУ) ────────────────
    listener_old
        .set_nonblocking(true)
        .expect("nonblocking для tokio");
    let listener_tokio =
        tokio::net::TcpListener::from_std(listener_old).expect("std → tokio listener");
    let addr_after = listener_tokio.local_addr().expect("local_addr").to_string();
    tokio::spawn(async move {
        let _ = torgashka_api::serve_listener(listener_tokio).await;
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (n_after, elapsed_after, head_after) = {
        let addr = addr_after.clone();
        tokio::task::spawn_blocking(move || {
            raw_http_get(&addr, "/api/v1/setup/status", Duration::from_secs(2))
        })
        .await
        .expect("проба після ініціалізації")
    };
    eprintln!(
        "[boot-gate] ФАЗА 3 (той самий слухач + boot-gate): {n_after} байт за {elapsed_after:?}; \
         перший рядок: {head_after}"
    );
    assert!(
        n_after > 0 && elapsed_after < Duration::from_secs(2),
        "той самий слухач з boot-gate мусить відповідати негайно: {n_after} байт за {elapsed_after:?}"
    );
}
