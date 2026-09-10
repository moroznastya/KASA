// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Команди standby-вузла мережі магазинів (B1)
// ─────────────────────────────────────────────────────────────────────────────
//
// Закриває блокер B1: `standby_provision::provision_standby` ніхто не
// викликав — касса-standby після join ніколи не робила pg_basebackup.
//
//   * start_standby_provision — прочитати креденшл join з SQLite settings
//     (node_*), виконати провіжинг, позначити [node] mode=standby у
//     db_sources.toml (після рестарту фасад монтує /api/v1/local/*),
//     стартувати фоновий heartbeat-цикл;
//   * restart_app — перезапуск застосунку (спawn exe + exit через 300 мс),
//     щоб фасад піднявся з новим режимом вузла.
// ─────────────────────────────────────────────────────────────────────────────

use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use torgashka_infrastructure::standby_heartbeat;
use torgashka_infrastructure::standby_provision::{self, StandbyParams};

/// Побудувати параметри провіжингу з SQLite settings (sync I/O — викликати
/// у spawn_blocking). `node_node_id` — маркер «join виконано».
///
/// Мапа «settings → params» живе в інфраструктурі
/// ([`standby_provision::params_from_settings`]) — чиста функція, покрита
/// тестами (обов'язкові ключі, зокрема `node_replication_database`: без імені
/// БД провіжн не починається, бо його наслідок — `[node] primary_db_url`).
fn build_params_from_settings() -> Result<StandbyParams, String> {
    let st = standby_heartbeat::read_standby_settings()?
        .ok_or_else(|| "join не виконано: спершу приєднайте вузол (NodeJoinPage)".to_string())?;
    standby_provision::params_from_settings(&st)
}

/// Запустити провіжинінг standby-вузла (pg_basebackup з primary) — B1a.
///
/// 1. Читає `node_*` креденшл з SQLite settings (їх зберіг NodeJoinPage
///    після join); без `node_node_id` → Err.
/// 2. Викликає `standby_provision::provision_standby` (зупинка локального
///    PG → pg_basebackup → старт hot-standby на 5433).
/// 3. Після успіху: [node] mode=standby у db_sources.toml (NodeConfig) +
///    старт фоногового heartbeat-циклу.
///
/// Команда async; sync I/O (SQLite, файли) — у spawn_blocking (патерн
/// get_devices_status). Важкі pg-кроки провіжингу provision_standby сама
/// ізолює у spawn_blocking — UI лишається чуйним.
#[tauri::command]
pub async fn start_standby_provision() -> Result<(), String> {
    let params: StandbyParams = tauri::async_runtime::spawn_blocking(build_params_from_settings)
        .await
        .map_err(|e| format!("spawn_blocking(settings): {e}"))??;
    // Реальне ім'я БД (з node_replication_database) потрібне й ПІСЛЯ провіжну:
    // параметри споживаються provision_standby, тож знімаємо їх наперед.
    let primary_host = params.primary_host.clone();
    let primary_port = params.primary_port;
    let database = params.database.clone();
    torgashka_infrastructure::standby_provision::provision_standby(params)
        .await
        .map_err(|e| format!("standby provision: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        // [node] mode=standby + primary_db_url: після рестарту фасад змонтує
        // /api/v1/local/* проти локальної репліки 5433 (init_local_standby,
        // torgashka-api). URL обов'язковий — local_db_url() виводить з нього
        // 127.0.0.1:<local_port>/<РЕАЛЬНА БД>; без нього фасад лишався без
        // пулів і вгадував «torgashka» (дефект 2026-09).
        let user = std::env::var("TORGASHKA_PG_USER").unwrap_or_else(|_| "postgres".to_string());
        let cfg = NodeConfig {
            mode: NodeMode::Standby,
            primary_db_url: Some(
                torgashka_infrastructure::node_config::primary_db_url_from_parts(
                    &user,
                    &primary_host,
                    primary_port,
                    &database,
                ),
            ),
            ..NodeConfig::default()
        };
        cfg.save_to_disk()?;
        standby_heartbeat::start_standby_heartbeat()
    })
    .await
    .map_err(|e| format!("spawn_blocking(post-provision): {e}"))??;
    Ok(())
}

/// Перезапустити застосунок — B1c.
///
/// Spawn поточного exe (std::env::current_exe) + `app.exit(0)` через 300 мс
/// (дає часу новому процесу зайняти порт). Призначення: після першого
/// join+provision фасад має піднятись з [node] mode=standby — тільки після
/// рестарту init_local_standby змонтує /api/v1/local/*.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Err(e) = std::process::Command::new(&exe).spawn() {
            eprintln!("[torgashka] restart_app: не вдалося запустити {exe:?}: {e}");
        }
        app.exit(0);
    });
    Ok(())
}

// Тести відсутні: команди — тонка обгортка над інфраструктурою; логіка
// (читання settings, формування heartbeat) вкрита тестами standby_heartbeat.
