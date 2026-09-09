//! Локальний routing-шар standby-вузла (ЕТАП 18).
//!
//! Коли primary мережі магазинів недоступний, вузол (каса) деградує в
//! **локальний режим**:
//!
//! - **читання** (категорії, продукти, залишки, чеки — GET) виконуються
//!   проти ЛОКАЛЬНОЇ embedded PostgreSQL-репліки (порт `node_config::local_port`,
//!   зазвичай 5433; підготовлена `standby_provision` ЕТАП 16) через ТІ САМІ
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
use torgashka_domain::{
    CategoryDto, InventoryCountsDto, Page, PosError, PosService, ProductDto, ProductFilters,
    ReadDirectories, ReceiptSearchDto, ReceiptSearchQuery, ReceiptStatsDto, WriteDirectories,
    WriteError,
};
use torgashka_infrastructure::{
    node_config::{self, NodeConfig},
    offline,
    store_ctx::StorePool,
};
use uuid::Uuid;

use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Стан локального режиму
// ─────────────────────────────────────────────────────────────────────────────

/// Сервіси локального режиму: репозиторії над пулом ЛОКАЛЬНОЇ репліки.
///
/// Створюється фасадом ПРИ СТАРТІ, лише коли `node_config.mode == Standby`
/// і локальна репліка (127.0.0.1:local_port) приймає з'єднання. Інакше
/// `AppState.local = None` — локальні маршрути не монтуються.
#[derive(Clone)]
pub struct LocalApiState {
    /// Конфігурація вузла (mode/порт/degrade).
    pub cfg: NodeConfig,
    /// Пул локальної репліки (RLS-контекст проставляється тим самим StorePool).
    pub pool: StorePool,
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
async fn note_connectivity_transition(pool: &StorePool, primary_up: bool) {
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
        crate::network::log_node_event(&pool.0, None, event, level, json!({})).await;
    }
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
        };
        (code, Json(json!({"detail": msg}))).into_response()
    }
}

impl From<torgashka_domain::DirectoryError> for LocalErr {
    fn from(e: torgashka_domain::DirectoryError) -> Self {
        LocalErr::Read(e.to_string())
    }
}

impl From<PosError> for LocalErr {
    fn from(e: PosError) -> Self {
        LocalErr::Read(e.to_string())
    }
}

impl From<WriteError> for LocalErr {
    fn from(e: WriteError) -> Self {
        LocalErr::Read(e.to_string())
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
    note_connectivity_transition(&ls.pool, primary_up).await;
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
// Хендлери: запис у SQLite-чергу
// ─────────────────────────────────────────────────────────────────────────────

/// Витягує store_id із заголовка X-Store-Id (як основний API) або з тіла.
fn store_id_from(headers: &HeaderMap, body: &Value) -> Option<String> {
    headers
        .get("x-store-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| body["store_id"].as_str().map(|s| s.to_string()))
}

/// POST /api/v1/local/receipts — чек у SQLite-чергу (атомарно + outbox).
///
/// Тіло — той самий JSON чека, що каса передає у звичайний
/// `POST /api/v2/receipts/sale` / офлайн-команду `save_receipt_offline`
/// (валідація/снапшоти/enqueue виконуються offline-шаром, ЕТАП 4).
/// Відповідь 202 Accepted — чек прийнято в чергу, НЕ на primary.
pub async fn local_enqueue_receipt(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, LocalErr> {
    let _ = local(&state)?; // переконатись, що локальний режим увімкнено
    let raw = String::from_utf8(body.to_vec())
        .map_err(|e| LocalErr::BadRequest(format!("тіло чека не UTF-8: {e}")))?;
    let parsed: Value = serde_json::from_str(&raw)
        .map_err(|e| LocalErr::BadRequest(format!("тіло чека не JSON: {e}")))?;
    let store = store_id_from(&headers, &parsed);
    let local_id = offline::commands::save_receipt_offline(raw, store).map_err(LocalErr::Queue)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"queued": true, "local_receipt_id": local_id})),
    ))
}

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
        "receipt" => offline::commands::save_receipt_offline(payload, Some(store))
            .map(|id| json!({"queued": true, "local_id": id})),
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
                "невідомий kind '{other}' (очікується receipt|purchase_order|inventory|transfer|write_off)"
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
// Збірка роутера
// ─────────────────────────────────────────────────────────────────────────────

/// Будує підроутер `/api/v1/local/*`.
///
/// Монтується ВСЕРЕДИНІ `router_v1::build_router` (перед CORS-шаром), тому
/// маршрути проходять auth + store middleware. Якщо вузол НЕ standby або
/// локальна репліка недоступна — повертає порожній роутер (жодних маршрутів
/// не додається; поведінка фасаду не змінюється).
pub fn router(state: AppState) -> Router<AppState> {
    if !state.node_config.is_standby() || state.local.is_none() {
        // Не standby / репліка не підключена — жодних маршрутів не додається.
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
        .route("/api/v1/local/receipts", post(local_enqueue_receipt))
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
            ..Default::default()
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

    #[test]
    fn store_id_prefers_header_then_body() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-store-id",
            "11111111-1111-1111-1111-111111111111".parse().unwrap(),
        );
        let body = json!({"store_id": "22222222-2222-2222-2222-222222222222"});
        assert_eq!(
            store_id_from(&h, &body).unwrap(),
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(
            store_id_from(&HeaderMap::new(), &body).unwrap(),
            "22222222-2222-2222-2222-222222222222"
        );
        assert_eq!(store_id_from(&HeaderMap::new(), &json!({})), None);
    }
}
