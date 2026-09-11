// ─────────────────────────────────────────────────────────────────────────────
// torgashka-api — bin/facade.rs: standalone-запуск фасаду (dev/тестування без Tauri)
// ─────────────────────────────────────────────────────────────────────────────
// Запускає axum-фасад на 127.0.0.1:8000. Адреса (пріоритет):
//   1) env TORGASHKA_FACADE_ADDR  (напр. "0.0.0.0:8000" — [server]-режим);
//   2) config.toml → [server].addr (mode="desktop"|"server"; секція опційна);
//   3) дефолт DEFAULT_FACADE_ADDR (127.0.0.1:8000).
// Шляхи config.toml (перший існуючий):
//   env TORGASHKA_CONFIG → ./config.toml → frontend/src-tauri/config.toml
//   → <repo>/config.toml.
// Зупинка: Ctrl+C → graceful abort таску.
//
// Приклад:
//   cargo run -p torgashka-api --bin facade
//   curl http://127.0.0.1:8000/api/v1/health
// ─────────────────────────────────────────────────────────────────────────────

use std::path::PathBuf;

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

/// Кандидати config.toml у порядку пріоритету (перший ІСНУЮЧИЙ виграє).
/// Репозиторій-відносні шляхи — від compile-time CARGO_MANIFEST_DIR
/// (crates/torgashka-api):
///   ../../config.toml       → frontend/src-tauri/config.toml
///   ../../../../config.toml → <repo>/config.toml
fn config_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("TORGASHKA_CONFIG") {
        if !p.trim().is_empty() {
            v.push(PathBuf::from(p));
        }
    }
    v.push(PathBuf::from("config.toml")); // CWD
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    v.push(manifest.join("../../config.toml")); // frontend/src-tauri/config.toml
    v.push(manifest.join("../../../../config.toml")); // <repo>/config.toml
    v
}

/// Читає перший існуючий config.toml (None — файлу немає або не читається).
fn read_first_config() -> Option<String> {
    config_candidates()
        .iter()
        .find(|c| c.is_file())
        .and_then(|c| std::fs::read_to_string(c).ok())
}

/// Адреса з секції [server].addr конфіг-файлу (без env-перевизначення).
/// Відсутня секція/ключ/помилка парсингу → None (викликач бере дефолт).
fn server_addr_from_config(config_toml: Option<&str>) -> Option<String> {
    let text = config_toml?;
    if text.trim().is_empty() {
        return None;
    }
    let v: toml::Value = toml::from_str(text).ok()?;
    let addr = v.get("server")?.get("addr")?.as_str()?.trim();
    if addr.is_empty() {
        None
    } else {
        Some(addr.to_string())
    }
}

/// Фінальна адреса фасаду. Пріоритет:
///   env TORGASHKA_FACADE_ADDR > config [server].addr > DEFAULT_FACADE_ADDR.
fn parse_server_addr(config_toml: Option<&str>, env_addr: Option<&str>) -> String {
    if let Some(a) = env_addr {
        let t = a.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Some(a) = server_addr_from_config(config_toml) {
        return a;
    }
    torgashka_api::DEFAULT_FACADE_ADDR.to_string()
}

#[tokio::main]
async fn main() {
    if let Some(code) = sdk_helper_dispatch() {
        std::process::exit(code);
    }
    let env_addr = std::env::var("TORGASHKA_FACADE_ADDR").ok();
    let config = read_first_config();
    let addr = parse_server_addr(config.as_deref(), env_addr.as_deref());
    eprintln!("[facade] слухаю на {addr}");
    let handle = torgashka_api::run_facade(&addr);
    tokio::signal::ctrl_c()
        .await
        .expect("помилка очікування Ctrl+C");
    handle.abort();
    eprintln!("[facade] зупинено");
}

// ─── Unit-тести: чистий парсер адреси ([server]-режим) ──────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG_SERVER: &str = r#"
[server]
mode = "server"
addr = "0.0.0.0:8443"
"#;

    const CONFIG_NO_SERVER: &str = r#"
[database]
app_db_user = "torgashka_app"
"#;

    #[test]
    fn default_when_no_config_no_env() {
        assert_eq!(
            parse_server_addr(None, None),
            torgashka_api::DEFAULT_FACADE_ADDR
        );
        // config без секції [server] → дефолт
        assert_eq!(
            parse_server_addr(Some(CONFIG_NO_SERVER), None),
            torgashka_api::DEFAULT_FACADE_ADDR
        );
        // config з порожньою секцією/порожнім addr → дефолт
        assert_eq!(
            parse_server_addr(Some("[server]\naddr = \"\"\n"), None),
            torgashka_api::DEFAULT_FACADE_ADDR
        );
        // невалідний toml → дефолт (не падаємо)
        assert_eq!(
            parse_server_addr(Some("not toml [[["), None),
            torgashka_api::DEFAULT_FACADE_ADDR
        );
    }

    #[test]
    fn config_server_addr_used_when_no_env() {
        assert_eq!(parse_server_addr(Some(CONFIG_SERVER), None), "0.0.0.0:8443");
    }

    #[test]
    fn env_overrides_config_and_default() {
        assert_eq!(
            parse_server_addr(Some(CONFIG_SERVER), Some("127.0.0.1:8000")),
            "127.0.0.1:8000"
        );
        // порожній env → config
        assert_eq!(
            parse_server_addr(Some(CONFIG_SERVER), Some("   ")),
            "0.0.0.0:8443"
        );
        // env без config → env (колишня поведінка збережена)
        assert_eq!(
            parse_server_addr(None, Some("127.0.0.1:9000")),
            "127.0.0.1:9000"
        );
    }

    #[test]
    fn config_candidates_have_manifest_relative_paths() {
        let v = config_candidates();
        assert!(v.iter().any(|c| c == &PathBuf::from("config.toml")));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert!(v.contains(&manifest.join("../../config.toml")));
        assert!(v.contains(&manifest.join("../../../../config.toml")));
    }
}
