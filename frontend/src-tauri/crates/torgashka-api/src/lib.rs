// ─────────────────────────────────────────────────────────────────────────────
// torgashka-api — вбудований axum-фасад Torgashka (Strangler Fig, етап 1)
// ─────────────────────────────────────────────────────────────────────────────
// Вбудований HTTP-шлюз на 127.0.0.1:8000 — Rust-ядро (дезактивація Python).
// Фронтенд (axios → http://localhost:8000/api/v1) не змінюється взагалі:
//   - /api/v1/health → нативний Rust-хендлер (200)
//   - активні роути (0 CRIT, 0 ALIAS) → нативні Rust-хендлери ПІД
//     feature-flag TORGASHKA_RUST_*=1 (дефолт у Tauri: 1)
//   - LEGACY-роути (фронтенд не кличе) → fallback → 410 Gone
//   - JWT-валідація (HS256) на всі роути, крім /health.
//
// Схема:
//   frontend (axios) ──► torgashka-api :8000 (Rust-ядро, PostgreSQL)
//                              │
//                              └──► LEGACY-шляхи → 410 Gone
// ─────────────────────────────────────────────────────────────────────────────

pub mod admin;
pub mod auth;
pub mod auth_routes;
pub mod categories_v2;
pub mod crud;
pub mod debtors;
pub mod documents;
pub mod invoices;
pub mod ledger;
pub mod network;
pub mod ocr;
pub mod pos;
pub mod print_templates;
pub mod products_v2;
pub mod proxy;
pub mod prro;
pub mod purchase_orders;
pub mod readdirs;
pub mod return_invoices;
pub mod router_v1;
pub mod setup;
pub mod store_context;
pub mod sync;
pub mod sync_receivers;
pub mod stores;
pub mod suppliers;

use std::sync::Arc;

use sqlx::PgPool;
use torgashka_domain::{
    AuthService, DebtorService, DocumentsService, InvoicesV1Service, InvoicesV2Service,
    LedgerService, PosService, PrintTemplatesService, ProductsV2Service, PurchaseOrdersService,
    ReadDirectories, ReturnInvoicesService, SetupService, StoreService, WriteDirectories,
};
use torgashka_infrastructure::store_ctx::StorePool;

/// Адреса фасаду за замовчуванням (той самий порт, що мав Python).
pub const DEFAULT_FACADE_ADDR: &str = "127.0.0.1:8000";

/// Env-флаг увімкнення Rust-гілки довідників (етап 1).
pub const RUST_READDIRS_ENV: &str = "TORGASHKA_RUST_READDIRS";

/// Env-флаг увімкнення Rust-гілки auth/users/settings/RBAC (етап 6).
pub const RUST_AUTH_ENV: &str = "TORGASHKA_RUST_AUTH";

/// Env-флаг Rust-гілки боржників (етап 8, група 1).
pub const RUST_DEBTORS_ENV: &str = "TORGASHKA_RUST_DEBTORS";

/// Env-флаг Rust-гілки документів (етап 8, група 2).
pub const RUST_DOCUMENTS_ENV: &str = "TORGASHKA_RUST_DOCUMENTS";
pub const RUST_INVOICES_ENV: &str = "TORGASHKA_RUST_INVOICES";
/// Env-флаг Rust-гілки повернень (етап 8, група 4).
pub const RUST_RETURN_INVOICES_ENV: &str = "TORGASHKA_RUST_RETURN_INVOICES";
pub const RUST_PURCHASE_ORDERS_ENV: &str = "TORGASHKA_RUST_PURCHASE_ORDERS";

/// Env-флаг Rust-гілки друку (етап 8, група 6).
pub const RUST_PRINT_ENV: &str = "TORGASHKA_RUST_PRINT";

/// Env-флаг Rust-гілки товарів v2 (етап 8, група 7): "1" — Rust виконує.
pub const RUST_PRODUCTS_V2_ENV: &str = "TORGASHKA_RUST_PRODUCTS_V2";

/// Env-флаг Rust-гілки ПРРО (етап 7.3): "1" — Rust виконує,
/// "shadow" — Rust готує чек і логує parity, Python виконує (проксі).
pub const RUST_PRRO_ENV: &str = "TORGASHKA_RUST_PRRO";

/// Env-флаг Rust-гілки ПРРО v2 (група 8/9): settings + test-connection +
/// fiscalize під ОРИГІНАЛЬНИМИ URL Python — "1" — Rust виконує.
pub const RUST_PRRO_V2_ENV: &str = "TORGASHKA_RUST_PRRO_V2";
pub const RUST_OCR_ENV: &str = "TORGASHKA_RUST_OCR";

/// Дефолтні значення feature-флагів після повної дезактивації Python (етап 8).
/// serve() встановлює їх, якщо env не задано явно — Rust-ядро за замовчуванням
/// для будь-якого викликача (Tauri, bin/facade, тести). Явний env має пріоритет.
pub const DEFAULT_RUST_FLAGS: [(&str, &str); 12] = [
    (RUST_READDIRS_ENV, "1"),
    (RUST_AUTH_ENV, "1"),
    (RUST_DEBTORS_ENV, "1"),
    (RUST_DOCUMENTS_ENV, "1"),
    (RUST_INVOICES_ENV, "1"),
    (RUST_RETURN_INVOICES_ENV, "1"),
    (RUST_PURCHASE_ORDERS_ENV, "1"),
    (RUST_PRINT_ENV, "1"),
    (RUST_PRODUCTS_V2_ENV, "1"),
    (RUST_PRRO_ENV, "1"),
    (RUST_PRRO_V2_ENV, "1"),
    (RUST_OCR_ENV, "1"),
];

/// Спільний стан фасаду: JWT-секрет + HTTP-клієнт + (опц.) Rust-репозиторій.
#[derive(Clone)]
pub struct AppState {
    /// Секрет підпису/перевірки JWT (HS256), спільний із Python-бекендом.
    pub jwt_secret: Arc<String>,
    /// Rust-репозиторій довідників (Some лише коли TORGASHKA_RUST_READDIRS=1).
    pub readdirs: Option<Arc<dyn ReadDirectories + Send + Sync>>,
    /// Rust-репозиторій запису (CRUD, етап 2) — той самий пул.
    pub write: Option<Arc<dyn WriteDirectories + Send + Sync>>,
    /// Пул PostgreSQL (для require_admin) — Some лише з флагом.
    pub write_pool: Option<PgPool>,
    /// Rust-репозиторій POS (етап 3) — той самий пул.
    pub pos: Option<Arc<dyn PosService + Send + Sync>>,
    /// Rust-репозиторій ledger (етап 4) — той самий пул.
    pub ledger: Option<Arc<dyn LedgerService + Send + Sync>>,
    /// Rust-репозиторій auth (етап 6) — Some лише коли TORGASHKA_RUST_AUTH=1.
    pub auth: Option<Arc<dyn AuthService + Send + Sync>>,
    /// Rust-фасад фіскального ПРРО (етап 7.3) — TORGASHKA_RUST_PRRO=1|shadow.
    pub prro: Option<Arc<crate::prro::PrroFacade>>,
    /// Rust-репозиторій боржників (етап 8, група 1) — TORGASHKA_RUST_DEBTORS=1.
    pub debtors: Option<Arc<dyn DebtorService + Send + Sync>>,
    /// Rust-репозиторій документів (етап 8, група 2) — TORGASHKA_RUST_DOCUMENTS=1.
    pub documents: Option<Arc<dyn DocumentsService + Send + Sync>>,
    /// Пул документів (require_admin документів незалежно від TORGASHKA_RUST_AUTH).
    pub documents_pool: Option<PgPool>,
    /// Rust-репозиторій інвойсів v1 (етап 8, група 3) — TORGASHKA_RUST_INVOICES=1.
    pub invoices_v1: Option<Arc<dyn InvoicesV1Service + Send + Sync>>,
    /// Rust-репозиторій інвойсів v2 (етап 8, група 3) — TORGASHKA_RUST_INVOICES=1.
    pub invoices_v2: Option<Arc<dyn InvoicesV2Service + Send + Sync>>,
    /// Пул інвойсів (require_admin інвойсів незалежно від TORGASHKA_RUST_AUTH).
    pub invoices_pool: Option<PgPool>,
    /// Rust-репозиторій повернень (етап 8, група 4) — TORGASHKA_RUST_RETURN_INVOICES=1.
    pub return_invoices: Option<Arc<dyn ReturnInvoicesService + Send + Sync>>,
    /// Пул повернень (require_admin повернень незалежно від TORGASHKA_RUST_AUTH).
    pub return_invoices_pool: Option<PgPool>,
    /// Rust-репозиторій замовлень постачальнику (етап 8, група 5) — TORGASHKA_RUST_PURCHASE_ORDERS=1.
    pub purchase_orders: Option<Arc<dyn PurchaseOrdersService + Send + Sync>>,
    /// Пул замовлень (require_admin замовлень незалежно від TORGASHKA_RUST_AUTH).
    pub purchase_orders_pool: Option<PgPool>,
    /// Rust-репозиторій друку (етап 8, група 6) — TORGASHKA_RUST_PRINT=1.
    pub print_templates: Option<Arc<dyn PrintTemplatesService + Send + Sync>>,
    /// Пул друку (require_admin друку незалежно від TORGASHKA_RUST_AUTH).
    pub print_pool: Option<PgPool>,
    /// Rust-репозиторій товарів v2 (етап 8, група 7) — TORGASHKA_RUST_PRODUCTS_V2=1.
    pub products_v2: Option<Arc<dyn ProductsV2Service + Send + Sync>>,
    /// Пул товарів v2 (require_admin товарів незалежно від TORGASHKA_RUST_AUTH).
    pub products_v2_pool: Option<PgPool>,
    /// Rust-сервіс OCR (етап 8, група 9) — TORGASHKA_RUST_OCR=1.
    pub ocr: Option<std::sync::Arc<torgashka_ocr::OcrService>>,
    /// Пул OCR (invoice-ocr зіставлення з БД незалежно від інших флагів).
    pub ocr_pool: Option<PgPool>,
    /// Директорія завантажених файлів (uploads/) — serve та збереження.
    /// Env TORGASHKA_UPLOADS_DIR (абсолютний або відносний шлях), default "uploads".
    pub uploads_dir: std::path::PathBuf,
    /// StorePool для StoreContext middleware (перевірка user_stores + RLS).
    pub store_pool: Option<StorePool>,
    /// Rust-сервіс торговельних точок (Етап 3) — /api/v1/stores, availability.
    pub stores: Option<std::sync::Arc<dyn StoreService + Send + Sync>>,
    /// Rust-сервіс setup (Частина 1+2) — /api/v1/setup, перший власник + персональна БД.
    pub setup: Option<std::sync::Arc<dyn SetupService + Send + Sync>>,
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
/// Якщо `TORGASHKA_RUST_READDIRS=1` і БД доступна — повертає (пул, read-репо,
/// write-репо). Інакше `None` (роути не монтуються → fallback → 410).
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
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_READDIRS_ENV}=1 — Rust-гілка довідників увімкнена (PostgreSQL, read-write)"
            );
            let store_pool = StorePool::new(pool.clone());
            let read = Arc::new(
                torgashka_infrastructure::repositories::directories::SqlxDirectories::new(
                    store_pool.clone(),
                ),
            ) as Arc<dyn ReadDirectories + Send + Sync>;
            let write = Arc::new(
                torgashka_infrastructure::repositories::write::SqlxWriteDirectories::new(
                    store_pool.clone(),
                ),
            ) as Arc<dyn WriteDirectories + Send + Sync>;
            let pos = Arc::new(torgashka_infrastructure::repositories::pos::SqlxPos::new(
                store_pool.clone(),
            )) as Arc<dyn PosService + Send + Sync>;
            let ledger = Arc::new(
                torgashka_infrastructure::repositories::ledger::SqlxLedger::new(store_pool.clone()),
            ) as Arc<dyn LedgerService + Send + Sync>;
            let auth = Arc::new(torgashka_infrastructure::repositories::auth::SqlxAuth::new(
                store_pool.clone(),
            )) as Arc<dyn AuthService + Send + Sync>;
            Some((pool, read, write, pos, ledger, auth))
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_READDIRS_ENV}=1, але БД недоступна ({e}); \
                 довідники не змонтовано (LEGACY → 410)"
            );
            None
        }
    }
}

/// Ініціалізує Rust-гілку OCR під TORGASHKA_RUST_OCR=1.
/// Повертає (OcrService, пул БД для invoice-ocr зіставлення).
async fn init_ocr() -> (
    Option<std::sync::Arc<torgashka_ocr::OcrService>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_OCR_ENV) {
        return (None, None);
    }
    match torgashka_infrastructure::db::connect_readonly_pool(5).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_OCR_ENV}=1 — Rust-гілка OCR увімкнена (PostgreSQL; Gemini keys: {:?})",
                torgashka_ocr::OcrService::new().client().keys_file_hint()
            );
            (
                Some(std::sync::Arc::new(torgashka_ocr::OcrService::new())),
                Some(pool),
            )
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_OCR_ENV}=1, але БД недоступна ({e}); OCR не змонтовано (LEGACY → 410)"
            );
            (None, None)
        }
    }
}

/// Ініціалізує Rust-гілку ПРРО під TORGASHKA_RUST_PRRO (1|shadow).
async fn init_prro() -> Option<Arc<crate::prro::PrroFacade>> {
    let mode = std::env::var(RUST_PRRO_ENV).unwrap_or_default();
    if !matches!(mode.trim().to_lowercase().as_str(), "1" | "true" | "shadow") {
        return None;
    }
    match torgashka_infrastructure::db::connect_readonly_pool(5).await {
        Ok(pool) => {
            match torgashka_infrastructure::prro::SqlxPrroRepository::connect(StorePool::new(pool))
                .await
            {
                Ok(repo) => {
                    let shadow = mode.trim().to_lowercase() == "shadow";
                    eprintln!(
                        "[torgashka-api] {RUST_PRRO_ENV}={mode} — Rust-гілка ПРРО увімкнена (shadow={shadow}, PostgreSQL)"
                    );
                    Some(Arc::new(crate::prro::PrroFacade::new(repo, shadow)))
                }
                Err(e) => {
                    eprintln!(
                        "[torgashka-api] попередження: {RUST_PRRO_ENV}={mode}, але схему ПРРО не створено ({e}); роути не змонтовано (LEGACY → 410)"
                    );
                    None
                }
            }
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_PRRO_ENV}={mode}, але БД недоступна ({e}); роути не змонтовано (LEGACY → 410)"
            );
            None
        }
    }
}

/// Ініціалізує Rust-гілку документів під TORGASHKA_RUST_DOCUMENTS=1.
async fn init_documents() -> (
    Option<Arc<dyn DocumentsService + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_DOCUMENTS_ENV) {
        return (None, None);
    }
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_DOCUMENTS_ENV}=1 — Rust-гілка документів увімкнена (PostgreSQL)"
            );
            let svc: Arc<dyn DocumentsService + Send + Sync> = Arc::new(
                torgashka_infrastructure::repositories::documents::SqlxDocuments::new(
                    StorePool::new(pool.clone()),
                ),
            );
            (Some(svc), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_DOCUMENTS_ENV}=1, але БД недоступна ({e}); документи через роути не змонтовано (LEGACY → 410)"
            );
            (None, None)
        }
    }
}

/// Ініціалізує Rust-гілку друку під TORGASHKA_RUST_PRINT=1.
async fn init_print_templates() -> (
    Option<Arc<dyn PrintTemplatesService + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_PRINT_ENV) {
        return (None, None);
    }
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_PRINT_ENV}=1 — Rust-гілка друку увімкнена (PostgreSQL)"
            );
            let repo =
                torgashka_infrastructure::repositories::print_templates::SqlxPrintTemplates::new(
                    StorePool::new(pool.clone()),
                );
            let svc: Arc<dyn PrintTemplatesService + Send + Sync> = Arc::new(repo);
            (Some(svc), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_PRINT_ENV}=1, але БД недоступна ({e}); друк через роути не змонтовано (LEGACY → 410)"
            );
            (None, None)
        }
    }
}

/// Ініціалізує Rust-гілку товарів v2 під TORGASHKA_RUST_PRODUCTS_V2=1.
async fn init_products_v2() -> (
    Option<Arc<dyn ProductsV2Service + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_PRODUCTS_V2_ENV) {
        return (None, None);
    }
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_PRODUCTS_V2_ENV}=1 — Rust-гілка товарів v2 увімкнена (PostgreSQL)"
            );
            let repo = torgashka_infrastructure::repositories::products_v2::SqlxProductsV2::new(
                StorePool::new(pool.clone()),
            );
            let svc: Arc<dyn ProductsV2Service + Send + Sync> = Arc::new(repo);
            (Some(svc), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_PRODUCTS_V2_ENV}=1, але БД недоступна ({e}); товари v2 через роути не змонтовано (LEGACY → 410)"
            );
            (None, None)
        }
    }
}

/// Ініціалізує Rust-гілку інвойсів під TORGASHKA_RUST_INVOICES=1.
/// Ініціалізує Rust-гілку повернень під TORGASHKA_RUST_RETURN_INVOICES=1.
async fn init_return_invoices() -> (
    Option<Arc<dyn ReturnInvoicesService + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_RETURN_INVOICES_ENV) {
        return (None, None);
    }
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_RETURN_INVOICES_ENV}=1 — Rust-гілка повернень увімкнена (PostgreSQL)"
            );
            let repo =
                torgashka_infrastructure::repositories::return_invoices::SqlxReturnInvoices::new(
                    StorePool::new(pool.clone()),
                );
            let svc: Arc<dyn ReturnInvoicesService + Send + Sync> = Arc::new(repo);
            (Some(svc), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_RETURN_INVOICES_ENV}=1, але БД недоступна ({e}); повернення через роути не змонтовано (LEGACY → 410)"
            );
            (None, None)
        }
    }
}

/// Ініціалізує Rust-гілку замовлень постачальнику під TORGASHKA_RUST_PURCHASE_ORDERS=1.
async fn init_purchase_orders() -> (
    Option<Arc<dyn PurchaseOrdersService + Send + Sync>>,
    Option<PgPool>,
) {
    if !env_flag(RUST_PURCHASE_ORDERS_ENV) {
        return (None, None);
    }
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_PURCHASE_ORDERS_ENV}=1 — Rust-гілка замовлень увімкнена (PostgreSQL)"
            );
            let repo =
                torgashka_infrastructure::repositories::purchase_orders::SqlxPurchaseOrders::new(
                    StorePool::new(pool.clone()),
                );
            let svc: Arc<dyn PurchaseOrdersService + Send + Sync> = Arc::new(repo);
            (Some(svc), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_PURCHASE_ORDERS_ENV}=1, але БД недоступна ({e}); замовлення через роути не змонтовано (LEGACY → 410)"
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
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_INVOICES_ENV}=1 — Rust-гілка інвойсів увімкнена (PostgreSQL)"
            );
            let repo = torgashka_infrastructure::repositories::invoices::SqlxInvoices::new(
                StorePool::new(pool.clone()),
            );
            let v1: Arc<dyn InvoicesV1Service + Send + Sync> = Arc::new(repo);
            let repo2 = torgashka_infrastructure::repositories::invoices::SqlxInvoices::new(
                StorePool::new(pool.clone()),
            );
            let v2: Arc<dyn InvoicesV2Service + Send + Sync> = Arc::new(repo2);
            (Some(v1), Some(v2), Some(pool))
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_INVOICES_ENV}=1, але БД недоступна ({e}); інвойси через роути не змонтовано (LEGACY → 410)"
            );
            (None, None, None)
        }
    }
}

/// Ініціалізує Rust-гілку боржників під TORGASHKA_RUST_DEBTORS=1.
async fn init_debtors() -> Option<Arc<dyn DebtorService + Send + Sync>> {
    if !env_flag(RUST_DEBTORS_ENV) {
        return None;
    }
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            eprintln!(
                "[torgashka-api] {RUST_DEBTORS_ENV}=1 — Rust-гілка боржників увімкнена (PostgreSQL)"
            );
            Some(Arc::new(
                torgashka_infrastructure::repositories::debtors::SqlxDebtors::new(StorePool::new(
                    pool,
                )),
            ) as Arc<dyn DebtorService + Send + Sync>)
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_DEBTORS_ENV}=1, але БД недоступна ({e}); боржники через роути не змонтовано (LEGACY → 410)"
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
            eprintln!("[torgashka-api] фасад на {addr} завершився з помилкою: {e}");
        }
    })
}

/// Async-реалізація фасаду (біндинг + serve).
///
/// Публічна — щоб Tauri-шар міг спавнити фасад через власний runtime
/// (`tauri::async_runtime::spawn`), а не через глобальний tokio::spawn.
pub async fn serve(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_listener(listener).await
}

/// Запустити фасад на вже прив'язаному слухачі.
///
/// Викликається з `lib.rs` застосунку: бінд виконується СИНХРОННО до створення
/// вікна, щоб порт :8000 був зайнятий ще до завантаження webview — інакше
/// фронтенд ловить ECONNREFUSED під час ініціалізації (гонка при старті).
pub async fn serve_listener(
    listener: tokio::net::TcpListener,
) -> Result<(), Box<dyn std::error::Error>> {
    // ── Діагностичний лог: безумовно — torgashka.log має з'являтися завжди ──
    // (stderr на Windows приховано windows_subsystem=windows — це єдиний канал)
    torgashka_infrastructure::embedded_pg::pg_log("INFO", "serve_listener: старт");
    match torgashka_infrastructure::db::resolve_database_url() {
        Ok(url) => torgashka_infrastructure::embedded_pg::pg_log(
            "INFO",
            &format!("resolve_database_url: Ok ({url}) — embedded PG пропускаємо"),
        ),
        Err(_) => torgashka_infrastructure::embedded_pg::pg_log(
            "INFO",
            "resolve_database_url: Err — запускаємо embedded PostgreSQL",
        ),
    }
    // Етап 8 — повна дезактивація Python sidecar: Rust-ядро за замовчуванням.
    // Env-флаги можна явно перевизначити (напр. TORGASHKA_RUST_PRRO=0) для тестів.
    for (flag, val) in DEFAULT_RUST_FLAGS {
        if std::env::var_os(flag).is_none() {
            std::env::set_var(flag, val);
        }
    }
    // Embedded PostgreSQL (Windows-збірка без системного PG): якщо звичайний
    // резолв DATABASE_URL (env → backend/.env) не дав результату — шукаємо
    // локальні бінарники PG (TORGASHKA_PG_DIR, resources/postgres, .cache/pg,
    // системний PG на Linux), ініціалізуємо data_dir, піднімаємо сервер на
    // 127.0.0.1:5433 і встановлюємо DATABASE_URL для решти процесу. При
    // завершенні serve_listener (Drop) сервер зупиняється pg_ctl stop.
    let _embedded_pg = if torgashka_infrastructure::db::resolve_database_url().is_err() {
        match torgashka_infrastructure::embedded_pg::bootstrap_if_needed() {
            Ok(pg) => {
                // Файлове логування (torgashka.log) — stderr на Windows приховано
                torgashka_infrastructure::embedded_pg::pg_log(
                    "INFO",
                    &format!(
                        "вбудований PostgreSQL: {} (data_dir: {})",
                        pg.database_url(),
                        pg.data_dir().display()
                    ),
                );
                Some(pg)
            }
            Err(e) => {
                torgashka_infrastructure::embedded_pg::pg_log(
                    "ERROR",
                    &format!("вбудований PostgreSQL недоступний ({e}); працюємо без БД"),
                );
                None
            }
        }
    } else {
        None
    };
    // Авто-міграції (Частина 1.2): застосувати схему на fresh-БД ПЕРЕД
    // підняттям listener. Ідемпотентно: повна схема лише якщо users немає;
    // owners_db створюється завжди (CREATE TABLE IF NOT EXISTS).
    match torgashka_infrastructure::db::connect_readonly_pool(5).await {
        Ok(pool) => {
            if let Err(e) = torgashka_infrastructure::db::ensure_schema(&pool).await {
                eprintln!("[torgashka-api] попередження: авто-міграція схеми не виконана: {e}");
            }
            pool.close().await;
        }
        Err(e) => {
            eprintln!("[torgashka-api] попередження: БД недоступна для авто-міграції ({e})");
        }
    }
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
    // Окремий флаг auth: TORGASHKA_RUST_AUTH=1 вмикає Rust-гілку auth навіть якщо
    // readdirs вимкнено (проксі-режим для решти) — але пул створюється спільно.
    let auth = if env_flag(RUST_AUTH_ENV) && auth.is_none() {
        match torgashka_infrastructure::db::connect_readonly_pool(10).await {
            Ok(pool) => {
                eprintln!(
                    "[torgashka-api] {RUST_AUTH_ENV}=1 — Rust-гілка auth увімкнена (PostgreSQL)"
                );
                Some(
                    Arc::new(torgashka_infrastructure::repositories::auth::SqlxAuth::new(
                        StorePool::new(pool),
                    )) as Arc<dyn AuthService + Send + Sync>,
                )
            }
            Err(e) => {
                eprintln!(
                    "[torgashka-api] попередження: {RUST_AUTH_ENV}=1, але БД недоступна ({e}); auth через роути не змонтовано (LEGACY → 410)"
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
    let (purchase_orders, purchase_orders_pool) = init_purchase_orders().await;
    let (print_templates, print_pool) = init_print_templates().await;
    let (products_v2, products_v2_pool) = init_products_v2().await;
    let uploads_dir = std::env::var("TORGASHKA_UPLOADS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("uploads"));
    let (ocr, ocr_pool) = init_ocr().await;
    // StoreContext (Етап 3): пул для middleware + сервіс точок.
    let store_pool = write_pool.clone().map(StorePool::new);
    let stores = store_pool.as_ref().map(|sp| {
        Arc::new(torgashka_infrastructure::repositories::stores::SqlxStoreService::new(sp.clone()))
            as Arc<dyn StoreService + Send + Sync>
    });
    // Setup (Частина 1+2): той самий пул мета-БД, що й stores.
    let setup = write_pool.clone().map(|p| {
        Arc::new(
            torgashka_infrastructure::repositories::setup::SqlxSetupService::new(StorePool::new(p)),
        ) as Arc<dyn SetupService + Send + Sync>
    });
    let state = AppState {
        jwt_secret: Arc::new(auth::resolve_jwt_secret()?),
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
        purchase_orders,
        purchase_orders_pool,
        print_templates,
        print_pool,
        products_v2,
        products_v2_pool,
        ocr,
        ocr_pool,
        uploads_dir,
        store_pool,
        stores,
        setup,
    };
    let app = router_v1::build_router(state);
    eprintln!(
        "[torgashka-api] фасад слухає http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;
    Ok(())
}
