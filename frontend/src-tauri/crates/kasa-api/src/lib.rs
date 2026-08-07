// ─────────────────────────────────────────────────────────────────────────────
// kasa-api — вбудований axum-фасад Kasa POS (Strangler Fig, етап 0)
// ─────────────────────────────────────────────────────────────────────────────
// Вбудований HTTP-шлюз на 127.0.0.1:8000 (той самий порт, що мав Python).
// Фронтенд (axios → http://localhost:8000/api/v1) не змінюється взагалі:
//   - /api/v1/health → нативний Rust-хендлер
//   - решта /api/v1/*  → reverse proxy на Python sidecar :8001 (reqwest)
//   - JWT-валідація (HS256) на всі роути, крім /health.
//
// Схема:
//   frontend (axios) ──► kasa-api :8000 ──► Python sidecar :8001 (FastAPI)
//                              │
//                              └──► (майбутнє) нативні Rust-хендлери
// ─────────────────────────────────────────────────────────────────────────────

pub mod auth;
pub mod proxy;
pub mod router_v1;

use std::sync::Arc;

/// Порт Python sidecar (FastAPI). Константа — єдине джерело істини.
pub const PYTHON_SIDECAR_PORT: u16 = 8001;

/// Адреса фасаду за замовчуванням (той самий порт, що мав Python).
pub const DEFAULT_FACADE_ADDR: &str = "127.0.0.1:8000";

/// Спільний стан фасаду: JWT-секрет + HTTP-клієнт для проксі.
#[derive(Clone)]
pub struct AppState {
    /// Секрет підпису/перевірки JWT (HS256), спільний із Python-бекендом.
    pub jwt_secret: Arc<String>,
    /// HTTP-клієнт для reverse proxy на Python sidecar.
    pub http_client: reqwest::Client,
}

/// Чистий payload для /api/v1/health (використовується роутером і diff CLI).
pub fn health_payload() -> serde_json::Value {
    serde_json::json!({"status": "ok"})
}

/// Чиста функція echo для differential CLI (повертає args без змін).
pub fn echo_payload(args: &serde_json::Value) -> serde_json::Value {
    args.clone()
}

/// Запускає axum-фасад на вказаній адресі як окремий tokio-таск.
///
/// Повертає `JoinHandle<()>` — через нього можна зупинити фасад (abort).
/// Помилка бінду/старту логується в stderr, таск завершується без паніки.
pub fn run_facade(addr: &str) -> tokio::task::JoinHandle<()> {
    let addr = addr.to_string();
    tokio::spawn(async move {
        if let Err(e) = serve(&addr).await {
            eprintln!("[kasa-api] фасад на {addr} завершився з помилкою: {e}");
        }
    })
}

/// Async-реалізація фасаду (біндинг + serve).
///
/// Публічна — щоб Tauri-шар міг спавнити фасад через власний runtime
/// (`tauri::async_runtime::spawn`), а не через глобальний tokio::spawn.
pub async fn serve(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState {
        jwt_secret: Arc::new(auth::resolve_jwt_secret()?),
        http_client: reqwest::Client::builder()
            .timeout(proxy::PROXY_TIMEOUT)
            .build()?,
    };
    let app = router_v1::build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("[kasa-api] фасад слухає http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
