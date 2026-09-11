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
pub mod admin_audit;
pub mod admin_db_sources;
pub mod admin_migrate;
pub mod admin_network_config;
pub mod admin_prro;
pub mod admin_reports;
pub mod auth;
pub mod auth_routes;
pub mod categories_v2;
pub mod crud;
pub mod debtors;
pub mod documents;
pub mod invoices;
pub mod ledger;
pub mod network;
pub mod network_nodes;
pub mod ocr;
pub mod pos;
pub mod print_templates;
pub mod products_v2;
pub mod promote;
pub mod proxy;
pub mod prro;
pub mod purchase_orders;
pub mod readdirs;
pub mod readonly_net;
pub mod return_invoices;
pub mod route_local;
pub mod router_v1;
pub mod setup;
pub mod store_context;
pub mod stores;
pub mod suppliers;
pub mod sync;
pub mod sync_receivers;
pub mod write_gate;

use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::PgPool;
use torgashka_domain::{
    AuthService, DebtorService, DocumentsService, InvoicesV1Service, InvoicesV2Service,
    LedgerService, PosService, PrintTemplatesService, ProductsV2Service, PurchaseOrdersService,
    ReadDirectories, ReturnInvoicesService, SetupService, StoreService, WriteDirectories,
};
use torgashka_infrastructure::node_config::NodeMode;
use torgashka_infrastructure::repositories::outbox_debtors::OutboxDebtors;
use torgashka_infrastructure::repositories::outbox_invoices::{OutboxInvoicesV1, OutboxInvoicesV2};
use torgashka_infrastructure::repositories::outbox_ledger::OutboxLedger;
use torgashka_infrastructure::repositories::outbox_pos::OutboxPos;
use torgashka_infrastructure::repositories::outbox_purchase_orders::OutboxPurchaseOrders;
use torgashka_infrastructure::repositories::outbox_return_invoices::OutboxReturnInvoices;
use torgashka_infrastructure::repositories::outbox_write::OutboxWrite;
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
    /// Конфігурація вузла мережі (ЕТАП 18): mode Primary|Standby, local_port,
    /// degrade_to_local. Завжди заповнена (default = Primary — стара поведінка).
    pub node_config: torgashka_infrastructure::node_config::NodeConfig,
    /// Локальний standby-стан (репліка 127.0.0.1:local_port + сервіси) —
    /// `Some` лише коли mode=Standby І репліка доступна при старті фасаду.
    pub local: Option<crate::route_local::LocalApiState>,
}

/// Чистий payload для /api/v1/health (використовується роутером і diff CLI).
pub fn health_payload() -> serde_json::Value {
    serde_json::json!({"status": "ok"})
}

/// Чиста функція echo для differential CLI (повертає args без змін).
pub fn echo_payload(args: &serde_json::Value) -> serde_json::Value {
    args.clone()
}

/// Режим вузла мережі (ADR-0007 F6): читається локально з `node_config`,
/// дешеве читання файлу. Викликається ЛИШЕ на старті фасаду (вибір адаптерів
/// `pos`/`write`/`invoices`/`purchase_orders`) — жодних per-request розгалужень.
fn node_cfg_mode() -> NodeMode {
    torgashka_infrastructure::node_config::NodeConfig::load().mode
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
async fn init_readdirs() -> Result<
    (
        PgPool,
        Arc<dyn ReadDirectories + Send + Sync>,
        Arc<dyn WriteDirectories + Send + Sync>,
        Arc<dyn PosService + Send + Sync>,
        Arc<dyn LedgerService + Send + Sync>,
        Arc<dyn AuthService + Send + Sync>,
    ),
    String,
> {
    if !env_flag(RUST_READDIRS_ENV) {
        return Err(format!(
            "{RUST_READDIRS_ENV} не увімкнено (1/true/yes) — Rust-гілку довідників не монтуємо"
        ));
    }
    match torgashka_infrastructure::db::connect_readonly_pool(10).await {
        Ok(pool) => {
            torgashka_infrastructure::embedded_pg::pg_log(
                "INFO",
                &format!(
                    "{RUST_READDIRS_ENV}=1 — Rust-гілка довідників увімкнена (PostgreSQL, read-write)"
                ),
            );
            let store_pool = StorePool::new(pool.clone());
            let read = Arc::new(
                torgashka_infrastructure::repositories::directories::SqlxDirectories::new(
                    store_pool.clone(),
                ),
            ) as Arc<dyn ReadDirectories + Send + Sync>;
            // ADR-0007 §11.7.9.7 (Фаза 3.3a): вибір write-АДАПТЕРА за режимом
            // вузла — рівно як `pos`. standby: `OutboxWrite` (інвентаризація →
            // SQLite-черга, довідники → відмова «потрібен головний сервер»;
            // їх виконує primary через гейт `ProxyToPrimary`).
            let write = match node_cfg_mode() {
                NodeMode::Primary => Arc::new(
                    torgashka_infrastructure::repositories::write::SqlxWriteDirectories::new(
                        store_pool.clone(),
                    ),
                ) as Arc<dyn WriteDirectories + Send + Sync>,
                NodeMode::Standby => Arc::new(OutboxWrite::new(Arc::new(
                    torgashka_infrastructure::repositories::write::SqlxWriteDirectories::new(
                        store_pool.clone(),
                    ),
                ))) as Arc<dyn WriteDirectories + Send + Sync>,
            };
            // ADR-0007 §11.7.9.7 (Фаза 3.3b): книга постачальника — той самий
            // вибір адаптера за режимом. standby: `OutboxLedger` (ручний запис →
            // SQLite-черга; до цього — сирий 500 у read-only репліку).
            let ledger = match node_cfg_mode() {
                NodeMode::Primary => Arc::new(
                    torgashka_infrastructure::repositories::ledger::SqlxLedger::new(
                        store_pool.clone(),
                    ),
                ) as Arc<dyn LedgerService + Send + Sync>,
                NodeMode::Standby => Arc::new(OutboxLedger::new(Arc::new(
                    torgashka_infrastructure::repositories::ledger::SqlxLedger::new(
                        store_pool.clone(),
                    ),
                ))) as Arc<dyn LedgerService + Send + Sync>,
            };
            // ADR-0007 F6 (режим читається локально, дешеве читання файлу)
            // + §11.1: вибір POS-АДАПТЕРА за режимом вузла.
            //   * primary — PG-репозиторій (F2: поведінка байт-в-байт);
            //   * standby — `OutboxPos`: читання з локальної репліки (§10),
            //     запис POS-документів — у SQLite-чергу (`LocalOutbox`).
            //     До цього standby писав у read-only репліку → 500
            //     «read-only transaction» на кожному чеку.
            let node_cfg = torgashka_infrastructure::node_config::NodeConfig::load();
            let pos: Arc<dyn PosService + Send + Sync> = match node_cfg.mode {
                NodeMode::Primary => Arc::new(
                    torgashka_infrastructure::repositories::pos::SqlxPos::new(store_pool.clone()),
                ),
                NodeMode::Standby => Arc::new(OutboxPos::new(Arc::new(
                    torgashka_infrastructure::repositories::pos::SqlxPos::new(store_pool.clone()),
                ))),
            };
            let pos = pos as Arc<dyn PosService + Send + Sync>;
            let auth = Arc::new(
                torgashka_infrastructure::repositories::auth::SqlxAuth::with_standby(
                    store_pool.clone(),
                    node_cfg.is_standby(),
                ),
            ) as Arc<dyn AuthService + Send + Sync>;
            Ok((pool, read, write, pos, ledger, auth))
        }
        Err(e) => {
            // Windows-каса: stderr прихований (windows_subsystem=windows) —
            // причина мусить лягти в torgashka.log.
            torgashka_infrastructure::embedded_pg::pg_log(
                "ERROR",
                &format!("{RUST_READDIRS_ENV}=1, але БД недоступна — пул читання НЕ створено: {e}"),
            );
            Err(format!("пул читання не створено: {e}"))
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
            // ADR-0007 §11.7.9.7 (Фаза 3.3b): повернення ПОСТАЧАЛЬНИКУ —
            // standby пише документ у SQLite-чергу (`TYPE_RETURN_INVOICE`).
            let svc: Arc<dyn ReturnInvoicesService + Send + Sync> = match node_cfg_mode() {
                NodeMode::Primary => Arc::new(repo),
                NodeMode::Standby => Arc::new(OutboxReturnInvoices::new(Arc::new(repo))),
            };
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
            let svc: Arc<dyn PurchaseOrdersService + Send + Sync> = match node_cfg_mode() {
                NodeMode::Primary => Arc::new(repo),
                NodeMode::Standby => Arc::new(OutboxPurchaseOrders::new(Arc::new(repo))),
            };
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
            let repo2 = torgashka_infrastructure::repositories::invoices::SqlxInvoices::new(
                StorePool::new(pool.clone()),
            );
            let (v1, v2): (
                Arc<dyn InvoicesV1Service + Send + Sync>,
                Arc<dyn InvoicesV2Service + Send + Sync>,
            ) = match node_cfg_mode() {
                NodeMode::Primary => (Arc::new(repo), Arc::new(repo2)),
                NodeMode::Standby => (
                    Arc::new(OutboxInvoicesV1::new(Arc::new(repo))),
                    Arc::new(OutboxInvoicesV2::new(Arc::new(repo2))),
                ),
            };
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
            let repo = Arc::new(
                torgashka_infrastructure::repositories::debtors::SqlxDebtors::new(StorePool::new(
                    pool,
                )),
            );
            // ADR-0007 §11.6.4 варіант 1 + §11.7.9.7 (Фаза 3.3b): боргові
            // сутності — `LocalOutbox`. standby: оплата боргу → SQLite-черга
            // (`TYPE_DEBTOR_PAYMENT`), створення/редагування боржника — відмова.
            Some(match node_cfg_mode() {
                NodeMode::Primary => repo as Arc<dyn DebtorService + Send + Sync>,
                NodeMode::Standby => {
                    Arc::new(OutboxDebtors::new(repo)) as Arc<dyn DebtorService + Send + Sync>
                }
            })
        }
        Err(e) => {
            eprintln!(
                "[torgashka-api] попередження: {RUST_DEBTORS_ENV}=1, але БД недоступна ({e}); боржники через роути не змонтовано (LEGACY → 410)"
            );
            None
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ФІКС ДЕФЕКТУ 5+6 (Torgashka, 2026-08): фасад :8000 відповідає ЗАВЖДИ, а
// ініціалізація БД не виконується в async-потоці й НЕ робить bootstrap на
// каталозі репліки.
//
// Було: `serve_listener` викликав СИНХРОННИЙ `bootstrap_if_needed()`
// (subprocess-и: initdb/pg_ctl/psql) ПРЯМО в async-контексті ДО `axum::serve`.
// Порт :8000 уже слухав (бінд у src/lib.rs), ядро завершувало TCP-handshake →
// клієнт бачив «з'єднання встановлено», але HTTP-відповіді не було НІКОЛИ
// (curl: 0 bytes received + timeout; CLOSE_WAIT/FIN_WAIT_2). psql без `-w`
// міг застигнути на запиті пароля НАЗАВЖДИ (GUI-процес без консолі).
//
// Стало: HTTP обслуговується з першої секунди (boot-gate), важкі кроки — у
// `spawn_blocking`, кожен крок логується з часом.
// ─────────────────────────────────────────────────────────────────────────────

/// План підготовки БД при старті фасаду (ЧИСТА функція — тестована без env).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbStartupPlan {
    /// DATABASE_URL резолвиться ззовні (env → db_sources.toml → backend/.env):
    /// embedded PG не чіпаємо взагалі.
    ExternalUrl(String),
    /// Standby-вузол: ЛИШЕ підняти локальну репліку (ідемпотентно).
    /// initdb/CREATE DATABASE на каталозі репліки ЗАБОРОНЕНІ — каталог
    /// отримано `pg_basebackup`, це hot standby (read-only): CREATE DATABASE
    /// там неможливий, а спроба bootstrap псує каталог репліки (дефект 6).
    StandbyReplica(String),
    /// Standby-вузол, але ім'я локальної БД НЕВІДОМЕ (`[node] primary_db_url`
    /// і env `TORGASHKA_PG_DB` порожні). `DATABASE_URL` не вигадуємо: пули й
    /// роути не створюються, причина (з полем) іде в `torgashka.log`.
    /// Раніше тут мовчки підставлялась БД «torgashka», якої на касі немає
    /// (дефект 2026-09: `/api/v1/setup/status` = 503 назавжди).
    StandbyWithoutDbName(String),
    /// Primary-вузол: повний bootstrap (initdb → pg_ctl start → CREATE DATABASE).
    PrimaryBootstrap,
}

/// Вибір плану підготовки БД (чиста логіка — покрита тестами).
pub fn plan_db_startup(
    resolved: Option<&str>,
    is_standby: bool,
    standby_url: Option<String>,
) -> DbStartupPlan {
    if let Some(url) = resolved.filter(|u| !u.trim().is_empty()) {
        return DbStartupPlan::ExternalUrl(url.to_string());
    }
    if is_standby {
        // Порожній/відсутній URL — НЕ «порожній рядок у DATABASE_URL», а чесна
        // відмова монтувати пули: ім'я БД невідоме, здогадуватись заборонено.
        return match standby_url.filter(|u| !u.trim().is_empty()) {
            Some(url) => DbStartupPlan::StandbyReplica(url),
            None => DbStartupPlan::StandbyWithoutDbName(
                "ім'я локальної БД невідоме: ні [node] primary_db_url, ні TORGASHKA_PG_DB"
                    .to_string(),
            ),
        };
    }
    DbStartupPlan::PrimaryBootstrap
}

/// Ім'я БД із `postgresql://` URL: частина після першого '/' у host-секції
/// (query-параметри відкидаються). `None` — URL без імені БД
/// (`postgresql://user@host:5432`) або без '/'.
fn db_name_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let after_userinfo = match after_scheme.find('@') {
        Some(i) => &after_scheme[i + 1..],
        None => after_scheme,
    };
    let path = &after_userinfo[after_userinfo.find('/')? + 1..];
    let name = path.split(['/', '?']).next().unwrap_or("").trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Резолв URL локальної репліки standby-вузла (ЧИСТА функція — тестована без
/// env/файлів). `Ok` — URL для пула; `Err(reason)` — явна причина, чому пул
/// створювати НЕ можна (жодних здогадок імені БД).
///
/// Джерела імені БД у порядку пріоритету:
/// 1. `primary_db_url` (поле `[node]` через
///    [`torgashka_infrastructure::node_config::NodeConfig::resolve_primary_db_url`]
///    — або активне джерело db_sources.toml): host:port → `127.0.0.1:local_port`,
///    userinfo та ім'я БД зберігаються, пароль відкидається (дефект 5);
/// 2. env `TORGASHKA_PG_DB`.
///
/// Літерала «torgashka» тут НЕМА: саме він давав продакшн-дефект 2026-09 —
/// каса-standby підключалась до неіснуючої БД, `/api/v1/setup/status` = 503
/// назавжди, логін-гейт каси висне (postgres.log: `FATAL: database "torgashka"
/// does not exist`).
pub fn standby_local_url_or_err(
    primary_url: Option<&str>,
    local_port: u16,
    user: &str,
    env_db: Option<&str>,
) -> Result<String, String> {
    // Тонка делегація: джерела (c)/(d) у цій сигнатурі відсутні (None) —
    // старі виклики/тести працюють без змін.
    standby_local_url_wide(primary_url, local_port, user, env_db, None, None).map(|(url, _src)| url)
}

// ─────────────────────────────────────────────────────────────────────────────
// ЛАНЦЮГ ДЖЕРЕЛ ІМЕНІ ЛОКАЛЬНОЇ БД (standby-каса без провіжну)
// ─────────────────────────────────────────────────────────────────────────────
// На касі, де `[node] primary_db_url` не зберігся (провіжн обірвався) і env
// `TORGASHKA_PG_DB` не задано, фасад визначає ім'я локальної репліки САМ —
// без оператора. Джерела в порядку пріоритету:
//   (a) `[node] primary_db_url`                        → PrimaryUrl
//   (b) env `TORGASHKA_PG_DB`                          → EnvDb
//   (c) SQLite settings `node_replication_database`    → NodeSettings
//   (d) проба ЖИВОЇ репліки на 127.0.0.1:<local_port>  → ReplicaProbe
// Здогадок немає: жодне джерело не дало РІВНО одного імені → Err із переліком
// УСЬОГО, що перевірено (причина йде в torgashka.log).

/// Результат проби живої репліки (передається у чисту логіку як дані).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicaDbProbe {
    /// Рівно одне ім'я БД знайдено на живій репліці.
    Ok(String),
    /// Проба неможлива / дала 0 або ≥2 БД (текст — причина з переліком).
    Err(String),
    /// Пробу не виконували (напр. ім'я вже відоме з джерела (c)).
    NotAttempted,
}

/// Звідки взято ім'я локальної БД (для логу/діагностики).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDbNameSource {
    /// `[node] primary_db_url` (або активне джерело db_sources.toml).
    PrimaryUrl,
    /// env `TORGASHKA_PG_DB`.
    EnvDb,
    /// SQLite settings `node_replication_database` (збережено join-екраном).
    NodeSettings,
    /// Проба живої репліки (рівно одна не-шаблонна БД).
    ReplicaProbe,
}

impl LocalDbNameSource {
    /// Коротке ім'я джерела для логів.
    pub fn as_str(&self) -> &'static str {
        match self {
            LocalDbNameSource::PrimaryUrl => "[node] primary_db_url",
            LocalDbNameSource::EnvDb => "env TORGASHKA_PG_DB",
            LocalDbNameSource::NodeSettings => "SQLite settings node_replication_database",
            LocalDbNameSource::ReplicaProbe => "проба живої репліки",
        }
    }
}

/// ЧИСТА: класифікація результату проби (список datname → Ok(1) / Err(0 чи ≥2)).
///
/// Жодних здогадок: 0 БД (репліка не провіжнена/порожня) і ≥2 БД (неоднозначно)
/// — це Err із ПЕРЕЛІКОМ знайденого.
pub fn classify_replica_probe(databases: &[String]) -> Result<String, String> {
    let names: Vec<String> = databases
        .iter()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .collect();
    match names.len() {
        1 => Ok(names.into_iter().next().unwrap_or_default()),
        0 => Err(
            "проба живої репліки: знайдено 0 баз (очікували рівно 1) — імені БД немає".to_string(),
        ),
        n => Err(format!(
            "проба живої репліки: знайдено {n} баз [{}] (очікували рівно 1 — яку взяти, невідомо)",
            names.join(", ")
        )),
    }
}

/// Спільний текст помилки: перелік УСІХ перевірених джерел (для логу/тестів).
fn local_db_name_sources_err(detail: &str) -> String {
    format!(
        "локальна репліка недоступна: імені БД немає — перевірено [node] primary_db_url, \
         env TORGASHKA_PG_DB, SQLite settings node_replication_database, пробу живої репліки \
         ({detail}); пул НЕ створюється, здогадка імені БД («torgashka») вимкнена"
    )
}

/// ЧИСТА логіка ланцюга (a>b>c>d). `Ok` = (ім'я БД, джерело).
pub fn local_db_name_or_err(
    primary_url: Option<&str>,
    env_db: Option<&str>,
    settings_db: Option<&str>,
    probe: Option<&ReplicaDbProbe>,
) -> Result<(String, LocalDbNameSource), String> {
    if let Some(name) = primary_url.and_then(db_name_from_url) {
        return Ok((name, LocalDbNameSource::PrimaryUrl));
    }
    if let Some(db) = env_db.map(str::trim).filter(|d| !d.is_empty()) {
        return Ok((db.to_string(), LocalDbNameSource::EnvDb));
    }
    if let Some(db) = settings_db.map(str::trim).filter(|d| !d.is_empty()) {
        return Ok((db.to_string(), LocalDbNameSource::NodeSettings));
    }
    match probe {
        Some(ReplicaDbProbe::Ok(name)) => {
            let db = classify_replica_probe(std::slice::from_ref(name))?;
            Ok((db, LocalDbNameSource::ReplicaProbe))
        }
        Some(ReplicaDbProbe::Err(e)) => Err(local_db_name_sources_err(e)),
        None | Some(ReplicaDbProbe::NotAttempted) => {
            Err(local_db_name_sources_err("пробу не виконували"))
        }
    }
}

/// ЧИСТА: локальний URL з джерел; `Err` — перелік УСІХ перевірених джерел.
///
/// `primary_db_url` з іменем БД → host:port переписується на `127.0.0.1:local_port`
/// ([`torgashka_infrastructure::node_config::local_readonly_url`], без пароля);
/// джерела (b)/(c)/(d) → [`torgashka_infrastructure::node_config::fallback_local_url`]
/// (`postgresql://{user}@127.0.0.1:{local_port}/{db}`).
pub fn standby_local_url_wide(
    primary_url: Option<&str>,
    local_port: u16,
    user: &str,
    env_db: Option<&str>,
    settings_db: Option<&str>,
    probe: Option<&ReplicaDbProbe>,
) -> Result<(String, LocalDbNameSource), String> {
    if let Some(primary) = primary_url.map(str::trim).filter(|u| !u.is_empty()) {
        if db_name_from_url(primary).is_some() {
            let url =
                torgashka_infrastructure::node_config::local_readonly_url(primary, local_port)
                    .ok_or_else(|| format!("primary_db_url не є postgresql:// URL: {primary}"))?;
            return Ok((url, LocalDbNameSource::PrimaryUrl));
        }
        // primary_db_url є, але БЕЗ імені БД — не підставляємо нічого, пробуємо
        // джерела нижче (теж без здогадок).
    }
    let (db, src) = local_db_name_or_err(primary_url, env_db, settings_db, probe)?;
    Ok((
        torgashka_infrastructure::node_config::fallback_local_url(local_port, &db, user),
        src,
    ))
}

/// Резолв + лог: ядро [`standby_url_for`] із ін'єкцією логера (тестовано).
/// Причина невдачі ЗАВЖДИ йде в лог-канал — на Windows-касі
/// (`windows_subsystem=windows`) `torgashka.log` єдиний видимий канал.
pub fn standby_url_with_logger<F: FnMut(&str, &str)>(
    primary_url: Option<&str>,
    local_port: u16,
    user: &str,
    env_db: Option<&str>,
    mut log: F,
) -> Option<String> {
    match standby_local_url_or_err(primary_url, local_port, user, env_db) {
        Ok(url) => Some(url),
        Err(reason) => {
            log("ERROR", &reason);
            None
        }
    }
}

/// URL локальної репліки з конфіга вузла + креденшалів з env (не чиста —
/// env-обгортка над [`standby_local_url_or_err`]). `None` — імені БД немає:
/// ERROR уже записано в `torgashka.log`, пул НЕ створюється.
fn standby_url_for(cfg: &torgashka_infrastructure::node_config::NodeConfig) -> Option<String> {
    let user = std::env::var("TORGASHKA_PG_USER").unwrap_or_else(|_| "postgres".to_string());
    let env_db = std::env::var("TORGASHKA_PG_DB").ok();
    let primary = cfg.resolve_primary_db_url();
    standby_url_with_logger(
        primary.as_deref(),
        cfg.local_port,
        &user,
        env_db.as_deref(),
        torgashka_infrastructure::embedded_pg::pg_log,
    )
}

/// Шляхи-«проби готовності»: під час ініціалізації відповідаємо негайно (503),
/// не змушуючи клієнта чекати (фронтенд ретраїть /setup/status кожні 2 с).
fn is_readiness_path(path: &str) -> bool {
    path == "/api/v1/health" || path == "/api/v1/setup/status"
}

/// Чесна 503 без очікування: `starting` (ініціалізація триває) або причина.
fn not_ready_response(kind: &str, detail: &str) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        [(axum::http::header::RETRY_AFTER, "2")],
        axum::Json(serde_json::json!({"status": kind, "detail": detail})),
    )
        .into_response()
}

/// Стан boot-фасаду: HTTP доступний до готовності справжнього роутера.
struct GateState {
    router: Option<axum::Router>,
    finished: bool,
    note: String,
}

/// Boot-gate фасаду: приймає HTTP-з'єднання з першої секунди, віддає 503 на
/// проби готовності, тримає решту запитів до публікації роутера, після чого
/// делегує їх справжньому роутеру. Володіє guard-ом embedded PG (Drop →
/// pg_ctl stop), тому PG живе, поки живе фасад.
pub struct FacadeGate {
    state: std::sync::Mutex<GateState>,
    ready_tx: tokio::sync::watch::Sender<bool>,
    ready_rx: tokio::sync::watch::Receiver<bool>,
    pg: std::sync::Mutex<Option<torgashka_infrastructure::embedded_pg::EmbeddedPostgres>>,
}

impl Default for FacadeGate {
    fn default() -> Self {
        Self::new()
    }
}

impl FacadeGate {
    pub fn new() -> Self {
        let (ready_tx, ready_rx) = tokio::sync::watch::channel(false);
        Self {
            state: std::sync::Mutex::new(GateState {
                router: None,
                finished: false,
                note: String::new(),
            }),
            ready_tx,
            ready_rx,
            pg: std::sync::Mutex::new(None),
        }
    }

    fn is_ready(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.router.is_some())
            .unwrap_or(false)
    }

    fn note(&self) -> String {
        self.state
            .lock()
            .map(|s| s.note.clone())
            .unwrap_or_else(|_| "стан фасаду недоступний".to_string())
    }

    /// Публікація справжнього роутера (ініціалізація завершилась успішно).
    fn publish(&self, router: axum::Router) {
        if let Ok(mut s) = self.state.lock() {
            s.router = Some(router);
            s.finished = true;
            s.note.clear();
        }
        let _ = self.ready_tx.send(true);
    }

    /// Ініціалізація не дала роутера — фасад відповідає 503 із причиною.
    fn fail(&self, note: String) {
        if let Ok(mut s) = self.state.lock() {
            s.finished = true;
            s.note = note;
        }
        let _ = self.ready_tx.send(true);
    }

    /// Тримає embedded PG живим стільки, скільки живе фасад.
    fn hold_pg(&self, pg: Option<torgashka_infrastructure::embedded_pg::EmbeddedPostgres>) {
        if let Ok(mut g) = self.pg.lock() {
            *g = pg;
        }
    }

    /// Ініціалізація ядра у ФОНІ (її ніхто не чекає на шляху HTTP).
    async fn run_init(&self) {
        match init_facade_state().await {
            Ok((state, pg)) => {
                self.hold_pg(pg);
                self.publish(router_v1::build_router(state));
                torgashka_infrastructure::embedded_pg::pg_log(
                    "INFO",
                    "ініціалізацію ядра завершено — фасад переведено на повний роутер",
                );
            }
            Err(e) => {
                let note = format!("ініціалізацію ядра не завершено: {e}");
                torgashka_infrastructure::embedded_pg::pg_log("ERROR", &note);
                eprintln!("[torgashka-api] {note}");
                self.fail(note);
            }
        }
    }

    async fn wait_ready(&self) {
        // Обмеження зверху: навіть якщо ініціалізація застрягла, запит не висить
        // безмежно (дефект 5: «жоден виклик не має права висіти безмежно»).
        const MAX_WAIT: Duration = Duration::from_secs(180);
        let mut rx = self.ready_rx.clone();
        if *rx.borrow_and_update() {
            return;
        }
        let _ = tokio::time::timeout(MAX_WAIT, async {
            loop {
                if *rx.borrow_and_update() {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await;
    }

    /// Обробка HTTP-запиту: 503 на проби готовності під час старту, делегування
    /// справжньому роутеру після публікації, 503 із причиною — якщо ініціалізація
    /// впала (жодних таймаутів і «мертвих» з'єднань).
    async fn handle(&self, req: axum::extract::Request) -> axum::response::Response {
        let path = req.uri().path().to_string();
        if !self.is_ready() && is_readiness_path(&path) {
            return not_ready_response("starting", "ініціалізація ядра триває");
        }
        self.wait_ready().await;
        let router = self.state.lock().ok().and_then(|s| s.router.clone());
        match router {
            Some(r) => {
                use tower::ServiceExt;
                r.oneshot(req).await.unwrap_or_else(|e| match e {})
            }
            None => not_ready_response(
                "db_unavailable",
                &format!("роутер недоступний: {}", self.note()),
            ),
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
/// ЕТАП 18: підключення локального standby-стану (репліка 127.0.0.1:local_port).
///
/// Викликається при старті фасаду. У режимі `Primary` (або коли репліка ще не
/// provisioned / PG не запущено) повертає `None` — локальні маршрути
/// `/api/v1/local/*` не монтуються, поведінка фасаду не змінюється.
async fn init_local_standby(
    cfg: &torgashka_infrastructure::node_config::NodeConfig,
) -> Option<crate::route_local::LocalApiState> {
    use sqlx::postgres::PgPoolOptions;
    use torgashka_infrastructure::{
        repositories::{directories::SqlxDirectories, pos::SqlxPos, write::SqlxWriteDirectories},
        store_ctx::StorePool,
    };
    if !cfg.is_standby() {
        return None;
    }
    // URL локальної репліки: ім'я БД — з [node] primary_db_url (host:port →
    // 127.0.0.1:local_port, пароль відкидається) або з TORGASHKA_PG_DB.
    // Жодних здогадок: немає імені БД → ERROR у torgashka.log і локальні роути
    // НЕ монтуються (раніше підставлялась неіснуюча БД «torgashka»).
    let url = standby_url_for(cfg)?;
    let pool = match PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            torgashka_infrastructure::embedded_pg::pg_log(
                "ERROR",
                &format!(
                    "standby: локальна репліка 127.0.0.1:{} недоступна ({e}) — локальний режим вимкнено (запустіть embedded PG / standby_provision)",
                    cfg.local_port
                ),
            );
            return None;
        }
    };
    let sp = StorePool::new(pool);
    torgashka_infrastructure::embedded_pg::pg_log(
        "INFO",
        &format!(
            "standby: локальна репліка 127.0.0.1:{} підключена — /api/v1/local/* активні",
            cfg.local_port
        ),
    );
    // ADR-0007 F3/F5: пул ЗАПИСУ в primary. `resolve_upstream_write_url`
    // НЕ падає на активне джерело — на standby воно вказує на локальну
    // репліку, а репліка ціллю запису не є ніколи. Немає URL/недосяжний
    // → None: адмін-записи віддадуть 503 (F4).
    let upstream_pool = match cfg.resolve_upstream_write_url() {
        Some(url) => match torgashka_infrastructure::db::connect_upstream_write_pool(&url, 5).await
        {
            Ok(p) => Some(p),
            Err(e) => {
                torgashka_infrastructure::embedded_pg::pg_log(
                    "ERROR",
                    &format!(
                        "standby: upstream_write_url недосяжний ({e}) — адмін-записи → 503 (F4 ADR-0007)"
                    ),
                );
                None
            }
        },
        None => {
            torgashka_infrastructure::embedded_pg::pg_log(
                "INFO",
                "standby: upstream_write_url не задано — адмін-записи → 503 (F4 ADR-0007)",
            );
            None
        }
    };
    Some(crate::route_local::LocalApiState {
        cfg: cfg.clone(),
        pool: sp.clone(),
        upstream_pool,
        readdirs: Arc::new(SqlxDirectories::new(sp.clone()))
            as Arc<dyn ReadDirectories + Send + Sync>,
        // Той самий інваріант, що в init_readdirs: на standby жоден
        // POS-хендл не пише в репліку (тут — лише читання, але адаптер
        // єдиний для вузла).
        pos: Arc::new(OutboxPos::new(Arc::new(SqlxPos::new(sp.clone()))))
            as Arc<dyn PosService + Send + Sync>,
        write: Arc::new(SqlxWriteDirectories::new(sp)) as Arc<dyn WriteDirectories + Send + Sync>,
    })
}

pub async fn serve(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_listener(listener).await
}

/// P3 (ADR-0007 §3.2 #21): фоновий `network_nodes` offline-job має сенс
/// ЛИШЕ на primary — там сторінка моніторингу мережі. На standby він
/// вимкнений (DISABLED_ON_STANDBY). Чиста умова — тестується без БД.
fn offline_job_enabled(cfg: &torgashka_infrastructure::node_config::NodeConfig) -> bool {
    !cfg.is_standby()
}

/// Ініціалізація ядра фасаду — виконується у ФОНІ (у власному таску), тому
/// жоден крок не блокує HTTP-обслуговування.
///
/// ФІКС ДЕФЕКТУ 5а: усі блокуючі PG-кроки (bootstrap, старт репліки) — через
/// `tokio::task::spawn_blocking`; sync-subprocess у async-потоці заборонений.
/// ФІКС ДЕФЕКТУ 6: у standby-режимі `bootstrap_if_needed()` НЕ викликається
/// (initdb/CREATE DATABASE на каталозі репліки заборонені) — лише
/// `ensure_local_replica_running()` і `DATABASE_URL` на локальну репліку.
/// ФІКС ДЕФЕКТУ 5в: `Error::Skipped` = «БД зовнішня» — не помилка.
async fn init_facade_state() -> Result<
    (
        AppState,
        Option<torgashka_infrastructure::embedded_pg::EmbeddedPostgres>,
    ),
    Box<dyn std::error::Error>,
> {
    // Етап 8 — повна дезактивація Python sidecar: Rust-ядро за замовчуванням.
    // Env-флаги можна явно перевизначити (напр. TORGASHKA_RUST_PRRO=0) для тестів.
    for (flag, val) in DEFAULT_RUST_FLAGS {
        if std::env::var_os(flag).is_none() {
            std::env::set_var(flag, val);
        }
    }
    // ── Крок 1: резолв DATABASE_URL (env → db_sources.toml → backend/.env) ──
    let t1 = Instant::now();
    let resolved = torgashka_infrastructure::db::resolve_database_url();
    match &resolved {
        Ok(url) => torgashka_infrastructure::embedded_pg::pg_log(
            "INFO",
            &format!(
                "крок 1: resolve_database_url: Ok ({url}) — embedded PG пропускаємо ({} мс)",
                t1.elapsed().as_millis()
            ),
        ),
        Err(_) => torgashka_infrastructure::embedded_pg::pg_log(
            "INFO",
            &format!(
                "крок 1: resolve_database_url: Err — визначаємо режим вузла ({} мс)",
                t1.elapsed().as_millis()
            ),
        ),
    }
    // ── Крок 2: режим вузла → план підготовки БД (дефекти 5+6) ──
    let node_cfg = torgashka_infrastructure::node_config::NodeConfig::load();
    let plan = plan_db_startup(
        resolved.as_ref().ok().map(String::as_str),
        node_cfg.is_standby(),
        standby_url_for(&node_cfg),
    );
    torgashka_infrastructure::embedded_pg::pg_log(
        "INFO",
        &format!(
            "крок 2: режим вузла = {}; план = {plan:?}",
            if node_cfg.is_standby() {
                "standby"
            } else {
                "primary"
            }
        ),
    );
    let embedded_pg = apply_db_startup_plan(plan).await;
    // ── Крок 3: авто-міграції схеми ──
    let t3 = Instant::now();
    // Авто-міграції (Частина 1.2): застосувати схему на fresh-БД ПЕРЕД
    // підняттям listener. Ідемпотентно: повна схема лише якщо users немає;
    // owners_db створюється завжди (CREATE TABLE IF NOT EXISTS).
    match torgashka_infrastructure::db::connect_readonly_pool(5).await {
        Ok(pool) => {
            if let Err(e) = torgashka_infrastructure::db::ensure_schema(&pool).await {
                torgashka_infrastructure::embedded_pg::pg_log(
                    "ERROR",
                    &format!("крок 3: авто-міграція схеми НЕ виконана — {e}"),
                );
            }
            pool.close().await;
        }
        Err(e) => {
            // Провал створення пула: причина мусить бути у файлі (Windows: stderr приховано).
            torgashka_infrastructure::embedded_pg::pg_log(
                "ERROR",
                &format!("крок 3: БД недоступна для авто-міграції — {e}"),
            );
        }
    }
    torgashka_infrastructure::embedded_pg::pg_log(
        "INFO",
        &format!(
            "крок 3: авто-міграції — завершено ({} мс)",
            t3.elapsed().as_millis()
        ),
    );
    // ── Крок 4: репозиторії/сервіси (кожен пул створюється один раз) ──
    let t4 = Instant::now();
    let (readdirs, write, write_pool, pos, ledger, auth) = match init_readdirs().await {
        Ok((pool, read, write, pos, ledger, auth)) => (
            Some(read),
            Some(write),
            Some(pool),
            Some(pos),
            Some(ledger),
            Some(auth),
        ),
        Err(reason) => {
            // Перелік того, що НЕ змонтовано — щоб причина 503-ї була видима
            // в torgashka.log без здогадок (дефект 2026-09).
            torgashka_infrastructure::embedded_pg::pg_log(
                "ERROR",
                &format!(
                    "крок 4: НЕ змонтовано readdirs/write/pos/ledger/auth/setup \
                     (/api/v1/setup/status = 503, логін-гейт каси висне) — {reason}"
                ),
            );
            (None, None, None, None, None, None)
        }
    };
    // Окремий флаг auth: TORGASHKA_RUST_AUTH=1 вмикає Rust-гілку auth навіть якщо
    // readdirs вимкнено (проксі-режим для решти) — але пул створюється спільно.
    let auth = if env_flag(RUST_AUTH_ENV) && auth.is_none() {
        match torgashka_infrastructure::db::connect_readonly_pool(10).await {
            Ok(pool) => {
                eprintln!(
                    "[torgashka-api] {RUST_AUTH_ENV}=1 — Rust-гілка auth увімкнена (PostgreSQL)"
                );
                // ADR-0007 F6: той самий гейт, що й у readdirs-гілці.
                let node_cfg = torgashka_infrastructure::node_config::NodeConfig::load();
                Some(Arc::new(
                    torgashka_infrastructure::repositories::auth::SqlxAuth::with_standby(
                        StorePool::new(pool),
                        node_cfg.is_standby(),
                    ),
                ) as Arc<dyn AuthService + Send + Sync>)
            }
            Err(e) => {
                torgashka_infrastructure::embedded_pg::pg_log(
                    "ERROR",
                    &format!(
                        "{RUST_AUTH_ENV}=1, але пул для auth НЕ створено ({e}) — auth через роути не змонтовано (LEGACY → 410)"
                    ),
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
    // ЕТАП 18: режим вузла (Primary|Standby) + підключення локальної репліки.
    let node_config = torgashka_infrastructure::node_config::NodeConfig::load();
    let local = init_local_standby(&node_config).await;
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
        node_config,
        local,
    };
    // Фоновий job мережі магазинів (ЕТАП 15, план §5.4): кожні 60 с вузли
    // active/lagging без heartbeat > 5 хв → offline (той самий поріг, що й
    // isDeviceOnline кас). Вузли archived не чіпаємо (термінальний стан).
    // ADR-0007 §3.2 #21 = DISABLED_ON_STANDBY: це housekeeping реєстру
    // вузлів (роль primary); на standby job крутився б проти read-only
    // репліки → спам `read-only transaction` у postgres.log.
    if !offline_job_enabled(&state.node_config) {
        eprintln!("[torgashka-api] network_nodes offline-job вимкнено (режим standby)");
    } else if let Some(pool) = state.write_pool.clone() {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                tick.tick().await;
                let res = sqlx::query(
                    "UPDATE network_nodes \
                     SET status = 'offline'::public.node_status, updated_at = now() \
                     WHERE status IN ('active', 'lagging') \
                       AND last_seen_at < now() - interval '5 minutes'",
                )
                .execute(&pool)
                .await;
                if let Err(e) = res {
                    // Помилка може бути тимчасовою (БД недоступна) — логуємо
                    // і продовжуємо наступний тик, а не виходимо.
                    eprintln!("[torgashka-api] network_nodes offline-job: {e}");
                }
            }
        });
    }

    torgashka_infrastructure::embedded_pg::pg_log(
        "INFO",
        &format!(
            "крок 4: репозиторії/сервіси готові ({} мс); ініціалізація ядра завершена",
            t4.elapsed().as_millis()
        ),
    );
    Ok((state, embedded_pg))
}

/// Ідемпотентний старт локальної репліки з логом результату (дефект 5а:
/// блокуючий PG-старт — тільки через `spawn_blocking`). Спільний для обох
/// standby-гілок плану.
async fn start_local_replica_logged() {
    let t = Instant::now();
    match tokio::task::spawn_blocking(
        torgashka_infrastructure::embedded_pg::ensure_local_replica_running,
    )
    .await
    {
        Ok(Ok(true)) => torgashka_infrastructure::embedded_pg::pg_log(
            "INFO",
            &format!(
                "крок 2 (standby): локальну репліку піднято ({} мс)",
                t.elapsed().as_millis()
            ),
        ),
        Ok(Ok(false)) => torgashka_infrastructure::embedded_pg::pg_log(
            "INFO",
            &format!(
                "крок 2 (standby): локальна репліка — no-op (уже слухає / ще не провіжнена) ({} мс)",
                t.elapsed().as_millis()
            ),
        ),
        // Чесна причина і робота далі: фасад відповідає, локальний режим
        // увімкнеться, щойно репліка стане доступною.
        Ok(Err(e)) => torgashka_infrastructure::embedded_pg::pg_log(
            "ERROR",
            &format!(
                "крок 2 (standby): локальну репліку НЕ піднято ({e}) — працюємо далі ({} мс)",
                t.elapsed().as_millis()
            ),
        ),
        Err(e) => torgashka_infrastructure::embedded_pg::pg_log(
            "ERROR",
            &format!("крок 2 (standby): таск старту репліки панікував ({e})"),
        ),
    }
}

/// Проба ЖИВОЇ репліки: підключення БЕЗ пароля до БД `postgres` на
/// `127.0.0.1:{local_port}` і перелік не-шаблонних БД (детермінований порядок).
///
/// НЕ панікує і НЕ `unwrap`-ить: будь-яка помилка (драйвер/connect/запит/
/// таймаут 5 с) → [`ReplicaDbProbe::Err`] із текстом причини.
async fn probe_live_replica(local_port: u16, user: &str) -> ReplicaDbProbe {
    use sqlx::postgres::PgPoolOptions;
    let dsn = format!("postgresql://{user}@127.0.0.1:{local_port}/postgres");
    let pool = match PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&dsn)
        .await
    {
        Ok(p) => p,
        Err(e) => return ReplicaDbProbe::Err(format!("підключення до {dsn}: {e}")),
    };
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT datname FROM pg_database \
         WHERE NOT datistemplate AND datname <> 'postgres' ORDER BY datname",
    )
    .fetch_all(&pool)
    .await;
    pool.close().await;
    match rows {
        Ok(names) => match classify_replica_probe(&names) {
            Ok(name) => ReplicaDbProbe::Ok(name),
            Err(e) => ReplicaDbProbe::Err(e),
        },
        Err(e) => ReplicaDbProbe::Err(format!("запит pg_database (порт {local_port}): {e}")),
    }
}

/// Перевірка «пул реально піднявся»: `SELECT 1` на candidate-URL.
/// `Err(text)` — з'єднання/запит не вдались (жодного `set_var` тоді не буде).
async fn candidate_url_alive(url: &str) -> Result<(), String> {
    use sqlx::postgres::PgPoolOptions;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await
        .map_err(|e| format!("підключення до candidate-URL {url}: {e}"))?;
    let probe = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&pool)
        .await;
    pool.close().await;
    probe
        .map(|_| ())
        .map_err(|e| format!("SELECT 1 на candidate-URL {url}: {e}"))
}

/// Самолікування standby-каси: визначити ім'я локальної БД із (c)/(d), ПЕРЕВІРИТИ
/// живість URL, і лише тоді `DATABASE_URL` + запис `[node] primary_db_url`.
///
/// Повертає `None` завжди (standby-реплікою фасад не володіє — Drop її не
/// зупиняє). Будь-яка невдача → ERROR у `torgashka.log` із ПЕРЕЛІКОМ джерел;
/// без `set_var`, без запису у файл (регресії для primary немає — ця функція
/// викликається лише з гілки `StandbyWithoutDbName`).
async fn self_heal_standby_db_name(
    reason: &str,
) -> Option<torgashka_infrastructure::embedded_pg::EmbeddedPostgres> {
    use torgashka_infrastructure::embedded_pg::pg_log;
    let user = std::env::var("TORGASHKA_PG_USER").unwrap_or_else(|_| "postgres".to_string());
    // cfg: тонка impure-обгортка (env → файл) — тут потрібен local_port + host/port.
    let cfg = torgashka_infrastructure::node_config::NodeConfig::load();
    let settings = torgashka_infrastructure::standby_heartbeat::read_standby_settings()
        .ok()
        .flatten();
    let settings_db = settings
        .as_ref()
        .and_then(|s| s.replication_database.as_deref())
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    // (d) пробу виконуємо ЛИШЕ якщо (c) не дало імені — зайвих з'єднань немає.
    let probe = if settings_db.is_some() {
        ReplicaDbProbe::NotAttempted
    } else {
        probe_live_replica(cfg.local_port, &user).await
    };
    if let ReplicaDbProbe::Err(e) = &probe {
        pg_log(
            "INFO",
            &format!("крок 2 (standby): проба живої репліки не дала імені — {e}"),
        );
    }
    let primary = cfg.resolve_primary_db_url();
    let env_db = std::env::var("TORGASHKA_PG_DB").ok();
    match standby_local_url_wide(
        primary.as_deref(),
        cfg.local_port,
        &user,
        env_db.as_deref(),
        settings_db.as_deref(),
        Some(&probe),
    ) {
        Ok((url, src)) => {
            // Пул мусить РЕАЛЬНО піднятись — інакше не чіпаємо ні env, ні файл.
            if let Err(e) = candidate_url_alive(&url).await {
                pg_log(
                    "ERROR",
                    &format!(
                        "крок 2 (standby): ім'я БД знайдено ({}, джерело: {}), але з'єднання \
                         НЕ піднялось — DATABASE_URL не встановлено, файл НЕ змінено ({e}); \
                         підстава: {reason}",
                        url,
                        src.as_str()
                    ),
                );
                return None;
            }
            std::env::set_var("DATABASE_URL", &url);
            pg_log(
                "INFO",
                &format!(
                    "крок 2 (standby): DATABASE_URL = {url} (джерело: {}) — пули/роути \
                     змонтуються, /api/v1/setup/status відповідатиме",
                    src.as_str()
                ),
            );
            // Самолікування [node] primary_db_url: лише для (c)/(d) — (a)/(b)
            // не потребують запису (ім'я вже є у файлі/env).
            if matches!(
                src,
                LocalDbNameSource::NodeSettings | LocalDbNameSource::ReplicaProbe
            ) {
                // Ім'я БД беремо з URL, який щойно перевірили (fallback_local_url:
                // postgresql://user@127.0.0.1:port/<db>).
                let db = db_name_from_url(&url).unwrap_or_else(|| url.clone());
                let host = settings
                    .as_ref()
                    .and_then(|s| s.replication_host.clone())
                    .map(|h| h.trim().to_string())
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| "127.0.0.1".to_string());
                let port = settings
                    .as_ref()
                    .and_then(|s| s.replication_port)
                    .unwrap_or(cfg.local_port);
                let healed = torgashka_infrastructure::node_config::primary_db_url_from_parts(
                    &user, &host, port, &db,
                );
                let healed_cfg = torgashka_infrastructure::node_config::NodeConfig {
                    mode: torgashka_infrastructure::node_config::NodeMode::Standby,
                    local_port: cfg.local_port,
                    primary_db_url: Some(healed.clone()),
                    // Решта полів — з реально завантаженого конфіга: `..default()`
                    // затирав `degrade_to_local`/`repoint_pending` дефолтами.
                    ..cfg.clone()
                };
                match healed_cfg.save_to_disk() {
                    Ok(path) => pg_log(
                        "INFO",
                        &format!(
                            "крок 2 (standby): самолікування [node] primary_db_url = {healed} \
                             (mode=standby, local_port={}) записано у {}",
                            cfg.local_port,
                            path.display()
                        ),
                    ),
                    Err(e) => pg_log(
                        "ERROR",
                        &format!(
                            "крок 2 (standby): самолікування [node] primary_db_url НЕ записано ({e}); \
                             робота продовжується з DATABASE_URL = {url}"
                        ),
                    ),
                }
            }
            None
        }
        Err(reason_full) => {
            pg_log(
                "ERROR",
                &format!(
                    "крок 2 (standby): DATABASE_URL НЕ встановлено — {reason_full}; \
                     локальні пули (readdirs/write/pos/ledger/auth) і /api/v1/setup/status \
                     НЕ монтуються; здогадок імені БД немає; підстава: {reason}"
                ),
            );
            None
        }
    }
}

/// Виконує план підготовки БД (ФІКС дефектів 5+6).
///
/// * `ExternalUrl` — БД ззовні: embedded PG не чіпаємо;
/// * `StandbyReplica` — ЛИШЕ ідемпотентний старт локальної репліки +
///   `DATABASE_URL` на неї (жодного initdb/CREATE DATABASE — каталог репліки
///   read-only і належить pg_basebackup);
/// * `PrimaryBootstrap` — повний bootstrap, але у `spawn_blocking`.
pub async fn apply_db_startup_plan(
    plan: DbStartupPlan,
) -> Option<torgashka_infrastructure::embedded_pg::EmbeddedPostgres> {
    use torgashka_infrastructure::embedded_pg::bootstrap_if_needed;
    match plan {
        DbStartupPlan::ExternalUrl(url) => {
            torgashka_infrastructure::embedded_pg::pg_log(
                "INFO",
                &format!("крок 2: DATABASE_URL ззовні ({url}) — embedded PG не потрібен"),
            );
            None
        }
        DbStartupPlan::StandbyReplica(url) => {
            torgashka_infrastructure::embedded_pg::pg_log(
                "INFO",
                &format!(
                    "крок 2 (standby): bootstrap (initdb/CREATE DATABASE) ПРОПУЩЕНО — \
                     це каталог репліки; ідемпотентно піднімаємо локальну репліку, читаємо {url}"
                ),
            );
            start_local_replica_logged().await;
            // Джерело даних standby-каси — локальна репліка, без пароля.
            std::env::set_var("DATABASE_URL", &url);
            torgashka_infrastructure::embedded_pg::pg_log(
                "INFO",
                &format!("крок 2 (standby): DATABASE_URL = {url} (локальна репліка, лише читання)"),
            );
            // Реплікою НЕ володіємо: Drop фасаду не має її зупиняти.
            None
        }
        DbStartupPlan::StandbyWithoutDbName(reason) => {
            torgashka_infrastructure::embedded_pg::pg_log(
                "INFO",
                "крок 2 (standby): bootstrap (initdb/CREATE DATABASE) ПРОПУЩЕНО — це каталог репліки",
            );
            // Саму репліку піднімаємо (її стан не залежить від імені БД у URL).
            start_local_replica_logged().await;
            // САМОЛІКУВАННЯ ІМЕНІ БД (гілка доступна ЛИШЕ тут — репліку вже
            // піднято): (c) SQLite settings `node_replication_database` →
            // (d) проба живої репліки на 127.0.0.1:<local_port>. Джерела (a)/(b)
            // сюди не дійшли (plan = StandbyWithoutDbName), але лишаються в
            // ланцюзі для повноти переліку в помилці.
            self_heal_standby_db_name(&reason).await
        }
        DbStartupPlan::PrimaryBootstrap => {
            torgashka_infrastructure::embedded_pg::pg_log(
                "INFO",
                "крок 2 (primary): запускаємо embedded PostgreSQL (initdb → старт → CREATE DATABASE)",
            );
            let t = Instant::now();
            // Дефект 5а: bootstrap — блокуючий (subprocess-и) → spawn_blocking.
            match tokio::task::spawn_blocking(bootstrap_if_needed).await {
                Ok(Ok(pg)) => {
                    torgashka_infrastructure::embedded_pg::pg_log(
                        "INFO",
                        &format!(
                            "крок 2 (primary): вбудований PostgreSQL: {} (data_dir: {}, {} мс)",
                            pg.database_url(),
                            pg.data_dir().display(),
                            t.elapsed().as_millis()
                        ),
                    );
                    Some(pg)
                }
                // Дефект 5в: Skipped = DATABASE_URL з'явився ззовні — це не помилка.
                Ok(Err(torgashka_infrastructure::embedded_pg::Error::Skipped(msg))) => {
                    torgashka_infrastructure::embedded_pg::pg_log(
                        "INFO",
                        &format!("крок 2: embedded PG пропущено — {msg} (БД зовнішня)"),
                    );
                    None
                }
                Ok(Err(e)) => {
                    torgashka_infrastructure::embedded_pg::pg_log(
                        "ERROR",
                        &format!(
                            "крок 2 (primary): вбудований PostgreSQL недоступний ({e}); працюємо без БД ({} мс)",
                            t.elapsed().as_millis()
                        ),
                    );
                    None
                }
                Err(e) => {
                    torgashka_infrastructure::embedded_pg::pg_log(
                        "ERROR",
                        &format!(
                            "крок 2 (primary): таск bootstrap панікував ({e}); працюємо без БД"
                        ),
                    );
                    None
                }
            }
        }
    }
}

/// Запустити фасад на вже прив'язаному слухачі.
///
/// Викликається з `lib.rs` застосунку: бінд виконується СИНХРОННО до створення
/// вікна, щоб порт :8000 був зайнятий ще до завантаження webview — інакше
/// фронтенд ловить ECONNREFUSED під час ініціалізації (гонка при старті).
///
/// ФІКС ДЕФЕКТУ 5: HTTP-обслуговування стартує ЗРАЗУ (boot-gate), а ініціалізація
/// БД іде у фоні. Раніше синхронний bootstrap тримав слухач «німим»: TCP-handshake
/// завершувався, а HTTP-відповіді не було (curl: 0 байт + таймаут).
pub async fn serve_listener(
    listener: tokio::net::TcpListener,
) -> Result<(), Box<dyn std::error::Error>> {
    // ── Діагностичний лог: безумовно — torgashka.log має з'являтися завжди ──
    // (stderr на Windows приховано windows_subsystem=windows — це єдиний канал)
    torgashka_infrastructure::embedded_pg::pg_log("INFO", "serve_listener: старт");
    let gate = Arc::new(FacadeGate::new());
    let addr = listener.local_addr()?;
    eprintln!("[torgashka-api] фасад слухає http://{addr} (ініціалізація БД — у фоні)");
    torgashka_infrastructure::embedded_pg::pg_log(
        "INFO",
        &format!("фасад слухає http://{addr} — HTTP-відповіді з першої секунди (boot-gate)"),
    );
    // Фонова ініціалізація ядра: жоден блокуючий крок не виконується на шляху HTTP.
    let init_gate = gate.clone();
    tokio::spawn(async move { init_gate.run_init().await });
    // HTTP-обслуговування з першої секунди.
    let app =
        axum::Router::new().fallback(axum::routing::any(move |req: axum::extract::Request| {
            let gate = gate.clone();
            async move { gate.handle(req).await }
        }));
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Дефект 6: standby НІКОЛИ не йде primary-шляхом (initdb/CREATE DATABASE) ──

    #[test]
    fn standby_without_external_url_never_bootstraps() {
        let plan = plan_db_startup(
            None,
            true,
            Some("postgresql://repuser@127.0.0.1:5433/repdb".to_string()),
        );
        assert_eq!(
            plan,
            DbStartupPlan::StandbyReplica("postgresql://repuser@127.0.0.1:5433/repdb".to_string()),
            "standby-вузол мусить іти гілкою репліки, а не bootstrap (дефект 6)"
        );
        assert_ne!(plan, DbStartupPlan::PrimaryBootstrap);
    }

    #[test]
    fn primary_without_external_url_bootstraps() {
        assert_eq!(
            plan_db_startup(None, false, None),
            DbStartupPlan::PrimaryBootstrap
        );
    }

    #[test]
    fn external_url_always_wins_and_skips_embedded_pg() {
        // env DATABASE_URL задано → embedded PG не чіпаємо (ні primary, ні standby)
        assert_eq!(
            plan_db_startup(Some("postgresql://u@h:5432/db"), false, None),
            DbStartupPlan::ExternalUrl("postgresql://u@h:5432/db".to_string())
        );
        assert_eq!(
            plan_db_startup(
                Some("postgresql://u@h:5432/db"),
                true,
                Some("x".to_string())
            ),
            DbStartupPlan::ExternalUrl("postgresql://u@h:5432/db".to_string())
        );
        // порожній/пробільний URL не вважається заданим
        assert_eq!(
            plan_db_startup(Some("   "), false, None),
            DbStartupPlan::PrimaryBootstrap
        );
    }

    #[test]
    fn standby_url_from_primary_is_passwordless_and_local() {
        let url = standby_local_url_or_err(
            Some("postgresql://repuser:s3cret@10.0.0.5:5432/pos_net"),
            5433,
            "repuser",
            None,
        )
        .expect("URL локальної репліки");
        assert_eq!(url, "postgresql://repuser@127.0.0.1:5433/pos_net");
        assert!(!url.contains("s3cret"), "{url}");
        // userinfo без ':' → пароля немає
        let userinfo = &url[url.find("://").unwrap() + 3..url.rfind('@').unwrap()];
        assert!(!userinfo.contains(':'), "пароль у URL заборонений: {url}");
    }

    /// Критерій контракту: ім'я БД primary доживає до локального URL
    /// (userinfo збережено, host:port → 127.0.0.1:local_port, БД збережено).
    #[test]
    fn standby_local_url_keeps_primary_db_name() {
        assert_eq!(
            standby_local_url_or_err(
                Some("postgresql://postgres@192.0.2.10:5432/pos_system_fresh"),
                5433,
                "postgres",
                None,
            )
            .expect("URL локальної репліки"),
            "postgresql://postgres@127.0.0.1:5433/pos_system_fresh"
        );
    }

    // ── Регресія 2026-09: жодних здогадок імені БД «torgashka» ───────────────

    #[test]
    fn standby_url_without_db_name_is_error_and_is_logged() {
        let mut records: Vec<(String, String)> = Vec::new();
        let out = standby_url_with_logger(None, 5433, "postgres", None, |level, msg| {
            records.push((level.to_string(), msg.to_string()));
        });
        assert!(out.is_none(), "без імені БД пул НЕ створюється");
        assert_eq!(records.len(), 1, "рівно один запис у лог");
        assert_eq!(records[0].0, "ERROR", "рівень запису — ERROR");
        assert!(
            records[0].1.contains("локальна репліка недоступна"),
            "текст: {}",
            records[0].1
        );
        assert!(
            records[0].1.contains("primary_db_url") && records[0].1.contains("TORGASHKA_PG_DB"),
            "текст мусить називати обидва джерела: {}",
            records[0].1
        );

        // primary_db_url Є, але без імені БД → теж ERROR, без здогадки.
        let mut records2: Vec<(String, String)> = Vec::new();
        let out2 = standby_url_with_logger(
            Some("postgresql://postgres@192.0.2.10:5432"),
            5433,
            "postgres",
            None,
            |level, msg| records2.push((level.to_string(), msg.to_string())),
        );
        assert!(out2.is_none(), "URL без імені БД → пул НЕ створюється");
        assert_eq!(records2.len(), 1);
        assert_eq!(records2[0].0, "ERROR");
    }

    /// Який би шлях не резолвився — «/torgashka» у виводі НЕ з'являється.
    #[test]
    fn no_url_ever_ends_with_guessed_torgashka() {
        let candidates = [
            standby_local_url_or_err(
                Some("postgresql://postgres@192.0.2.10:5432/pos_system_fresh"),
                5433,
                "postgres",
                None,
            ),
            standby_local_url_or_err(None, 5433, "postgres", Some("pos_system_fresh")),
            standby_local_url_or_err(
                Some("postgresql://postgres@192.0.2.10:5432/pos_system_fresh"),
                5433,
                "postgres",
                Some("pos_system_fresh"),
            ),
        ];
        for c in candidates {
            let url = c.expect("URL локальної репліки");
            assert!(!url.ends_with("/torgashka"), "здогадка повернулась: {url}");
            assert!(!url.contains("torgashka"), "здогадка повернулась: {url}");
        }
        // Без жодного джерела імені БД — Err, а не URL.
        assert!(standby_local_url_or_err(None, 5433, "postgres", None).is_err());
        assert!(standby_local_url_or_err(None, 5433, "postgres", Some("   ")).is_err());
    }

    /// План standby-вузла без імені БД не містить жодного URL (і НІКОЛИ не
    /// піде primary-шляхом з initdb на каталозі репліки).
    #[test]
    fn standby_plan_without_db_name_skips_pool() {
        let plan = plan_db_startup(None, true, None);
        assert_ne!(
            plan,
            DbStartupPlan::PrimaryBootstrap,
            "standby НІКОЛИ не bootstrap (дефект 6)"
        );
        assert_ne!(
            plan,
            DbStartupPlan::StandbyReplica(String::new()),
            "порожній DATABASE_URL — стара поведінка, тепер заборонена"
        );
        match plan {
            DbStartupPlan::StandbyWithoutDbName(reason) => {
                assert!(
                    reason.contains("primary_db_url") || reason.contains("TORGASHKA_PG_DB"),
                    "причина мусить називати джерела: {reason}"
                );
            }
            other => panic!("очікували StandbyWithoutDbName, отримали {other:?}"),
        }
    }

    #[test]
    fn db_name_from_url_reads_name_only() {
        assert_eq!(
            db_name_from_url("postgresql://u:p@h:5432/pos_system_fresh"),
            Some("pos_system_fresh".to_string())
        );
        assert_eq!(
            db_name_from_url("postgres://h:5432/db?sslmode=require"),
            Some("db".to_string())
        );
        assert_eq!(db_name_from_url("postgresql://u@h:5432"), None);
        assert_eq!(db_name_from_url("postgresql://u@h:5432/"), None);
        assert_eq!(db_name_from_url("не-url"), None);
    }

    // ── Ланцюг джерел імені БД (a>b>c>d) — самолікування standby-каси ────────

    #[test]
    fn classify_replica_probe_one_ok_zero_and_many_err() {
        // Рівно одна БД (порожні/пробільні відкидаємо) → Ok з trimmed іменем.
        assert_eq!(
            classify_replica_probe(&["  pos_system_fresh  ".to_string()]),
            Ok("pos_system_fresh".to_string())
        );
        // 0 БД → Err зі згадкою «0».
        let e0 = classify_replica_probe(&[]).expect_err("0 баз — це помилка");
        assert!(e0.contains('0'), "текст мусить згадувати 0: {e0}");
        assert_eq!(
            classify_replica_probe(&["   ".to_string()]).expect_err("порожні імена = 0 баз"),
            e0,
            "порожні рядки = ті самі 0 баз"
        );
        // ≥2 БД → Err зі згадкою ОБОХ імен (жодних здогадок).
        let e2 = classify_replica_probe(&["a".to_string(), "b".to_string()])
            .expect_err("2 БД — неоднозначно");
        assert!(e2.contains('a') && e2.contains('b'), "текст: {e2}");
        assert!(e2.contains('2'), "текст мусить згадувати кількість: {e2}");
    }

    #[test]
    fn local_db_name_priority_a_b_c_d() {
        let probe = ReplicaDbProbe::Ok("from_probe".to_string());
        let all = |p: Option<&ReplicaDbProbe>| {
            local_db_name_or_err(
                Some("postgresql://postgres@10.0.0.5:5432/from_primary"),
                Some("from_env"),
                Some("from_settings"),
                p,
            )
        };
        // (a) primary_db_url виграє над усіма.
        assert_eq!(
            all(Some(&probe)),
            Ok(("from_primary".to_string(), LocalDbNameSource::PrimaryUrl))
        );
        // (b) env виграє над (c)/(d).
        assert_eq!(
            local_db_name_or_err(None, Some("from_env"), Some("from_settings"), Some(&probe)),
            Ok(("from_env".to_string(), LocalDbNameSource::EnvDb))
        );
        // (b) порожній/пробільний env не блокує (c).
        assert_eq!(
            local_db_name_or_err(None, Some("   "), Some("from_settings"), Some(&probe)),
            Ok(("from_settings".to_string(), LocalDbNameSource::NodeSettings))
        );
        // (c) settings виграє над пробою (d).
        assert_eq!(
            local_db_name_or_err(None, None, Some("from_settings"), Some(&probe)),
            Ok(("from_settings".to_string(), LocalDbNameSource::NodeSettings))
        );
        // (d) проба — останнє джерело.
        assert_eq!(
            local_db_name_or_err(None, None, None, Some(&probe)),
            Ok(("from_probe".to_string(), LocalDbNameSource::ReplicaProbe))
        );
        // (a) без імені БД (URL лише з host:port) → падаємо далі ланцюгом.
        assert_eq!(
            local_db_name_or_err(
                Some("postgresql://postgres@10.0.0.5:5432"),
                None,
                None,
                Some(&probe)
            ),
            Ok(("from_probe".to_string(), LocalDbNameSource::ReplicaProbe))
        );
        // Проба Err / NotAttempted → Err із переліком УСІХ джерел.
        for p in [
            ReplicaDbProbe::Err("проба: 2 бази [a, b]".to_string()),
            ReplicaDbProbe::NotAttempted,
        ] {
            let e = local_db_name_or_err(None, None, None, Some(&p))
                .expect_err("жодного імені — помилка");
            for needle in [
                "primary_db_url",
                "TORGASHKA_PG_DB",
                "node_replication_database",
                "живої репліки",
            ] {
                assert!(e.contains(needle), "у тексті немає «{needle}»: {e}");
            }
        }
    }

    #[test]
    fn all_sources_empty_err_lists_every_source() {
        let e = local_db_name_or_err(None, None, None, None).expect_err("порожньо — Err");
        assert!(e.contains("primary_db_url"), "{e}");
        assert!(e.contains("TORGASHKA_PG_DB"), "{e}");
        assert!(e.contains("node_replication_database"), "{e}");
        assert!(e.contains("локальна репліка недоступна"), "{e}");
    }

    #[test]
    fn standby_local_url_wide_probe_ok_is_passwordless_local_url() {
        let probe = ReplicaDbProbe::Ok("pos_system_fresh".to_string());
        let (url, src) = standby_local_url_wide(None, 5433, "postgres", None, None, Some(&probe))
            .expect("URL з проби");
        assert_eq!(url, "postgresql://postgres@127.0.0.1:5433/pos_system_fresh");
        assert_eq!(src, LocalDbNameSource::ReplicaProbe);
        assert!(
            !url.contains('@')
                || !url[url.find("://").unwrap() + 3..url.rfind('@').unwrap()].contains(':'),
            "пароль у URL заборонений: {url}"
        );

        // settings (c) → той самий формат URL, джерело NodeSettings.
        let (url_c, src_c) =
            standby_local_url_wide(None, 5432, "postgres", None, Some("pos_system_fresh"), None)
                .expect("URL зі settings");
        assert_eq!(
            url_c,
            "postgresql://postgres@127.0.0.1:5432/pos_system_fresh"
        );
        assert_eq!(src_c, LocalDbNameSource::NodeSettings);

        // (a) і (b) — як раніше (регресія).
        let (url_a, src_a) = standby_local_url_wide(
            Some("postgresql://repuser:s3cret@10.0.0.5:5432/pos_net"),
            5433,
            "repuser",
            Some("ignored"),
            Some("ignored"),
            Some(&probe),
        )
        .expect("URL з primary");
        assert_eq!(url_a, "postgresql://repuser@127.0.0.1:5433/pos_net");
        assert_eq!(src_a, LocalDbNameSource::PrimaryUrl);
        let (url_b, src_b) =
            standby_local_url_wide(None, 5433, "postgres", Some("envdb"), None, None)
                .expect("URL з env");
        assert_eq!(url_b, "postgresql://postgres@127.0.0.1:5433/envdb");
        assert_eq!(src_b, LocalDbNameSource::EnvDb);
    }

    #[test]
    fn standby_local_url_wide_probe_err_mentions_reason_and_all_sources() {
        let probe = ReplicaDbProbe::Err("знайдено 2 баз [a, b]".to_string());
        let e = standby_local_url_wide(None, 5433, "postgres", None, None, Some(&probe))
            .expect_err("жодного імені — Err");
        assert!(e.contains("знайдено 2 баз [a, b]"), "причина проби: {e}");
        for needle in [
            "primary_db_url",
            "TORGASHKA_PG_DB",
            "node_replication_database",
        ] {
            assert!(e.contains(needle), "у тексті немає «{needle}»: {e}");
        }
        // NotAttempted (пробу не виконували) — теж Err, а не URL.
        assert!(standby_local_url_wide(
            None,
            5433,
            "postgres",
            None,
            None,
            Some(&ReplicaDbProbe::NotAttempted)
        )
        .is_err());
    }

    #[test]
    fn readiness_paths_are_probe_endpoints_only() {
        assert!(is_readiness_path("/api/v1/health"));
        assert!(is_readiness_path("/api/v1/setup/status"));
        assert!(!is_readiness_path("/api/v1/auth/login"));
        assert!(!is_readiness_path("/api/v1/setup"));
    }
    // ── P3 (ADR-0007 #21): offline-job мережі вузлів — primary-only ──

    #[test]
    fn offline_job_disabled_on_standby_enabled_on_primary() {
        use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
        let standby = NodeConfig::load_from_str("[node]\nmode = \"standby\"\n");
        assert!(!offline_job_enabled(&standby), "standby → job вимкнено");
        let primary = NodeConfig::load_from_str("[node]\nmode = \"primary\"\n");
        assert!(offline_job_enabled(&primary), "primary → job працює (F2)");
        assert!(
            offline_job_enabled(&NodeConfig::default()),
            "без [node] → Primary → поведінка незмінна (F2)"
        );
        let standby = NodeConfig {
            mode: NodeMode::Standby,
            ..Default::default()
        };
        assert!(!offline_job_enabled(&standby));
    }
}
