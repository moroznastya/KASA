// ─────────────────────────────────────────────────────────────────────────────
// kasa-api — bin/facade.rs: standalone-запуск фасаду (dev/тестування без Tauri)
// ─────────────────────────────────────────────────────────────────────────────
// Запускає axum-фасад на 127.0.0.1:8000 (або KASA_FACADE_ADDR).
// Зупинка: Ctrl+C → graceful abort таску.
//
// Приклад:
//   cargo run -p kasa-api --bin facade
//   curl http://127.0.0.1:8000/api/v1/health
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let addr = std::env::var("KASA_FACADE_ADDR")
        .unwrap_or_else(|_| kasa_api::DEFAULT_FACADE_ADDR.to_string());
    let handle = kasa_api::run_facade(&addr);
    tokio::signal::ctrl_c()
        .await
        .expect("помилка очікування Ctrl+C");
    handle.abort();
    eprintln!("[facade] зупинено");
}
