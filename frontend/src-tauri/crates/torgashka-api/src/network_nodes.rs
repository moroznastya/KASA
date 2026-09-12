// ─────────────────────────────────────────────────────────────────────────────
// network_nodes — реєстр вузлів мережі магазинів (ADR-0008 §7.1-A3: наявна
// сутність покриває роль вузла-клієнта хаба: store_id, name, node_token_hash,
// status, last_seen_at).
//
//   POST /api/v1/admin/network-nodes                  — вузол + join-код (owner)
//   GET  /api/v1/admin/network-nodes                  — список вузлів (admin|owner)
//   POST /api/v1/admin/network-nodes/:id/archive      — вивести з мережі (owner)
//   GET  /api/v1/admin/network-events                 — журнал мережевих подій
//
// ADR-0008 §7.2 п.6 (E7): публічний join, heartbeat вузла і force-resync
// ВИДАЛЕНО — вони обслуговували фізичну копію БД (креденшли реплікації,
// replication-слоти), якої більше немає. Синхронізація вузла з
// хабом іде прикладним протоколом (`/api/v1/sync/*`), токен вузла зберігається
// в його власних налаштуваннях (`sync.hub_token`).
//
// Безпека (як network.rs): join-код (8 символів CODE_ALPHABET без 0/O/1/I)
// і node_token (48 hex) повертаються ОДИН раз; у БД — лише SHA-256-хеші
// (join_token_hash / node_token_hash). /admin/* — роль через
// auth_routes::require_admin/require_owner (роль береться з JWT, БЕЗ запиту
// в БД).
// ─────────────────────────────────────────────────────────────────────────────

use std::str::FromStr;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth_routes, network, AppState};

// ─── Помилки → HTTP ({"detail": msg}, як решта модулів фасаду) ──────────────

#[derive(Debug)]
pub enum NodeErr {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    /// Вузол виведено з мережі (archived) — термінальний стан реєстру.
    Gone(String),
    TooManyRequests(String),
    /// БД фасаду недоступна (write_pool=None).
    Unavailable(String),
    Db(sqlx::Error),
    /// require_admin/require_owner (auth_routes) — 401/403/404 як у auth-гілці.
    Auth(auth_routes::AuthRouteError),
}

impl IntoResponse for NodeErr {
    fn into_response(self) -> Response {
        match self {
            NodeErr::BadRequest(m) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NodeErr::Unauthorized(m) => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NodeErr::Forbidden(m) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NodeErr::NotFound(m) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NodeErr::Conflict(m) => {
                (StatusCode::CONFLICT, Json(serde_json::json!({"detail": m}))).into_response()
            }
            NodeErr::Gone(m) => {
                (StatusCode::GONE, Json(serde_json::json!({"detail": m}))).into_response()
            }
            NodeErr::TooManyRequests(m) => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NodeErr::Unavailable(m) => {
                eprintln!("[torgashka-api] network_nodes: {m}");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"detail": "Сервіс недоступний"})),
                )
                    .into_response()
            }
            NodeErr::Db(e) => {
                eprintln!("[torgashka-api] network_nodes: помилка БД: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": "Внутрішня помилка сервера"})),
                )
                    .into_response()
            }
            NodeErr::Auth(e) => e.into_response(),
        }
    }
}

impl From<sqlx::Error> for NodeErr {
    fn from(e: sqlx::Error) -> Self {
        NodeErr::Db(e)
    }
}

impl From<auth_routes::AuthRouteError> for NodeErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        NodeErr::Auth(e)
    }
}

fn parse_uuid(raw: &str, field: &str) -> Result<Uuid, NodeErr> {
    Uuid::parse_str(raw)
        .map_err(|_| NodeErr::BadRequest(format!("Невірний {field}: '{raw}' — очікується UUID")))
}

/// Пул PostgreSQL фасаду (network_nodes — у тій самій public-схемі).
fn pool(state: &AppState) -> Result<PgPool, NodeErr> {
    // Реєстр вузлів мережі живе у ВЛАСНІЙ БД інстанса (ADR-0008: режимів
    // вузла немає, пул для запису завжди той самий).
    state.write_pool_or_err().map_err(NodeErr::Unavailable)
}

// ─── Генерація секретів ─────────────────────────────────────────────────────

// ─── Підказка хоста для join-екрана (не адреса реплікації) ──────────────────

/// Хост:порт, на яких фасад цього вузла доступний у мережі — джерело той самий
/// DSN, через який фасад підключений до БД (стандартний парсинг sqlx
/// `PgConnectOptions`). Показується оператору в `create_node` як підказка
/// («куди підключатися»), жодних креденшлів не містить.
fn primary_db_info() -> (String, u16, String) {
    let defaults = || ("127.0.0.1".to_string(), 5432u16, String::new());
    let url = match torgashka_infrastructure::db::resolve_database_url() {
        Ok(u) => u,
        Err(_) => return defaults(),
    };
    match sqlx::postgres::PgConnectOptions::from_str(&url) {
        Ok(o) => (
            o.get_host().to_string(),
            o.get_port(),
            o.get_database().unwrap_or_default().to_string(),
        ),
        Err(_) => defaults(),
    }
}

// ─── POST /api/v1/admin/network-nodes (owner) ───────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateNodeBody {
    pub name: String,
    /// store_id точки (uuid або null) — NULL/відсутній = вузол без прив'язки
    /// до точки (сам сервер / майбутній primary). Валідація: 404 якщо не існує.
    #[serde(default)]
    pub store_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateNodeResponse {
    pub id: Uuid,
    /// Одноразовий join-код (8 символів A-Z0-9 без 0/O/1/I) — повертається
    /// один раз; у БД — лише SHA-256-хеш.
    pub join_code: String,
    pub join_code_expires_at: NaiveDateTime,
    pub primary_host_hint: String,
}

/// Створює запис вузла (status='provisioning') і видає одноразовий join-код
/// (TTL 30 хв). Повторний виклик для того самого store_id створює НОВИЙ вузол
/// (жодних UNIQUE-обмежень на store_id — мережа дозволяє кілька вузлів).
pub async fn create_node(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(body): Json<CreateNodeBody>,
) -> Result<(StatusCode, Json<CreateNodeResponse>), NodeErr> {
    let pool = pool(&state)?;
    let owner_id = auth_routes::require_owner(&state, &claims).await?;

    let name = body.name.trim().to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(NodeErr::BadRequest(
            "name: обов'язкова назва вузла (до 255 символів)".to_string(),
        ));
    }
    let store_id: Option<Uuid> = match body.store_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(parse_uuid(raw, "store_id")?),
    };
    // Точка має існувати (FK network_nodes.store_id → stores.id) — 404.
    if let Some(sid) = store_id {
        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM stores WHERE id = $1")
            .bind(sid)
            .fetch_optional(&pool)
            .await?;
        if exists.is_none() {
            return Err(NodeErr::NotFound("Точку не знайдено".to_string()));
        }
    }

    // Одноразовий join-код: у БД — лише SHA-256-хеш, TTL 30 хв (UTC).
    let join_code = network::gen_code();
    let join_token_hash = network::sha256_hex(&join_code);
    let row: (Uuid, NaiveDateTime) = sqlx::query_as(
        "INSERT INTO network_nodes (store_id, name, join_token_hash, join_token_expires_at) \
         VALUES ($1, $2, $3, (now() AT TIME ZONE 'utc') + interval '30 minutes') \
         RETURNING id, join_token_expires_at",
    )
    .bind(store_id)
    .bind(&name)
    .bind(&join_token_hash)
    .fetch_one(&pool)
    .await?;
    let (node_id, expires_at) = row;

    network::audit(
        &pool,
        owner_id,
        "network_node_created",
        "network_node",
        node_id,
        store_id,
        serde_json::json!({
            "name": name,
            "role": "standby",
            "status": "provisioning",
            "join_code_ttl_minutes": 30,
        }),
    )
    .await;
    network::log_node_event(
        &pool,
        Some(node_id),
        "node_created",
        "info",
        serde_json::json!({ "name": name }),
    )
    .await;

    let (host, _, _) = primary_db_info();
    Ok((
        StatusCode::CREATED,
        Json(CreateNodeResponse {
            id: node_id,
            join_code,
            join_code_expires_at: expires_at,
            primary_host_hint: host,
        }),
    ))
}

// ─── GET /api/v1/admin/network-nodes (admin|owner) ──────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NodeDto {
    pub id: Uuid,
    /// NULL — вузол без точки (сам сервер / primary).
    pub store_id: Option<Uuid>,
    pub name: String,
    pub role: String,
    pub status: String,
    pub host: Option<String>,
    pub app_version: Option<String>,
    pub last_seen_at: Option<NaiveDateTime>,
    pub db_size_bytes: Option<i64>,
    pub created_at: NaiveDateTime,
}

/// Список вузлів мережі (БЕЗ секретів — жодних хешів/креденшлів реплікації).
pub async fn list_nodes(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
) -> Result<Json<Vec<NodeDto>>, NodeErr> {
    let pool = pool(&state)?;
    auth_routes::require_admin(&state, &claims).await?;

    let nodes: Vec<NodeDto> = sqlx::query_as(
        "SELECT id, store_id, name, role::text AS role, status::text AS status, \
                host, app_version, last_seen_at, db_size_bytes, \
                created_at \
         FROM network_nodes ORDER BY created_at DESC, id",
    )
    .fetch_all(&pool)
    .await?;
    Ok(Json(nodes))
}

// ─── GET /api/v1/admin/network-events (admin|owner) ──────────────────────────

/// Рядок журналу network_events (рішення Творця — діагностика мережі).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NetworkEventDto {
    pub id: Uuid,
    /// NULL — подія без вузла (degraded_local фасаду, repoint…).
    pub node_id: Option<Uuid>,
    /// node_created|archived|status_change|sync_error|reject_stale|
    /// degraded_local|primary_restored (легасі-значення реплікації лишаються
    /// в журналі як історичні записи).
    pub event: String,
    /// info|warn|error.
    pub level: String,
    /// Контекст: old_status,new_status,lag_bytes,error,reason…
    pub detail: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct NetworkEventsQuery {
    /// Фільтр за вузлом (uuid; необов'язковий).
    pub node_id: Option<String>,
    /// Фільтр за типом події (необов'язковий).
    pub event: Option<String>,
    /// Розмір сторінки: 1..=500, default 100.
    pub limit: Option<i64>,
}

/// GET /api/v1/admin/network-events — журнал мережевих подій (admin|owner).
/// Фільтри node_id/event + limit; найновіші перші.
pub async fn list_network_events(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<NetworkEventsQuery>,
) -> Result<Json<Vec<NetworkEventDto>>, NodeErr> {
    let pool = pool(&state)?;
    auth_routes::require_admin(&state, &claims).await?;

    let node_id: Option<Uuid> = match q
        .node_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => Some(parse_uuid(raw, "node_id")?),
        None => None,
    };
    let event: Option<String> = q
        .event
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let limit: i64 = q.limit.unwrap_or(100).clamp(1, 500);

    let events: Vec<NetworkEventDto> = sqlx::query_as(
        "SELECT id, node_id, event, level, detail, created_at \
         FROM network_events \
         WHERE ($1::uuid IS NULL OR node_id = $1) \
           AND ($2::text IS NULL OR event = $2) \
         ORDER BY created_at DESC LIMIT $3",
    )
    .bind(node_id)
    .bind(event)
    .bind(limit)
    .fetch_all(&pool)
    .await?;
    Ok(Json(events))
}

// ─── POST /admin/network-nodes/:id/archive (owner) ──────────────────────────

/// Архівація вузла: `status='archived'` (термінальний стан, ідемпотентно).
///
/// ADR-0008: це ЄДИНИЙ власний перехід стану вузла, що лишився — `force-resync`
/// (повторне копіювання БД) видалено разом із фізичною реплікацією.
pub async fn archive_node(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, NodeErr> {
    let pool = pool(&state)?;
    let owner_id = auth_routes::require_owner(&state, &claims).await?;
    let node_id = parse_uuid(&node_id, "node_id")?;

    let row: Option<(Option<Uuid>, String)> =
        sqlx::query_as("SELECT store_id, status::text FROM network_nodes WHERE id = $1")
            .bind(node_id)
            .fetch_optional(&pool)
            .await?;
    let (store_id, current) = match row {
        Some(r) => r,
        None => return Err(NodeErr::NotFound("Вузол не знайдено".to_string())),
    };
    if current != "archived" {
        sqlx::query(
            "UPDATE network_nodes SET status = 'archived'::public.node_status, updated_at = now() \
             WHERE id = $1",
        )
        .bind(node_id)
        .execute(&pool)
        .await?;
    }
    network::audit(
        &pool,
        owner_id,
        "network_node_archived",
        "network_node",
        node_id,
        store_id,
        serde_json::json!({"from": current, "to": "archived"}),
    )
    .await;
    network::log_node_event(
        &pool,
        Some(node_id),
        "archived",
        "warn",
        serde_json::json!({ "from": current, "to": "archived" }),
    )
    .await;
    Ok(Json(serde_json::json!({
        "id": node_id,
        "status": "archived",
    })))
}

// ─── Юніт-тести (без БД) ────────────────────────────────────────────────────
//
// Юніт-тестів немає: після E7 у модулі лишилися прості SQL-хендлери реєстру
// (create/list/archive), які покриваються e2e (`admin_network_e2e`,
// `network_e2e`), а чисті хелпери колишньої реплікації (генерація
// replication-імен, heartbeat-статуси) видалені разом із нею.
