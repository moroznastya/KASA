// ─────────────────────────────────────────────────────────────────────────────
// kasa-api — вбудований axum-фасад Kasa POS (Strangler Fig, етап 1)
// ─────────────────────────────────────────────────────────────────────────────
// Вбудований HTTP-шлюз на 127.0.0.1:8000 (той самий порт, що мав Python).
// Фронтенд (axios → http://localhost:8000/api/v1) не змінюється взагалі:
//   - /api/v1/health → нативний Rust-хендлер
//   - довідники (products, categories, suppliers) → Rust-гілка ПІД
//     feature-flag KASA_RUST_READDIRS=1, інакше → reverse proxy на :8001
//   - решта /api/v1/*  → reverse proxy на Python sidecar :8001 (reqwest)
//   - JWT-валідація (HS256) на всі роути, крім /health.
//
// Схема:
//   frontend (axios) ──► kasa-api :8000 ──► Python sidecar :8001 (FastAPI)
//                              │
//                              ├──► Rust readdirs (БД PostgreSQL, read-only)
//                              └──► (майбутнє) нативні Rust-хендлери
// ─────────────────────────────────────────────────────────────────────────────

pub mod auth;
pub mod auth_routes;
pub mod crud;
pub mod debtors;
pub mod documents;
pub mod invoices;
pub mod ledger;
pub mod pos;
pub mod proxy;
pub mod prro;
pub mod readdirs;
pub mod return_invoices;
pub mod router_v1;

use std::sync::Arc;

use kasa_domain::{
    AuthService, DebtorService, DocumentsService, InvoicesV1Service, InvoicesV2Service,
    LedgerService, PosService, ReadDirectories, ReturnInvoicesService, WriteDirectories,
};
use sqlx::PgPool;

/// Порт Python sidecar (FastAPI). Константа — єдине джерело істини.
pub const PYTHON_SIDECAR_PORT: u16 = 8001;

/// Адреса фасаду за замовчуванням (той самий порт, що мав Python).
pub const DEFAULT_FACADE_ADDR: &str = "127.0.0.1:8000";

/// Env-флаг увімкнення Rust-гілки довідників (етап 1).
pub const RUST_READDIRS_ENV: &str = "KASA_RUST_READDIRS";

/// Env-флаг увімкнення Rust-гілки auth/users/settings/RBAC (етап 6).
pub const RUST_AUTH_ENV: &str = "KASA_RUST_AUTH";

/// Env-флаг Rust-гілки боржників (етап 8, група 1).
pub const RUST_DEBTORS_ENV: &str = "KASA_RUST_DEBTORS";

/// Env-флаг Rust-гілки документів (етап 8, група 2).
pub const RUST_DOCUMENTS_ENV: &str = "KASA_RUST_DOCUMENTS";
pub const RUST_INVOICES_ENV: &str = "KASA_RUST_INVOICES";
/// Env-флаг Rust-гілки повернень (етап 8, група 4).
pub const RUST_RETURN_INVOICES_ENV: &str = "KASA_RUST_RETURN_INVOICES";

/// Env-флаг Rust-гілки ПРРО (етап 7.3): "1" — Rust виконує,
/// "shadow" — Rust готує чек і логує parity, Python виконує (проксі).
pub const RUST_PRRO_ENV: &str = "KASA_RUST_PRRO";

/// Спільний стан фасаду: JWT-секрет + HTTP-клієнт + (опц.) Rust-репозиторій.
#[derive(Clone)]
pub struct AppState {
    /// Секрет підпису/перевірки JWT (HS256), спільний із Python-бекендом.
    pub jwt_secret: Arc<String>,
    /// HTTP-клієнт для reverse proxy на Python sidecar.
    pub http_client: reqwest::Client,
    /// Rust-репозиторій довідників (Some лише коли KASA_RUST_READDIRS=1).
    pub readdirs: Option<Arc<dyn ReadDirectories + Send + Sync>>,
    /// Rust-репозиторій запису (CRUD, етап 2) — той самий пул.
    pub write: Option<Arc<dyn WriteDirectories + Send + Sync>>,
    /// Пул PostgreSQL (для require_admin) — Some лише з флагом.
    pub write_pool: Option<PgPool>,
    /// Rust-репозиторій POS (етап 3) — той самий пул.
    pub pos: Option<Arc<dyn PosService + Send + Sync>>,
    /// Rust-репозиторій ledger (етап 4) — той самий пул.
    pub ledger: Option<Arc<dyn LedgerService + Send + Sync>>,
    /// Rust-репозиторій auth (етап 6) — Some лише коли KASA_RUST_AUTH=1.
    pub auth: Option<Arc<dyn AuthService + Send + Sync>>,
    /// Rust-фасад фіскального ПРРО (етап 7.3) — KASA_RUST_PRRO=1|shadow.
    pub prro: Option<Arc<crate::prro::PrroFacade>>,
    /// Rust-репозиторій боржників (етап 8, група 1) — KASA_RUST_DEBTORS=1.
    pub debtors: Option<Arc<dyn DebtorService + Send + Sync>>,
    /// Rust-репозиторій документів (етап 8, група 2) — KASA_RUST_DOCUMENTS=1.
    pub documents: Option<Arc<dyn DocumentsService + Send + Sync>>,
    /// Пул документів (require_admin документів незалежно від KASA_RUST_AUTH).
    pub documents_pool: Option<PgPool>,
    /// Rust-репозиторій інвойсів v1 (етап 8, група 3) — KASA_RUST_INVOICES=1.
    pub invoices_v1: Option<Arc<dyn InvoicesV1Service + Send + Sync>>,
    /// Rust-репозиторій інвойсів v2 (етап 8, група 3) — KASA_RUST_INVOICES=1.
    pub invoices_v2: Option<Arc<dyn InvoicesV2Service + Send + Sync>>,
    /// Пул інвойсів (require_admin інвойсів незалежно від KASA_RUST_AUTH).
    pub invoices_pool: Option<PgPool>,
    /// Rust-репозиторій повернень (етап 8, група 4) — KASA_RUST_RETURN_INVOICES=1.
    pub return_invoices: Option<Arc<dyn ReturnInvoicesService + Send + Sync>>,
    /// Пул повернень (require_admin повернень незалежно від KASA_RUST_AUTH).
    pub return_invoices_pool: Option<PgPool>,
}

/// Чистий payload для /api/v1/health (використовується роутером і diff CLI).
pub fn health_payload() -> serde_json::Value {
    serde_json::json!({"status": "ok"})
}

/// Чиста функція echo для differential CLI (повертає args без змін).
pub fn echo_payload(args: &serde_json::Value) -> serde_json::Value {
    args.clone()
}

/// Читання bool-флага з env (1/true/yes → true).
fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// Ініціалізує Rust-гілку довідників під feature-flag (етапи 1–2).
///
/// Якщо `KASA_RUST_READDIRS=1` і БД доступна — повертає (пул, read-репо,
/// write-репо). Інакше `None` (фасад лишається проксі, режим відкату на
/// Python :8001 зберігається).
async fn init_readdirs() -> Option<(
    PgPool,
    Arc<dyn ReadDirectories + Send + Sync>,
    Arc<dyn WriteDirectories + Send + Sync>,
    Arc<dyn PosService + Send + Sync>,
    Arc<dyn LedgerService + Send + Sync>,
    Arc<dyn AuthService + Send + Sync>,
)> {
    if !env_flag(RUST_READDIRS_ENV) {
        return None;
    }
    match kasa_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[kasa-api] {RUST_READDIRS_ENV}=1 — Rust-гілка довідників увімкнена (PostgreSQL, read-write)"
            );
            let read = Arc::new(
                kasa_infrastructure::repositories::directories::SqlxDirectories::new(pool.clone()),
            ) as Arc<dyn ReadDirectories + Send + Sync>;
            let write = Arc::new(
                kasa_infrastructure::repositories::write::SqlxWriteDirectories::new(pool.clone()),
            ) as Arc<dyn WriteDirectories + Send + Sync>;
            let pos = Arc::new(kasa_infrastructure::repositories::pos::SqlxPos::new(
                pool.clone(),
            )) as Arc<dyn PosService + Send + Sync>;
            let ledger = Arc::new(kasa_infrastructure::repositories::ledger::SqlxLedger::new(
                pool.clone(),
            )) as Arc<dyn LedgerService + Send + Sync>;
            let auth = Arc::new(kasa_infrastructure::repositories::auth::SqlxAuth::new(
                pool.clone(),
            )) as Arc<dyn AuthService + Send + Sync>;
            Some((pool, read, write, pos, ledger, auth))
        }
        Err(e) => {
            eprintln!(
                "[kasa-api] попередження: {RUST_READDIRS_ENV}=1, але БД недоступна ({e}); \
                 довідники працюють через проксі на Python :8001 (режим відкату)"
            );
            None
        }
    }
}

/// Ініціалізує Rust-гілку ПРРО під KASA_RUST_PRRO (1|shadow).
async fn init_prro() -> Option<Arc<crate::prro::PrroFacade>> {
    let mode = std::env::var(RUST_PRRO_ENV).unwrap_or_default();
    if !matches!(mode.trim().to_lowercase().as_str(), "1" | "true" | "shadow") {
        return None;
    }
    match kasa_infrastructure::db::connect_readonly_pool(5).await {
        Ok(pool) => match kasa_infrastructure::prro::SqlxPrroRepository::connect(pool).await {
            Ok(repo) => {
                let shadow = mode.trim().to_lowercase() == "shadow";
                eprintln!(
                        "[kasa-api] {RUST_PRRO_ENV}={mode} — Rust-гілка ПРРО увімкнена (shadow={shadow}, PostgreSQL)"
                    );
                Some(Arc::new(crate::prro::PrroFacade::new(repo, shadow)))
            }
            Err(e) => {
                eprintln!(
                        "[kasa-api] попередження: {RUST_PRRO_ENV}={mode}, але схему ПРРО не створено ({e}); проксі на Python :8001"
                    );
                None
            }
        },
        Err(e) => {
            eprintln!(
                "[kasa-api] попередження: {RUST_PRRO_ENV}={mode}, але БД недоступна ({e}); проксі на Python :8001"
            );
            None
        }
    }
}

/// Ініціалізує Rust-гілку документів під KASA_RUST_DOCUMENTS=1.
async fn init_documents() -> (
    Option<Arc<dyn DocumentsService + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_DOCUMENTS_ENV) {
        return (None, None);
    }
    match kasa_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[kasa-api] {RUST_DOCUMENTS_ENV}=1 — Rust-гілка документів увімкнена (PostgreSQL)"
            );
            let svc: Arc<dyn DocumentsService + Send + Sync> = Arc::new(
                kasa_infrastructure::repositories::documents::SqlxDocuments::new(pool.clone()),
            );
            (Some(svc), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[kasa-api] попередження: {RUST_DOCUMENTS_ENV}=1, але БД недоступна ({e}); документи через проксі на Python :8001"
            );
            (None, None)
        }
    }
}

/// Ініціалізує Rust-гілку інвойсів під KASA_RUST_INVOICES=1.
/// Ініціалізує Rust-гілку повернень під KASA_RUST_RETURN_INVOICES=1.
async fn init_return_invoices() -> (
    Option<Arc<dyn ReturnInvoicesService + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_RETURN_INVOICES_ENV) {
        return (None, None);
    }
    match kasa_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[kasa-api] {RUST_RETURN_INVOICES_ENV}=1 — Rust-гілка повернень увімкнена (PostgreSQL)"
            );
            let repo = kasa_infrastructure::repositories::return_invoices::SqlxReturnInvoices::new(
                pool.clone(),
            );
            let svc: Arc<dyn ReturnInvoicesService + Send + Sync> = Arc::new(repo);
            (Some(svc), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[kasa-api] попередження: {RUST_RETURN_INVOICES_ENV}=1, але БД недоступна ({e}); повернення через проксі на Python :8001"
            );
            (None, None)
        }
    }
}

async fn init_invoices() -> (
    Option<Arc<dyn InvoicesV1Service + Send + Sync>>,
    Option<Arc<dyn InvoicesV2Service + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_INVOICES_ENV) {
        return (None, None, None);
    }
    match kasa_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[kasa-api] {RUST_INVOICES_ENV}=1 — Rust-гілка інвойсів увімкнена (PostgreSQL)"
            );
            let repo = kasa_infrastructure::repositories::invoices::SqlxInvoices::new(pool.clone());
            let v1: Arc<dyn InvoicesV1Service + Send + Sync> = Arc::new(repo);
            let repo2 =
                kasa_infrastructure::repositories::invoices::SqlxInvoices::new(pool.clone());
            let v2: Arc<dyn InvoicesV2Service + Send + Sync> = Arc::new(repo2);
            (Some(v1), Some(v2), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[kasa-api] попередження: {RUST_INVOICES_ENV}=1, але БД недоступна ({e}); інвойси через проксі на Python :8001"
            );
            (None, None, None)
        }
    }
}

/// Ініціалізує Rust-гілку боржників під KASA_RUST_DEBTORS=1.
async fn init_debtors() -> Option<Arc<dyn DebtorService + Send + Sync>> {
    if !env_flag(RUST_DEBTORS_ENV) {
        return None;
    }
    match kasa_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[kasa-api] {RUST_DEBTORS_ENV}=1 — Rust-гілка боржників увімкнена (PostgreSQL)"
            );
            Some(
                Arc::new(kasa_infrastructure::repositories::debtors::SqlxDebtors::new(pool))
                    as Arc<dyn DebtorService + Send + Sync>,
            )
        }
        Err(e) => {
            eprintln!(
                "[kasa-api] попередження: {RUST_DEBTORS_ENV}=1, але БД недоступна ({e}); боржники через проксі на Python :8001"
            );
            None
        }
    }
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
    let (readdirs, write, write_pool, pos, ledger, auth) = match init_readdirs().await {
        Some((pool, read, write, pos, ledger, auth)) => (
            Some(read),
            Some(write),
            Some(pool),
            Some(pos),
            Some(ledger),
            Some(auth),
        ),
        None => (None, None, None, None, None, None),
    };
    // Окремий флаг auth: KASA_RUST_AUTH=1 вмикає Rust-гілку auth навіть якщо
    // readdirs вимкнено (проксі-режим для решти) — але пул створюється спільно.
    let auth = if env_flag(RUST_AUTH_ENV) && auth.is_none() {
        match kasa_infrastructure::db::connect_readonly_pool(10).await {
            Ok(pool) => {
                eprintln!("[kasa-api] {RUST_AUTH_ENV}=1 — Rust-гілка auth увімкнена (PostgreSQL)");
                Some(
                    Arc::new(kasa_infrastructure::repositories::auth::SqlxAuth::new(pool))
                        as Arc<dyn AuthService + Send + Sync>,
                )
            }
            Err(e) => {
                eprintln!(
                    "[kasa-api] попередження: {RUST_AUTH_ENV}=1, але БД недоступна ({e}); auth через проксі на Python :8001"
                );
                None
            }
        }
    } else {
        auth
    };
    let prro = init_prro().await;
    let debtors = init_debtors().await;
    let (documents, documents_pool) = init_documents().await;
    let (invoices_v1, invoices_v2, invoices_pool) = init_invoices().await;
    let (return_invoices, return_invoices_pool) = init_return_invoices().await;
    let state = AppState {
        jwt_secret: Arc::new(auth::resolve_jwt_secret()?),
        http_client: reqwest::Client::builder()
            .timeout(proxy::PROXY_TIMEOUT)
            .build()?,
        readdirs,
        write,
        write_pool,
        pos,
        ledger,
        auth,
        prro,
        debtors,
        documents,
        documents_pool,
        invoices_v1,
        invoices_v2,
        invoices_pool,
        return_invoices,
        return_invoices_pool,
    };
    let app = router_v1::build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("[kasa-api] фасад слухає http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
