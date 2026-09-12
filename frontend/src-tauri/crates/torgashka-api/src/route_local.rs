//! Локальний routing-шар вузла (локальна копія БД; ЕТАП 18, оновлено E7).
//!
//! Коли primary мережі магазинів недоступний, вузол (каса) деградує в
//! **локальний режим**:
//!
//! - **читання** (категорії, продукти, залишки, чеки — GET) виконуються
//!   проти ЛОКАЛЬНОЇ копії БД (порт `node_config::local_port`, зазвичай 5433)
//!   через ТІ САМІ
//!   domain-сервіси/репозиторії, що й primary-гілка — формат відповідей
//!   ідентичний основному API;
//! - **запис** (нові чеки/POS-операції — POST) потрапляє у SQLite-чергу
//!   (`torgashka_infrastructure::offline`, ЕТАП 3-5) — атомарно з outbox,
//!   звідки фоновий/ручний push (`/api/v1/local/sync/now`) відправляє їх на
//!   primary, щойно мережа відновлюється.
//!
//! Маршрути змонтовані під `/api/v1/local/*` і проходять ТІ САМІ auth + store
//! middleware (JWT + X-Store-Id + RLS), що й основний API (вбудовано в
//! `router_v1::build_router` перед CORS-шаром). Рішення «йти в локальний
//! режим» приймає клієнт каси на основі `/api/v1/local/status`
//! (`primary_reachable`), а фасад надає статус вузла для heartbeat
//! (`effective_status`: active/lagging/offline — ЕТАП 18.D).

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Timelike;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use torgashka_domain::{
    CategoryDto, InventoryCountsDto, Page, PosError, PosService, ProductDto, ProductFilters,
    ReadDirectories, ReceiptSearchDto, ReceiptSearchQuery, ReceiptStatsDto, WriteDirectories,
    WriteError,
};
use torgashka_domain::{InvoicesV1Service, ReturnInvoicesService};
use torgashka_infrastructure::{
    node_config::{self, NodeConfig},
    offline,
    repositories::{invoices::SqlxInvoices, pos::SqlxPos, return_invoices::SqlxReturnInvoices},
    store_ctx::{current_store_ctx, with_store_ctx, StoreCtx, StorePool},
};
use uuid::Uuid;

use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Стан локального режиму
// ─────────────────────────────────────────────────────────────────────────────

/// Сервіси локального режиму: репозиторії над пулом ЛОКАЛЬНОЇ репліки.
///
/// Створюється фасадом ПРИ СТАРТІ, лише коли локальна копія БД
/// (127.0.0.1:`node_config::local_port`) приймає з'єднання. Інакше
/// `AppState.local = None` — локальні маршрути не монтуються.
#[derive(Clone)]
pub struct LocalApiState {
    /// Конфігурація вузла (mode/порт/degrade).
    pub cfg: NodeConfig,
    /// Пул локальної репліки (RLS-контекст проставляється тим самим StorePool).
    pub pool: StorePool,
    /// Пул ЗАПИСУ в primary (`[node] upstream_write_url`, ADR-0007 F3).
    /// `None` — апстріму немає/недосяжний: запис у нього неможливий
    /// (F5: локальна репліка ціллю запису не є ніколи → подію пропускаємо).
    pub upstream_pool: Option<PgPool>,
    /// Читання довідників з репліки (категорії/продукти/постачальники).
    pub readdirs: Arc<dyn ReadDirectories + Send + Sync>,
    /// POS-читання з репліки (чеки: stats/search).
    pub pos: Arc<dyn PosService + Send + Sync>,
    /// Залишки/інвентар з репліки.
    pub write: Arc<dyn WriteDirectories + Send + Sync>,
}

impl LocalApiState {
    /// Резолв URL primary (для перевірки доступності): явний `primary_db_url`
    /// → активне джерело db_sources.toml.
    pub fn primary_url(&self) -> Option<String> {
        self.cfg.resolve_primary_db_url()
    }

    /// Пул апстрім-запису (ADR-0007 F3/F5): `Some` лише коли primary
    /// сконфігурований і був доступний при старті фасаду.
    pub fn upstream_write_pool(&self) -> Option<&PgPool> {
        self.upstream_pool.as_ref()
    }

    /// Жива перевірка: чи primary приймає TCP-з'єднання (таймаут 2 с).
    pub async fn primary_is_up(&self) -> bool {
        match self.primary_url() {
            Some(url) => node_config::primary_reachable(&url).await,
            None => false,
        }
    }
}

/// Останнє спостереження доступності primary (для edge-детектора переходів).
static LAST_PRIMARY_UP: Mutex<Option<bool>> = Mutex::new(None);

/// Фіксує ПЕРЕХІДИ primary up/down у діагностичний журнал network_events
/// (рішення Творця): degraded_local (warn) при недоступності, primary_restored
/// (info) при відновленні. Edge-детектор — запис лише при ЗМІНІ стану, тож
/// поллінг /local/status не спамить (суворіше за обмеження «1 запис/60 с»:
/// у сталому стані записів немає взагалі). Перше спостереження сесії вже в
/// деградації — теж логується (раз). Запис м'який: на read-only репліці
/// (до promote) INSERT неможливий — log_node_event глушить помилку в stderr.
async fn note_connectivity_transition(up: Option<&PgPool>, primary_up: bool) {
    // Lock — ЛИШЕ в межах блоку: guard мусить знятись ДО .await (інакше
    // future не Send і axum Handler не реалізується). Повертає подію, яку
    // треба записати (None — стан не змінився, без запису).
    let to_log: Option<(&str, &str)> = {
        let mut last = match LAST_PRIMARY_UP.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match *last {
            None => {
                *last = Some(primary_up);
                if primary_up {
                    None // перше спостереження — primary доступний, норма
                } else {
                    // сесія стартувала вже в деградації — зафіксувати раз
                    Some(("degraded_local", "warn"))
                }
            }
            Some(prev) if prev != primary_up => {
                *last = Some(primary_up);
                if primary_up {
                    Some(("primary_restored", "info"))
                } else {
                    Some(("degraded_local", "warn"))
                }
            }
            Some(_) => None, // стан без змін — без запису (анти-спам)
        }
    };
    if let Some((event, level)) = to_log {
        match event_log_pool(up) {
            Some(pool) => {
                crate::network::log_node_event(pool, None, event, level, json!({})).await
            }
            None => eprintln!(
                "[torgashka-api] network: подію {event} не записано — апстрім-пул недоступний (standby; F5 ADR-0007: локальна репліка не ціль запису)"
            ),
        }
    }
}

/// Ціль запису діагностичної події (ADR-0007 F5): ТІЛЬКИ апстрім-пул.
/// `None` → подію НЕ пишемо (подія некритична; краще втратити рядок
/// діагностики, ніж робити INSERT у read-only репліку).
fn event_log_pool(up: Option<&PgPool>) -> Option<&PgPool> {
    up
}

/// Доступ до локального стану з хендлера (503, якщо репліка не підключена).
pub(crate) fn local(state: &AppState) -> Result<LocalApiState, LocalErr> {
    state.local.clone().ok_or_else(|| {
        LocalErr::Unavailable(
            "локальна репліка недоступна (режим standby не активовано або PG на 5433 не запущено)"
                .into(),
        )
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Помилки
// ─────────────────────────────────────────────────────────────────────────────

/// Помилки локального режиму → HTTP.
#[derive(Debug, thiserror::Error)]
pub enum LocalErr {
    #[error("локальний режим недоступний: {0}")]
    Unavailable(String),
    #[error("помилка читання репліки: {0}")]
    Read(String),
    #[error("помилка SQLite-черги: {0}")]
    Queue(String),
    #[error("некоректний запит: {0}")]
    BadRequest(String),
    #[error("помилка автентифікації: {0}")]
    Unauthorized(String),
    #[error("доступ заборонено: {0}")]
    Forbidden(String),
    #[error("конфлікт стану: {0}")]
    Conflict(String),
    #[error("не знайдено: {0}")]
    NotFound(String),
}

impl IntoResponse for LocalErr {
    fn into_response(self) -> Response {
        let (code, msg) = match &self {
            LocalErr::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m.clone()),
            LocalErr::Read(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),
            LocalErr::Queue(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),
            LocalErr::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            LocalErr::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            LocalErr::Forbidden(m) => (StatusCode::FORBIDDEN, m.clone()),
            LocalErr::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            LocalErr::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
        };
        (code, Json(json!({"detail": msg}))).into_response()
    }
}

// Інфраструктурні варіанти несуть сирий текст БД (sqlx/PG) — він іде ЛИШЕ в
// лог; у тілі відповіді лишається людський текст (ADR-0007 §D). Людські
// варіанти (NotFound/BadRequest/Conflict/Forbidden) — без змін.
impl From<torgashka_domain::DirectoryError> for LocalErr {
    fn from(e: torgashka_domain::DirectoryError) -> Self {
        match e {
            torgashka_domain::DirectoryError::Infrastructure(msg) => {
                torgashka_infrastructure::embedded_pg::pg_log(
                    "ERROR",
                    &format!("[route_local] DirectoryError::Infrastructure: {msg}"),
                );
                LocalErr::Read("Не вдалося виконати операцію з даними, спробуйте ще раз".into())
            }
            other => LocalErr::Read(other.to_string()),
        }
    }
}

impl From<PosError> for LocalErr {
    fn from(e: PosError) -> Self {
        match e {
            PosError::Infrastructure(msg) => {
                torgashka_infrastructure::embedded_pg::pg_log(
                    "ERROR",
                    &format!("[route_local] PosError::Infrastructure: {msg}"),
                );
                LocalErr::Read("Не вдалося виконати операцію, спробуйте ще раз".into())
            }
            other => LocalErr::Read(other.to_string()),
        }
    }
}

impl From<WriteError> for LocalErr {
    fn from(e: WriteError) -> Self {
        match e {
            WriteError::Infrastructure(msg) => {
                torgashka_infrastructure::embedded_pg::pg_log(
                    "ERROR",
                    &format!("[route_local] WriteError::Infrastructure: {msg}"),
                );
                LocalErr::Read("Не вдалося зберегти зміну, спробуйте ще раз".into())
            }
            other => LocalErr::Read(other.to_string()),
        }
    }
}

impl LocalErr {
    /// Стабільний машинний код для машинних споживачів (`promote.outbox_drain.error`):
    /// `LocalErr::Read`/`Queue` народжуються з помилок БД, їхній Display може
    /// містити текст PG/SQLite — назовні віддаємо код, сирий текст уже в лог.
    pub fn db_class(&self) -> String {
        match self {
            LocalErr::Read(_) | LocalErr::Queue(_) => "[DB_ERROR]".to_string(),
            other => other.to_string(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Статус вузла (ЕТАП 18.D — heartbeat/status)
// ─────────────────────────────────────────────────────────────────────────────

/// Ефективний статус вузла для heartbeat (значення з
/// `network_nodes::HEARTBEAT_STATUSES`):
/// - primary недоступний           → `offline` (вузол живий, але відрізаний);
/// - primary доступний + черга > 0 → `lagging` (є несинхронізовані операції);
/// - primary доступний + черга = 0 → `active`.
pub fn effective_status(primary_up: bool, pending_ops: usize) -> &'static str {
    if !primary_up {
        "offline"
    } else if pending_ops > 0 {
        "lagging"
    } else {
        "active"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query-структури (мінімальні, формат сумісний з основними хендлерами)
// ─────────────────────────────────────────────────────────────────────────────

/// GET /api/v1/local/products — підмножина фільтрів основного API.
#[derive(Debug, Default, Deserialize)]
pub struct LocalProductQuery {
    pub query: Option<String>,
    pub category_id: Option<String>,
    pub page: Option<i64>,
    pub size: Option<i64>,
}

impl LocalProductQuery {
    fn into_filters(self) -> ProductFilters {
        ProductFilters {
            query: self.query.filter(|s| !s.trim().is_empty()),
            barcode: None,
            category_id: self
                .category_id
                .filter(|s| !s.trim().is_empty())
                .and_then(|s| Uuid::parse_str(s.trim()).ok()),
            supplier_id: None,
            min_price: None,
            max_price: None,
            is_weight: None,
            page: self.page.unwrap_or(1).max(1),
            size: self.size.unwrap_or(50).clamp(1, 100),
        }
    }
}

/// GET /api/v1/local/receipts/search.
#[derive(Debug, Default, Deserialize)]
pub struct LocalReceiptSearchQuery {
    pub q: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub receipt_type: Option<String>,
    pub page: Option<i64>,
    pub size: Option<i64>,
}

/// Парсинг дати: ISO `%Y-%m-%dT%H:%M:%S%.f` або `%Y-%m-%d` (північ).
fn parse_local_dt(s: &str) -> Option<chrono::NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })
}

impl LocalReceiptSearchQuery {
    fn into_query(self) -> ReceiptSearchQuery {
        ReceiptSearchQuery {
            q: self.q.unwrap_or_default(),
            date_from: self.date_from.as_deref().and_then(parse_local_dt),
            date_to: self.date_to.as_deref().and_then(parse_local_dt).map(|dt| {
                // date-only → кінець дня (23:59:59.999999), як normalize_date_to.
                if dt.hour() == 0 && dt.minute() == 0 && dt.second() == 0 {
                    dt.date()
                        .and_hms_nano_opt(23, 59, 59, 999_999_000)
                        .unwrap_or(dt)
                } else {
                    dt
                }
            }),
            receipt_type: self.receipt_type.filter(|s| !s.trim().is_empty()),
            page: self.page.unwrap_or(1).max(1),
            size: self.size.unwrap_or(20).clamp(1, 100),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Хендлери: статус/читання
// ─────────────────────────────────────────────────────────────────────────────

/// GET /api/v1/local/status — стан локального режиму та вузла.
///
/// `primary_reachable` — жива TCP-перевірка (до 2 с); `effective` — статус
/// для heartbeat (ЕТАП 18.D); `queue_pending` — несинхронізовані операції
/// SQLite-черги.
pub async fn local_status(State(state): State<AppState>) -> Result<Json<Value>, LocalErr> {
    let ls = local(&state)?;
    let primary_up = ls.primary_is_up().await;
    note_connectivity_transition(ls.upstream_write_pool(), primary_up).await;
    let cfg = &ls.cfg;
    let pending = offline::commands::get_unsynced_count().unwrap_or(0);
    let status = effective_status(primary_up, pending);
    Ok(Json(json!({
        "mode": "standby",
        "degrade_to_local": cfg.degrade_to_local,
        "local_port": cfg.local_port,
        "primary_reachable": primary_up,
        "effective": status,
        "queue_pending": pending,
    })))
}

/// GET /api/v1/local/categories?page=&size= — категорії з ЛОКАЛЬНОЇ репліки.
pub async fn local_categories(
    State(state): State<AppState>,
    Query(q): Query<PageSize>,
) -> Result<Json<Page<CategoryDto>>, LocalErr> {
    let ls = local(&state)?;
    let (page, size) = q.page_size();
    Ok(Json(ls.readdirs.list_categories(page, size).await?))
}

/// GET /api/v1/local/products?query=&category_id=&page=&size=.
pub async fn local_products(
    State(state): State<AppState>,
    Query(q): Query<LocalProductQuery>,
) -> Result<Json<Page<ProductDto>>, LocalErr> {
    let ls = local(&state)?;
    let filters = q.into_filters();
    Ok(Json(ls.readdirs.list_products(&filters).await?))
}

/// GET /api/v1/local/receipts/stats/today — статистика чеків за сьогодні
/// (локальна репліка; RLS-контекст каси застосовується StorePool).
pub async fn local_receipts_today(
    State(state): State<AppState>,
) -> Result<Json<ReceiptStatsDto>, LocalErr> {
    let ls = local(&state)?;
    Ok(Json(ls.pos.today_stats().await?))
}

/// GET /api/v1/local/receipts/search — історія чеків з локальної репліки.
pub async fn local_receipts_search(
    State(state): State<AppState>,
    Query(q): Query<LocalReceiptSearchQuery>,
) -> Result<Json<ReceiptSearchDto>, LocalErr> {
    let ls = local(&state)?;
    let query = q.into_query();
    Ok(Json(ls.pos.search_receipts(&query).await?))
}

/// GET /api/v1/local/inventory/counts — залишки (інвентаризаційні лічильники).
pub async fn local_inventory_counts(
    State(state): State<AppState>,
) -> Result<Json<InventoryCountsDto>, LocalErr> {
    let ls = local(&state)?;
    Ok(Json(ls.write.inventory_counts().await?))
}

#[derive(Debug, Default, Deserialize)]
pub struct PageSize {
    pub page: Option<i64>,
    pub size: Option<i64>,
}

impl PageSize {
    fn page_size(self) -> (i64, i64) {
        (
            self.page.unwrap_or(1).max(1),
            self.size.unwrap_or(50).clamp(1, 100),
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Хендлер: запис у SQLite-чергу
// ─────────────────────────────────────────────────────────────────────────────
//
// B (ADR-0007 §11.5): ДРУГОЇ реалізації запису чека тут немає. Маршрут
// `POST /api/v1/local/receipts` видалено: канонічний (єдиний) шлях чека —
// `POST /api/v2/receipts/sale|return` → `state.pos` (на standby це
// `OutboxPos` → та сама SQLite-черга через
// `sync_push::enqueue_receipt_with_uuid`). Причина — два входи з різними
// контрактами (сирий JSON vs `ReceiptCreateInput`) на ту саму чергу.

/// Тіло POST /api/v1/local/ops — універсальний запис POS-операції в чергу.
#[derive(Debug, Deserialize)]
pub struct LocalOpBody {
    /// Тип операції: receipt | purchase_order | inventory | transfer | write_off.
    pub kind: String,
    #[serde(default)]
    pub store_id: Option<String>,
    /// Пейлоад операції (той самий, що й у відповідній offline-команді).
    pub payload: Value,
}

/// POST /api/v1/local/ops — операція (не чек) у SQLite-чергу.
pub async fn local_enqueue_op(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LocalOpBody>,
) -> Result<impl IntoResponse, LocalErr> {
    let _ = local(&state)?;
    let store = body
        .store_id
        .clone()
        .or_else(|| {
            headers
                .get("x-store-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| {
            LocalErr::BadRequest("store_id обов'язковий для POST /api/v1/local/ops".into())
        })?;
    let payload = body.payload.to_string();
    let out = match body.kind.as_str() {
        // Другої реалізації запису чека тут немає (§11.5): чек приймає
        // канонічний `POST /api/v2/receipts/sale|return` → `state.pos`.
        "receipt" => Err(
            "kind 'receipt' прибрано: чек приймає POST /api/v2/receipts/sale|return".to_string(),
        ),
        "purchase_order" => offline::commands::save_purchase_order_offline(payload, store)
            .map(|id| json!({"queued": true, "local_id": id})),
        "inventory" => offline::commands::save_inventory_offline(payload, store)
            .map(|id| json!({"queued": true, "local_id": id})),
        "transfer" => offline::commands::save_transfer_offline(payload, store)
            .map(|id| json!({"queued": true, "local_id": id})),
        "write_off" => offline::commands::save_write_off_offline(payload, store)
            .map(|id| json!({"queued": true, "local_id": id})),
        other => {
            return Err(LocalErr::BadRequest(format!(
                "невідомий kind '{other}' (очікується purchase_order|inventory|transfer|write_off)"
            )))
        }
    };
    out.map(|v| (StatusCode::ACCEPTED, Json(v)))
        .map_err(LocalErr::Queue)
}

/// GET /api/v1/local/queue/status — стан SQLite-черги (як Tauri sync_status).
pub async fn local_queue_status() -> Result<Json<Value>, LocalErr> {
    offline::commands::sync_status()
        .map(Json)
        .map_err(LocalErr::Queue)
}

/// POST /api/v1/local/sync/now — спроба push-у черги на primary ЗАРАЗ
/// (клієнт викликає після відновлення мережі; далі — фоновий push-таск).
pub async fn local_sync_now() -> Result<Json<Value>, LocalErr> {
    offline::commands::sync_now()
        .await
        .map(Json)
        .map_err(LocalErr::Queue)
}

// ─────────────────────────────────────────────────────────────────────────────
// Хендлер: ЗВІРКА ЗАЛИШКІВ НАКЛАДНОЇ (ADR-0007 §10.3 п.3)
// ─────────────────────────────────────────────────────────────────────────────
//
// Показує по кожному товару документа ОБИДВА числа й дельту:
//   * `local_qty`         — локальний (оптимістичний) залишок каси, SQLite;
//   * `authoritative_qty` — залишок точки з РЕПЛІКИ (PostgreSQL);
//   * `delta` = local − authoritative.
//
// §10.3 (заборона): це ЛИШЕ ПОКАЗ. Вирівнювання робить НАЯВНИЙ механізм —
// інвентаризація (`TYPE_INVENTORY` → `stock::set_stock_level`). Жодного
// нового reconcile-движка тут немає й не має бути.
//
// `local_qty` — ОЦІНКА, а не істина: каса застосовує stock-ефект документа
// одразу (offline-first), тож до доставки на primary число може випереджати
// авторитетне (дельта > 0) або відставати (дельта < 0).

/// Параметри звірки: id документа = `client_uuid` локального агрегата
/// (той самий id, що повертає створення накладної).
#[derive(Debug, Deserialize)]
pub struct StockReconciliationQuery {
    pub invoice_id: String,
}

/// GET /api/v1/local/stock-reconciliation?invoice_id=<uuid>
pub async fn local_stock_reconciliation(
    State(state): State<AppState>,
    Query(q): Query<StockReconciliationQuery>,
) -> Result<Json<Value>, LocalErr> {
    let ls = local(&state)?;
    let invoice_id = q.invoice_id.trim().to_string();
    if invoice_id.is_empty() {
        return Err(LocalErr::BadRequest(
            "потрібен invoice_id (client_uuid документа з локальної черги каси)".to_string(),
        ));
    }

    // ── Локальна половина: агрегат каси + оптимістичний залишок SQLite ─────
    let db_path = offline::db::OfflineDatabase::default_db_path().map_err(LocalErr::Queue)?;
    let view = {
        let conn = offline::sync_push::open_connection(&db_path).map_err(LocalErr::Queue)?;
        offline::reconciliation::local_view(&conn, &invoice_id).map_err(LocalErr::Queue)?
    };
    let Some(view) = view else {
        return Err(LocalErr::NotFound(format!(
            "документ {invoice_id} відсутній у локальній черзі цієї каси —              звірка показує документи, створені/прийняті касою"
        )));
    };
    // Документ мусить належати точці запиту (RLS у SQLite немає — перевіряємо).
    if let Some(ctx) = current_store_ctx() {
        if let Some(doc_store) = view.store_id.as_deref() {
            if doc_store != ctx.store_id.to_string() {
                return Err(LocalErr::NotFound(format!(
                    "документ {invoice_id} належить іншій точці"
                )));
            }
        }
    }
    let store_id = view.store_id.clone().unwrap_or_default();

    // ── Авторитетна половина: залишок точки з репліки PG ──────────────────
    let mut items: Vec<Value> = Vec::with_capacity(view.lines.len());
    let (mut matching, mut mismatching) = (0usize, 0usize);
    for line in &view.lines {
        let auth = authoritative_milli(&ls.pool, &store_id, &line.product_id).await?;
        let (auth_qty, delta, matches) = match auth {
            Some(auth_milli) => {
                let delta_milli = line.local_milli - auth_milli;
                (
                    Some(offline::stock::milli_to_units(auth_milli)),
                    Some(offline::stock::milli_to_units(delta_milli)),
                    delta_milli == 0,
                )
            }
            // Товар не є UUID primary (напр. рядок, написаний legacy-касою):
            // авторитетного числа для нього не існує — це ПОКАЗУЄМО, не 500.
            None => (None, None, false),
        };
        if matches {
            matching += 1;
        } else {
            mismatching += 1;
        }
        items.push(json!({
            "product_id": line.product_id,
            "name": line.name,
            "local_qty": offline::stock::milli_to_units(line.local_milli),
            "authoritative_qty": auth_qty,
            "delta": delta,
            "matches": matches,
        }));
    }

    Ok(Json(json!({
        "invoice_id": view.invoice_id,
        "kind": view.kind,
        "number": view.number,
        "store_id": view.store_id,
        "local_source": "sqlite_cash_queue",
        "authoritative_source": "replica_pg",
        // §10.3: локальне число — ОЦІНКА каси, не істина; вирівнювання —
        // інвентаризацією (наявний механізм), не цим ендпоінтом.
        "local_is_estimate": true,
        "note": "local_qty — оптимістична ОЦІНКА каси (SQLite, ADR-0007 §10.3), \
                 не істина; авторитет — replica_pg. Вирівнювання — інвентаризацією.",
        "items": items,
        "summary": {
            "total": view.lines.len(),
            "matching": matching,
            "mismatching": mismatching,
        },
    })))
}

/// Авторитетний залишок товару в точці з РЕПЛІКИ: міліодиниці (scale 3).
/// `None` — товар не є UUID primary (авторитетного числа не існує).
async fn authoritative_milli(
    pool: &StorePool,
    store_id: &str,
    product_id: &str,
) -> Result<Option<i64>, LocalErr> {
    let (Ok(store), Ok(product)) = (Uuid::parse_str(store_id), Uuid::parse_str(product_id)) else {
        return Ok(None);
    };
    let qty: Option<String> = sqlx::query_scalar(
        "SELECT quantity::text FROM stock WHERE store_id = $1 AND product_id = $2",
    )
    .bind(store)
    .bind(product)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        // Сирий текст PG → лог; у тілі (500) — людський текст (ADR-0007 §D).
        torgashka_infrastructure::embedded_pg::pg_log(
            "ERROR",
            &format!("[route_local] читання stock з репліки ({product_id}): {e}"),
        );
        LocalErr::Read("Не вдалося прочитати залишок з локальної репліки, спробуйте ще раз".into())
    })?;
    Ok(Some(match qty {
        // Рядка stock немає = залишок 0 (та сама семантика, що локальна).
        None => 0,
        Some(text) => (text.trim().parse::<f64>().unwrap_or(0.0)
            * offline::stock::UNITS_SCALE as f64)
            .round() as i64,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Збірка роутера
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/local/outbox/drain (ФАЗА 3.8 — безпека черги при promote)
// ─────────────────────────────────────────────────────────────────────────────

/// Підсумок drain-у черги вузла у ВЛАСНИЙ PG.
#[derive(Debug, Clone, Default)]
pub struct DrainSummary {
    /// Скільки агрегатів знято з черги (created + already_exists).
    pub drained: usize,
    pub created: usize,
    pub already_exists: usize,
    /// Агрегати, які застосувати НЕ вдалось (лишаються `pending` — дані не
    /// втрачено; причина в `errors_detail`).
    pub errors: usize,
    /// Скільки ще лишилось pending після циклу (батчі по 50: якщо > 0 —
    /// викликати drain ще раз).
    pub pending_left: usize,
    /// Перші (до 10) тексти помилок — оператору, без проксіювання коду.
    pub errors_detail: Vec<String>,
}

/// Залишок SQLite-черги каси → ВЛАСНИЙ PostgreSQL ТИМ САМИМ ядром, що й
/// серверний прийом push (`sync::process_push_item`): жодного дублювання
/// логіки застосування агрегатів (накладна/чек/повернення/інвентаризація...).
///
/// Навіщо: після `promote` вузол став джерелом істини — старого primary немає
/// ані в `[node] primary_db_url`, ані в реальності. Агрегати, зроблені
/// офлайн, мусять потрапити у ВЛАСНИЙ PG (а не в HTTP на мертвий сервер,
/// див. ґейт `sync_push::push_pending_batch_with_node`).
///
/// Ідемпотентність — `client_uuid` (як у push): повторний drain дає
/// `already_exists` і НЕ створює дублів. Помилка конкретного агрегата НЕ
/// знімає його з черги (статус лишається `pending`) і не валить решту: підсумок
/// показує `errors` + перші причини.
///
/// Ціль (пул/сервіси) — `state.local`, якщо є (standby, у т.ч. щойно
/// promote-нутий до рестарту: локальний кластер ЛИШЕ ЩО став writable),
/// інакше `state.store_pool` (рестарт після promote: mode=Primary, локальних
/// маршрутів немає) — БД, у яку фасад і так пише.
///
/// Сервіси будуються СИРІ (`SqlxPos`/`SqlxInvoices`/`SqlxReturnInvoices`) —
/// НЕ outbox-адаптери: інакше drain замкнувся б сам на себе (адаптер знову
/// поклав би агрегат у SQLite-чергу).
pub async fn drain_local_outbox(
    state: &AppState,
    claims: &crate::auth::Claims,
) -> Result<DrainSummary, LocalErr> {
    let cashier = Uuid::parse_str(&claims.sub)
        .map_err(|_| LocalErr::BadRequest("sub власника не є UUID — drain неможливий".into()))?;
    let db_path = offline::db::OfflineDatabase::default_db_path().map_err(LocalErr::Queue)?;
    let mut conn = offline::sync_push::open_connection(&db_path).map_err(LocalErr::Queue)?;

    let pool: StorePool = match state.local.as_ref() {
        Some(ls) => ls.pool.clone(),
        None => state.store_pool.clone().ok_or_else(|| {
            LocalErr::Unavailable(
                "немає пула БД: ні локальної репліки (standby), ні store_pool (primary) — \
                 застосувати чергу нікуди"
                    .into(),
            )
        })?,
    };

    let svc = torgashka_application::PosServiceFacade::new(
        Arc::new(SqlxPos::new(pool.clone())) as Arc<dyn PosService + Send + Sync>
    );
    let invoices_v1: Arc<dyn InvoicesV1Service + Send + Sync> =
        Arc::new(SqlxInvoices::new(pool.clone()));
    let return_invoices: Arc<dyn ReturnInvoicesService + Send + Sync> =
        Arc::new(SqlxReturnInvoices::new(pool.clone()));

    let mut summary = DrainSummary::default();
    // До 5 батчів × 50 (як sync_now): поки є pending і прогрес.
    for _ in 0..5 {
        let batch = offline::sync_push::pending_outbox(&conn, offline::sync_push::PUSH_BATCH_MAX)
            .map_err(LocalErr::Queue)?;
        if batch.is_empty() {
            break;
        }
        let mut progressed = 0usize;
        for item in &batch {
            let Ok(envelope) = serde_json::from_str::<crate::sync::PushEnvelope>(&item.payload)
            else {
                summary.errors += 1;
                if summary.errors_detail.len() < 10 {
                    summary.errors_detail.push(format!(
                        "outbox#{} ({}): payload не є конвертом push",
                        item.id, item.outbox_type
                    ));
                }
                continue;
            };
            // RLS-контекст = точка САМОГО документа (черга вузла; X-Store-Id
            // у drain немає — store-middleware недоступний у DR-режимі).
            let ctx = StoreCtx {
                user_id: cashier,
                store_id: envelope.store_id,
                role: claims.role.clone(),
            };
            let res = with_store_ctx(
                ctx,
                crate::sync::process_push_item(
                    &svc,
                    &pool,
                    Some(&invoices_v1),
                    Some(&return_invoices),
                    &envelope,
                    Some(cashier),
                    envelope.store_id,
                ),
            )
            .await;
            match res.status {
                "created" | "already_exists" => {
                    offline::sync_push::mark_done(&mut conn, item).map_err(LocalErr::Queue)?;
                    if res.status == "created" {
                        summary.created += 1;
                    } else {
                        summary.already_exists += 1;
                    }
                    summary.drained += 1;
                    progressed += 1;
                }
                _ => {
                    summary.errors += 1;
                    if summary.errors_detail.len() < 10 {
                        summary.errors_detail.push(format!(
                            "outbox#{} ({}): {}",
                            item.id,
                            item.outbox_type,
                            res.error.unwrap_or_else(|| "невідома помилка".to_string())
                        ));
                    }
                }
            }
        }
        if progressed == 0 {
            break; // далі молотити нічого (усі помилки) — решта лишається pending
        }
    }
    summary.pending_left = offline::sync_push::outbox_stats(&conn)
        .map_err(LocalErr::Queue)?
        .pending;
    Ok(summary)
}

// ─────────────────────────────────────────────────────────────────────────────
// Stateless owner-авторизація (офлайн)
// ─────────────────────────────────────────────────────────────────────────────

/// Валідує Bearer JWT локально (підпис `jwt_secret`, БЕЗ жодного запиту в БД)
/// і вимагає `role=owner`, `type=access`.
///
/// Єдиний шар захисту маршруту `/api/v1/local/outbox/drain`: він лежить ПОЗА
/// auth/store middleware приватної гілки — щоб працювати, коли власна БД
/// вузла ще/вже недоступна для store-middleware.
///
/// ADR-0008 (E7): перенесено сюди з видаленого `promote.rs`, бо drain
/// лишився єдиним споживачем цієї перевірки.
fn require_owner_offline(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::auth::Claims, LocalErr> {
    let Some(h) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return Err(LocalErr::Unauthorized(
            "Відсутній заголовок авторизації".into(),
        ));
    };
    let Some(token) = h.strip_prefix("Bearer ") else {
        return Err(LocalErr::Unauthorized(
            "Невірний формат токена. Використовуйте Bearer".into(),
        ));
    };
    let claims = crate::auth::validate_jwt(token, &state.jwt_secret)
        .map_err(|e| LocalErr::Unauthorized(format!("Недійсний або прострочений токен: {e}")))?;
    if claims.token_type != "access" {
        return Err(LocalErr::Unauthorized(
            "Очікується access-токен (не refresh)".into(),
        ));
    }
    if claims.role != "owner" {
        return Err(LocalErr::Forbidden(
            "Доступ заборонено: операція доступна лише власнику мережі (role=owner)".into(),
        ));
    }
    Ok(claims)
}

/// POST /api/v1/local/outbox/drain — ручний виклик drain-у (owner-only, без
/// store-middleware: у момент відновлення store-middleware може бути
/// недоступним). Застосовує залишок SQLite-черги ВЛАСНОЇ БД вузла; виклик
/// ідемпотентний.
pub async fn drain_outbox_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, LocalErr> {
    let claims = require_owner_offline(&state, &headers)?;
    let s = drain_local_outbox(&state, &claims).await?;
    Ok(Json(json!({
        "drained": s.drained,
        "created": s.created,
        "already_exists": s.already_exists,
        "errors": s.errors,
        "pending_left": s.pending_left,
        "errors_detail": s.errors_detail,
        "note": "залишок SQLite-черги вузла застосовано до ВЛАСНОГО PostgreSQL; \
                 повторний виклик безпечний (ідемпотентність client_uuid)",
    })))
}

/// Підроутер drain-у: монтується БЕЗ auth+store middleware приватної гілки —
/// авторизація stateless JWT owner усередині хендлера.
/// Монтується ЗАВЖДИ (не лише на standby): після promote+рестарту вузол уже
/// `mode=Primary`, локальних маршрутів немає, а черга в SQLite лишається.
pub fn outbox_router() -> Router<AppState> {
    Router::<AppState>::new().route("/api/v1/local/outbox/drain", post(drain_outbox_handler))
}

/// Будує підроутер `/api/v1/local/*`.
///
/// Монтується ВСЕРЕДИНІ `router_v1::build_router` (перед CORS-шаром), тому
/// маршрути проходять auth + store middleware. Якщо локальна копія БД
/// недоступна — повертає порожній роутер (жодних маршрутів не додається;
/// поведінка фасаду не змінюється).
///
/// ADR-0008: перевірки режиму вузла тут немає — E7 видалив самі режими;
/// рішення ухвалює ФАКТ підключення локальної копії ([`crate::init_local_api`]).
pub fn router(state: AppState) -> Router<AppState> {
    if state.local.is_none() {
        // Локальної копії БД немає — жодних маршрутів не додається.
        return Router::<AppState>::new();
    }
    Router::<AppState>::new()
        .route("/api/v1/local/status", get(local_status))
        .route("/api/v1/local/categories", get(local_categories))
        .route("/api/v1/local/products", get(local_products))
        .route(
            "/api/v1/local/receipts/stats/today",
            get(local_receipts_today),
        )
        .route("/api/v1/local/receipts/search", get(local_receipts_search))
        .route(
            "/api/v1/local/inventory/counts",
            get(local_inventory_counts),
        )
        .route("/api/v1/local/queue/status", get(local_queue_status))
        // READ-поверхня: GET не гейтується write-gate (читання репліки — §10
        // ADR-0007); RLS-контекст точки проставляє StorePool.
        .route(
            "/api/v1/local/stock-reconciliation",
            get(local_stock_reconciliation),
        )
        .route("/api/v1/local/ops", post(local_enqueue_op))
        .route("/api/v1/local/sync/now", post(local_sync_now))
        // Ті самі шари, що й private-гілка основного API: auth (JWT) → store
        // (X-Store-Id + RLS-контекст) → handler. Локальні запити каси йдуть
        // під тим самим захистом, що й звичайні (RLS фільтрує репліку за
        // поточною точкою — дані інших точок мережі не видимі).
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::store_context::store_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_status_mapping() {
        // (primary_up, pending) → статус (ЕТАП 18.D).
        assert_eq!(effective_status(true, 0), "active");
        assert_eq!(effective_status(true, 3), "lagging");
        assert_eq!(effective_status(false, 0), "offline");
        assert_eq!(effective_status(false, 7), "offline");
    }

    #[test]
    fn product_filters_parse_basic() {
        let q = LocalProductQuery {
            query: Some("  молоко ".into()),
            category_id: Some("not-a-uuid".into()),
            page: Some(0),
            size: Some(500),
        };
        let f = q.into_filters();
        assert_eq!(f.query.as_deref(), Some("  молоко "));
        assert_eq!(
            f.category_id, None,
            "невалідний UUID → None (як Python _uuid_or_none)"
        );
        assert_eq!(f.page, 1, "page 0 → 1");
        assert_eq!(f.size, 100, "size клампується до 100");
    }

    #[test]
    fn product_filters_valid_uuid() {
        let uuid = Uuid::new_v4().to_string();
        let q = LocalProductQuery {
            category_id: Some(uuid.clone()),
            ..Default::default()
        };
        assert_eq!(q.into_filters().category_id.unwrap().to_string(), uuid);
    }

    #[test]
    fn receipt_search_query_dates() {
        let q = LocalReceiptSearchQuery {
            q: Some("чек".into()),
            date_from: Some("2026-09-01".into()),
            date_to: Some("2026-09-02T23:59:59".into()),
            page: None,
            size: None,
            receipt_type: None,
        };
        let rq = q.into_query();
        assert_eq!(rq.q, "чек");
        assert!(rq.date_from.is_some(), "date-only парситься");
        // date_to з часом не нормалізується до кінця дня.
        let to = rq.date_to.unwrap();
        assert_eq!(to.hour(), 23);
        assert_eq!(rq.page, 1);
        assert_eq!(rq.size, 20);
    }
}
