// ─────────────────────────────────────────────────────────────────────────────
// torgashka-api — bin/facade.rs: standalone-запуск фасаду (dev/тестування без Tauri)
// ─────────────────────────────────────────────────────────────────────────────
// Запускає axum-фасад на 127.0.0.1:8000 (або TORGASHKA_FACADE_ADDR).
// Зупинка: Ctrl+C → graceful abort таску.
//
// Приклад:
//   cargo run -p torgashka-api --bin facade
//   curl http://127.0.0.1:8000/api/v1/health
// ─────────────────────────────────────────────────────────────────────────────

/// Режим SDK-хелпера ПРРО (ізоляція FFI EUSignCP у субпроцесі):
/// IitSigner::sign/verify запускає current_exe з TORGASHKA_PRRO_SDK_HELPER=1;
/// хелпер виконує SDK-роботу і виходить. Крах багнутого cspb.so (#GP/SIGSEGV)
/// вбиває лише субпроцес — фасад/Torgashka виживає, помилка → HTTP 400.
fn sdk_helper_dispatch() -> Option<i32> {
    if std::env::var_os(torgashka_prro::crypto::iit::SDK_HELPER_ENV).is_some() {
        Some(torgashka_prro::crypto::iit::sdk_helper_main())
    } else {
        None
    }
}

#[tokio::main]
async fn main() {
    if let Some(code) = sdk_helper_dispatch() {
        std::process::exit(code);
    }
    let addr = std::env::var("TORGASHKA_FACADE_ADDR")
        .unwrap_or_else(|_| torgashka_api::DEFAULT_FACADE_ADDR.to_string());
    let handle = torgashka_api::run_facade(&addr);
    tokio::signal::ctrl_c()
        .await
        .expect("помилка очікування Ctrl+C");
    handle.abort();
    eprintln!("[facade] зупинено");
}
