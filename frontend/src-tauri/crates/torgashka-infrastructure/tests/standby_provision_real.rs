//! Реальний інтеграційний тест провіжинінгу standby (ЕТАП 16; дефект 1).
//!
//! ⚠️ Потребує PostgreSQL-бінарників та живого **primary** із replication-роллю
//! (слот створюється автоматично через `-C`). Тому запускається ЯВНО через
//! `--ignored`: звичайний `cargo test -p torgashka-infrastructure` без PG
//! залишається зеленим.
//!
//! Критерій (дефект 1): після того, як `provision_standby()` ПОВЕРНУЛА
//! управління, локальна репліка МУСИТЬ слухати `127.0.0.1:5433` і бути в
//! recovery. Якщо guard-менеджер `EmbeddedPostgres` дропається всередині
//! `spawn_blocking` одразу після `start()`, `Drop` виконує `pg_ctl stop -m fast`
//! і вбиває щойно підняту репліку — тоді цей тест падає (а крок 7 провіжна
//! опитує вже мертвий сервер).
//!
//! Запуск (приклад; primary слухає 127.0.0.1:5545, дані — окремі tmp-каталоги):
//! ```bash
//! export TORGASHKA_PG_DIR=/usr/lib/postgresql/17/bin
//! export TORGASHKA_TEST_STANDBY_DIR=/tmp/standby_e2e
//! export TORGASHKA_TEST_PRIMARY_HOST=127.0.0.1
//! export TORGASHKA_TEST_PRIMARY_PORT=5545
//! export TORGASHKA_TEST_REPL_ROLE=replicator_e2e
//! export TORGASHKA_TEST_REPL_SLOT=standby_e2e
//! export TORGASHKA_TEST_REPL_PASSWORD=e2epass123
//! cargo test -p torgashka-infrastructure --test standby_provision_real -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use torgashka_infrastructure::standby_provision::{provision_standby, StandbyParams};

/// Порт локальної репліки (константа embedded_pg::EMBEDDED_PG_PORT).
const LOCAL_PORT: u16 = 5433;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_required(key: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| panic!("{key} не задано (див. doc-коментар файлу)"))
}

/// `psql -h 127.0.0.1 -p <port> -U <user> -tAc <sql>` — без пароля (локальний
/// trust, який провіжинінг вставляє у pg_hba репліки).
fn psql(bin_dir: &Path, port: u16, user: &str, sql: &str) -> Result<String, String> {
    let out = Command::new(bin_dir.join("psql"))
        .args([
            "-h",
            "127.0.0.1",
            "-p",
            &port.to_string(),
            "-U",
            user,
            "-d",
            "postgres",
            "-tAc",
            sql,
        ])
        .output()
        .map_err(|e| format!("psql spawn: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Чи щось слухає `127.0.0.1:<port>` (TCP).
fn port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "потребує живого primary PostgreSQL (див. doc-коментар угорі файлу)"]
async fn provision_keeps_local_replica_running() {
    let bin_dir = PathBuf::from(env_or("TORGASHKA_PG_DIR", "/usr/lib/postgresql/17/bin"));
    let data_dir = PathBuf::from(env_required("TORGASHKA_TEST_STANDBY_DIR"));
    let user = env_or("TORGASHKA_PG_USER", "postgres");

    let params = StandbyParams {
        primary_host: env_or("TORGASHKA_TEST_PRIMARY_HOST", "127.0.0.1"),
        primary_port: env_or("TORGASHKA_TEST_PRIMARY_PORT", "5545")
            .parse()
            .expect("TORGASHKA_TEST_PRIMARY_PORT — число"),
        database: env_or("TORGASHKA_TEST_DATABASE", "pos_system_fresh"),
        replication_role: env_or("TORGASHKA_TEST_REPL_ROLE", "replicator_e2e"),
        replication_slot: env_or("TORGASHKA_TEST_REPL_SLOT", "standby_e2e"),
        replication_password: env_or("TORGASHKA_TEST_REPL_PASSWORD", "e2epass123"),
        data_dir: Some(data_dir.clone()),
        bin_dir: Some(bin_dir.clone()),
        secret_anchor: Some(data_dir.join("replication_secret.ctx")),
    };

    provision_standby(params)
        .await
        .expect("provision_standby має завершитися успішно (кроки 1-7)");

    // Дефект 1 (регресія): репліка мусить лишитися запущеною ПІСЛЯ повернення.
    assert!(
        port_open(LOCAL_PORT),
        "після provision_standby порт 127.0.0.1:{LOCAL_PORT} НЕ слухає — \
         guard EmbeddedPostgres зупинив щойно підняту репліку (дефект 1)"
    );
    let mut last = String::new();
    let mut in_recovery = false;
    for _ in 0..20 {
        match psql(&bin_dir, LOCAL_PORT, &user, "select pg_is_in_recovery()") {
            Ok(v) if v == "t" => {
                in_recovery = true;
                break;
            }
            other => last = format!("{other:?}"),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(
        in_recovery,
        "локальна репліка не відповідає pg_is_in_recovery()=true на \
         127.0.0.1:{LOCAL_PORT} після повернення provision_standby (останнє: {last})"
    );
}

/// Реальний тест **дефекту 2**: після «ребуту» (репліка зупинена, але
/// каталог даних уже провіжнено) `ensure_local_replica_running()` МУСИТЬ
/// підняти локальну репліку — інакше фасад :8000 не має БД у standby-режимі.
///
/// Передумова: `TORGASHKA_TEST_STANDBY_DIR` == `data_dir_default()`
/// (`$XDG_DATA_HOME/Torgashka/pgdata`), тобто каталог, який бачить
/// `ensure_local_replica_running()`. Запуск — з тим самим `--ignored`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "потребує провіжненої репліки у data_dir_default() (див. doc)"]
async fn ensure_local_replica_restarts_provisioned_replica() {
    use torgashka_infrastructure::embedded_pg::{ensure_local_replica_running, EMBEDDED_PG_PORT};

    let bin_dir = PathBuf::from(env_or("TORGASHKA_PG_DIR", "/usr/lib/postgresql/17/bin"));
    let data_dir = data_dir_default_for_test();
    assert!(
        data_dir.join("PG_VERSION").exists(),
        "репліку не провіжнено у {} (задайте TORGASHKA_TEST_STANDBY_DIR = data_dir_default)",
        data_dir.display()
    );

    // Імітація ребуту: зупиняємо репліку поза застосунком (pg_ctl stop).
    let _ = Command::new(bin_dir.join("pg_ctl"))
        .args([
            "-D",
            &data_dir.to_string_lossy(),
            "-m",
            "fast",
            "-w",
            "stop",
        ])
        .output();
    assert!(
        !port_open(EMBEDDED_PG_PORT),
        "не вдалося зупинити репліку на {} (імітація ребуту не вдалась)",
        EMBEDDED_PG_PORT
    );

    // Дефект 2: старт застосунку мусить підняти локальну репліку (Ok(true)).
    let started = ensure_local_replica_running().expect("ensure_local_replica_running");
    assert!(started, "репліка має бути піднята (Ok(true)) після ребуту");
    assert!(
        port_open(EMBEDDED_PG_PORT),
        "після ensure_local_replica_running порт {EMBEDDED_PG_PORT} не слухає"
    );
    let user = env_or("TORGASHKA_PG_USER", "postgres");
    let mut in_recovery = false;
    for _ in 0..20 {
        if psql(
            &bin_dir,
            EMBEDDED_PG_PORT,
            &user,
            "select pg_is_in_recovery()",
        )
        .ok()
        .as_deref()
            == Some("t")
        {
            in_recovery = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(
        in_recovery,
        "піднятий сервер не є standby (pg_is_in_recovery != true)"
    );

    // Ідемпотентність: повторний виклик — no-op (Ok(false), сервер не чіпаємо).
    let again = ensure_local_replica_running().expect("повторний виклик");
    assert!(!again, "повторний виклик має бути no-op (Ok(false))");
    assert!(
        port_open(EMBEDDED_PG_PORT),
        "репліка має лишитися запущеною"
    );
}

/// `data_dir_default()` з `embedded_pg` (публічна — той самий шлях, що й у
/// застосунку: `$XDG_DATA_HOME/Torgashka/pgdata` або `$HOME/.local/share/...`).
fn data_dir_default_for_test() -> PathBuf {
    torgashka_infrastructure::embedded_pg::data_dir_default()
}
