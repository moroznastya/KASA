// ─────────────────────────────────────────────────────────────────────────────
// network_nodes — мережа магазинів (ЕТАП 15): реєстр вузлів-standby +
// join/heartbeat/archive API. Реальної реплікації немає — вона в ЕТАП 16.
// ─────────────────────────────────────────────────────────────────────────────
// Таблиця public.network_nodes (ЕТАП 15, network-replication-etap15-20.md §3.1;
// DDL: schema.sql + db.rs NETWORK_NODES_DDL). Цей модуль — ТІЛЬКИ API-шар:
//
//   POST /api/v1/admin/network-nodes                  — вузол + join-код (owner)
//   POST /api/v1/network-nodes/join                   — ПУБЛІЧНИЙ join (rate-limit)
//   PUT  /api/v1/network-nodes/:id/heartbeat          — телеметрія (Bearer node_token)
//   GET  /api/v1/admin/network-nodes                  — список вузлів (admin|owner)
//   POST /api/v1/admin/network-nodes/:id/archive      — вивести з мережі (owner)
//   POST /api/v1/admin/network-nodes/:id/force-resync — повторний basebackup (owner)
//
// Безпека (як network.rs): join_code (8 символів CODE_ALPHABET без 0/O/1/I)
// і node_token (48 hex) повертаються ОДИН раз; у БД — лише SHA-256-хеші
// (join_token_hash / node_token_hash). /admin/* — role через
// auth_routes::require_admin/require_owner (роль береться з JWT, БЕЗ запиту
// в БД). heartbeat автентифікується node_token (Bearer) ОКРЕМО від JWT
// користувача — вузол не є користувачем, тому роут змонтовано поза JWT-шаром
// (activate Router), токен перевіряє сам хендлер.
//
// Rate limiting /join: 5 невдалих спроб з одного IP за 60 с → 429 (in-memory
// Mutex<HashMap>, той самий механізм, що й /devices/activate у network.rs,
// але ОКРЕМИЙ бакет — ліміти не змішуються). IP — з X-Forwarded-For /
// X-Real-IP; без заголовків — спільний ключ "unknown".
//
// Стан-машина (§4): provisioning → (join) → node_token видано, статус лишається
// provisioning; клієнт сам heartbeat-ом позначить syncing (standby_provision
// у ЕТАП 16) → active/lagging; 5 хв без heartbeat → offline (фоновий job);
// archive — лише вручну (owner). Тут лише переходи стану — жодного реального
// DROP ROLE/slot/pg_basebackup (ЕТАП 16+).
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{NaiveDateTime, Utc};
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
    /// Вузол виведено з мережі (archived) — heartbeat більше не приймається.
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
    state
        .write_pool
        .clone()
        .ok_or_else(|| NodeErr::Unavailable("write_pool не ініціалізовано".to_string()))
}

// ─── Rate limiting /join (in-memory, per-IP, ОКРЕМИЙ від /devices/activate) ─

const RATE_MAX_FAILS: u32 = 5;
const RATE_WINDOW: Duration = Duration::from_secs(60);

struct RateBucket {
    fails: u32,
    window_started: Instant,
}

static RATE_BUCKETS: OnceLock<Mutex<HashMap<String, RateBucket>>> = OnceLock::new();

fn rate_buckets() -> &'static Mutex<HashMap<String, RateBucket>> {
    RATE_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// true → запит треба відхилити 429 (5+ невдалих спроб у поточному вікні).
fn rate_blocked(key: &str) -> bool {
    let mut m = rate_buckets().lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    if let Some(b) = m.get_mut(key) {
        if now.duration_since(b.window_started) > RATE_WINDOW {
            b.fails = 0;
            b.window_started = now;
        }
        b.fails >= RATE_MAX_FAILS
    } else {
        false
    }
}

/// Реєструє невдалу спробу join (невірний/протермінований/використаний код).
fn rate_register_fail(key: &str) {
    let mut m = rate_buckets().lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    let b = m.entry(key.to_string()).or_insert_with(|| RateBucket {
        fails: 0,
        window_started: now,
    });
    if now.duration_since(b.window_started) > RATE_WINDOW {
        b.fails = 0;
        b.window_started = now;
    }
    b.fails += 1;
}

/// Успішний join скидає лічильник для ключа.
fn rate_register_ok(key: &str) {
    rate_buckets()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(key);
}

// ─── Генерація секретів ─────────────────────────────────────────────────────

/// Токен вузла: 48 hex-символів (192 біти ентропії, 2× UUIDv4) — той самий
/// формат, що device_token у network.rs.
fn gen_node_token() -> String {
    let mut token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    token.truncate(48);
    token
}

/// Пароль ролі реплікації: 24 hex-символи (96 біт ентропії). Повертається
/// ОДИН раз у join-відповіді; у БД не зберігається (клієнт зберігає його
/// зашифрованим локально — §17.2 db_sources-підхід; CREATE ROLE — ЕТАП 16).
fn gen_replication_password() -> String {
    let mut pwd = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    pwd.truncate(24);
    pwd
}

// ─── Інфо про primary (для join-відповіді та підказки хоста) ───────────────

/// Адреса primary, яку клієнт-вузол використає для pg_basebackup (ЕТАП 16).
/// Джерело — той самий DSN, через який фасад підключений до БД
/// (torgashka_infrastructure::db::resolve_database_url, стандартний парсинг
/// sqlx PgConnectOptions). write_pool уже гарантує валідність DSN у хендлері;
/// fallback 127.0.0.1:5432 з порожньою назвою БД — лише на випадок, коли
/// резолв недоступний (тоді хендлер і так не дійшов би сюди).
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

// ─── POST /api/v1/network-nodes/join (ПУБЛІЧНИЙ, rate-limit) ────────────────

#[derive(Debug, Deserialize)]
pub struct JoinBody {
    pub join_code: String,
    pub node_fingerprint: String,
    pub requested_name: String,
}

#[derive(Debug, Serialize)]
pub struct ReplicationCreds {
    /// Ім'я ролі реплікації на primary (replicator_<short>).
    pub role: String,
    /// Пароль ролі (24 hex) — повертається ОДИН раз.
    pub password: String,
    pub primary_host: String,
    pub primary_port: u16,
    pub primary_database: String,
    /// Ім'я replication slot (standby_<short>).
    pub slot_name: String,
}

#[derive(Debug, Serialize)]
pub struct JoinResponse {
    pub node_id: Uuid,
    /// Довготривалий токен вузла для heartbeat (48 hex) — ОДИН раз.
    pub node_token: String,
    pub replication: ReplicationCreds,
}

/// Рядок вибірки вузла за join-хешем (id, store_id, expires_at, node_token_hash).
type NodeJoinRow = (Uuid, Option<Uuid>, Option<NaiveDateTime>, Option<String>);

/// Публічний join вузла за одноразовим кодом (без JWT — новий комп'ютер ще
/// нічого не має). Той самий rate-limit (5/60с на IP), що й /devices/activate.
pub async fn join_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<JoinBody>,
) -> Result<Json<JoinResponse>, NodeErr> {
    let pool = pool(&state)?;

    let code = body.join_code.trim().to_uppercase();
    if code.is_empty() || code.len() > 9 {
        return Err(NodeErr::BadRequest(
            "join_code: обов'язковий код приєднання (до 9 символів A-Z0-9)".to_string(),
        ));
    }
    let fingerprint = body.node_fingerprint.trim().to_string();
    if fingerprint.is_empty() || fingerprint.len() > 200 {
        return Err(NodeErr::BadRequest(
            "node_fingerprint: обов'язковий ідентифікатор вузла (до 200 символів)".to_string(),
        ));
    }
    let requested_name = body.requested_name.trim().to_string();
    if requested_name.is_empty() || requested_name.len() > 255 {
        return Err(NodeErr::BadRequest(
            "requested_name: обов'язкова назва вузла (до 255 символів)".to_string(),
        ));
    }

    let key = network::client_key(&headers);
    if rate_blocked(&key) {
        return Err(NodeErr::TooManyRequests(
            "Забагато невдалих спроб приєднання з цієї адреси. Спробуйте за хвилину".to_string(),
        ));
    }

    // Код → вузол за SHA-256-хешем (оригінал коду в БД не зберігається).
    let join_hash = network::sha256_hex(&code);
    let row: Option<NodeJoinRow> = sqlx::query_as(
        "SELECT id, store_id, join_token_expires_at, node_token_hash \
         FROM network_nodes WHERE join_token_hash = $1",
    )
    .bind(&join_hash)
    .fetch_optional(&pool)
    .await?;

    let (node_id, _store_id, expires_at, node_token_hash) = match row {
        Some(r) => r,
        None => {
            rate_register_fail(&key);
            return Err(NodeErr::NotFound("Невірний код приєднання".to_string()));
        }
    };
    // Код вже використано (інший вузол встиг першим) → 409.
    if node_token_hash.is_some() {
        rate_register_fail(&key);
        return Err(NodeErr::Conflict(
            "Код приєднання вже використано".to_string(),
        ));
    }
    // Протермінований код (TTL 30 хв) → 404.
    let now = Utc::now().naive_utc();
    match expires_at {
        Some(t) if t < now => {
            rate_register_fail(&key);
            return Err(NodeErr::NotFound(
                "Код приєднання протермінований — зверніться до власника за новим".to_string(),
            ));
        }
        None => {
            rate_register_fail(&key);
            return Err(NodeErr::NotFound("Код приєднання недійсний".to_string()));
        }
        Some(_) => {}
    }

    // Секрети (один раз): node_token (48 hex) → SHA-256 у node_token_hash;
    // replication-креденшли: ім'я ролі/слота від short-id вузла, пароль (24 hex)
    // повертається лише в відповіді. Status лишається 'provisioning' —
    // standby_provision (ЕТАП 16) переведе в 'syncing', поки клієнт сам
    // heartbeat-ом позначить.
    let node_token = gen_node_token();
    let node_token_hash = network::sha256_hex(&node_token);
    let node_hex = node_id.simple().to_string();
    let short: String = node_hex.chars().take(8).collect();
    let role_name = format!("replicator_{short}");
    let slot_name = format!("standby_{short}");
    let repl_password = gen_replication_password();

    // Race-safe "один раз": UPDATE ... WHERE node_token_hash IS NULL — якщо
    // паралельний join тим самим кодом встиг раніше, rows_affected=0 → 409.
    let res = sqlx::query(
        "UPDATE network_nodes \
         SET node_token_hash = $2, join_token_hash = NULL, join_token_expires_at = NULL, \
             replication_role_name = $3, replication_slot_name = $4, updated_at = now() \
         WHERE id = $1 AND node_token_hash IS NULL AND join_token_hash = $5",
    )
    .bind(node_id)
    .bind(&node_token_hash)
    .bind(&role_name)
    .bind(&slot_name)
    .bind(&join_hash)
    .execute(&pool)
    .await?;
    if res.rows_affected() == 0 {
        rate_register_fail(&key);
        return Err(NodeErr::Conflict(
            "Код приєднання вже використано".to_string(),
        ));
    }

    rate_register_ok(&key);
    network::log_node_event(
        &pool,
        Some(node_id),
        "joined",
        "info",
        serde_json::json!({ "requested_name": requested_name }),
    )
    .await;
    // ЕТАП 16 (реалізація CREATE ROLE): фасад primary підключений
    // суперкористувачем (backend/.env: postgres) — створюємо роль
    // реплікації ЗАРАЗ, щоб каса одразу могла виконати pg_basebackup
    // (join віддає креденшли лише один раз). Повторний join/provision:
    // роль уже існує (42710 duplicate_object) → ALTER ROLE оновлює пароль.
    // role_name = replicator_<8 hex> та repl_password = 24 hex — безпечні
    // для прямого SQL (лише [a-z0-9_] / hex), екранування не потрібне.
    let create_role_sql = format!(
        "CREATE ROLE {role} LOGIN REPLICATION PASSWORD '{pw}'",
        role = role_name,
        pw = repl_password
    );
    match sqlx::raw_sql(&create_role_sql).execute(&pool).await {
        Ok(_) => {}
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("42710") => {
            let alter_role_sql = format!(
                "ALTER ROLE {role} WITH LOGIN REPLICATION PASSWORD '{pw}'",
                role = role_name,
                pw = repl_password
            );
            sqlx::raw_sql(&alter_role_sql).execute(&pool).await?;
        }
        Err(e) => return Err(NodeErr::Db(e)),
    }

    let (primary_host, primary_port, primary_database) = primary_db_info();
    Ok(Json(JoinResponse {
        node_id,
        node_token,
        replication: ReplicationCreds {
            role: role_name,
            password: repl_password,
            primary_host,
            primary_port,
            primary_database,
            slot_name,
        },
    }))
}

// ─── PUT /api/v1/network-nodes/:id/heartbeat (Bearer node_token) ────────────

/// Дозволені heartbeat-ом статуси (archived — лише вручну власником; у тілі
/// він не приймається — такий вузол одержує 410 на самому початку).
const HEARTBEAT_STATUSES: &[&str] = &["provisioning", "syncing", "active", "lagging", "offline"];

/// Поріг лагу (50 МБ) — понад нього вузол позначається 'lagging'.
const LAG_THRESHOLD_BYTES: i64 = 50_000_000;
/// Чи вузол «застарілий» — не може просто продовжити реплікацію (ЕТАП 20 §13):
/// його WAL-позиція на primary вже недоступна (max_slot_wal_keep_size=10GB),
/// потрібен примусовий ресинк через новий pg_basebackup.
///
/// Вузол у стані 'syncing' (примусовий ресинк у процесі) завжди пропускається.
/// Решта — відхиляються, якщо «вік» вузла перевищує stale_after, де вік
/// рахується від last_seen_at, а для вузла, що жодного разу не виходив на
/// зв'язок (last_seen_at NULL — свіжий join), — від created_at.
fn requires_force_resync(
    current_status: &str,
    last_seen_at: Option<NaiveDateTime>,
    created_at: NaiveDateTime,
    now: NaiveDateTime,
    stale_after: chrono::Duration,
) -> bool {
    if current_status == "syncing" {
        return false; // примусовий ресинк у процесі — новий basebackup іде
    }
    // last_seen_at виставляється ЛИШЕ прийнятим heartbeat, тож свіжий вузол
    // має NULL за визначенням. Якщо брати NULL за «застарілий» — catch-22:
    // перший heartbeat завжди 410, вузол назавжди лишається 'provisioning'.
    // Тому базою для NULL є created_at (вузол застарілий лише якщо створений
    // давніше за stale_after). Порівняння суворе (>), не (>=).
    let base = last_seen_at.unwrap_or(created_at);
    now.signed_duration_since(base) > stale_after
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatBody {
    /// Бажаний статус вузла (клієнт повідомляє свій фактичний стан).
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub lag_bytes: Option<i64>,
    #[serde(default)]
    pub db_size_bytes: Option<i64>,
    #[serde(default)]
    pub app_version: Option<String>,
}

/// Періодичний heartbeat вузла. Авторизація — Bearer node_token (перевірка
/// через node_token_hash, ОКРЕМО від JWT користувача — вузол не користувач).
/// Оновлює last_seen_at + телеметрію; lag > 50 МБ → status='lagging'.
pub async fn heartbeat_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(node_id): Path<String>,
    Json(body): Json<HeartbeatBody>,
) -> Result<Json<serde_json::Value>, NodeErr> {
    let pool = pool(&state)?;
    let node_id = parse_uuid(&node_id, "node_id")?;

    // Bearer node_token → SHA-256 (оригінал токена в БД не зберігається).
    let token_hash = match headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
    {
        Some(t) => network::sha256_hex(t.trim()),
        None => {
            return Err(NodeErr::Unauthorized(
                "Відсутній заголовок авторизації (Bearer node_token)".to_string(),
            ));
        }
    };

    let row: Option<(Option<String>, String, Option<NaiveDateTime>, NaiveDateTime)> = sqlx::query_as(
        "SELECT node_token_hash, status::text, last_seen_at, created_at FROM network_nodes WHERE id = $1",
    )
    .bind(node_id)
    .fetch_optional(&pool)
    .await?;
    let (stored_hash, current_status, last_seen_at, created_at) = match row {
        Some(r) => r,
        None => return Err(NodeErr::NotFound("Вузол не знайдено".to_string())),
    };
    // Вузол виведено з мережі — heartbeat більше не приймається (410).
    if current_status == "archived" {
        return Err(NodeErr::Gone(
            "Вузол виведено з мережі (archived)".to_string(),
        ));
    }
    if stored_hash.as_deref() != Some(token_hash.as_str()) {
        return Err(NodeErr::Unauthorized("Недійсний токен вузла".to_string()));
    }

    // ЕТАП 20 §13: застарілий standby відхиляється. Вузол, що був offline понад
    // 7 днів, НЕ може «продовжити» — primary обрізав його WAL-хвіст
    // (max_slot_wal_keep_size=10GB). Виняток: status='syncing' (кнопка
    // «Примусовий ресинк» уже перевела вузол і скинула last_seen_at=NULL —
    // новий pg_basebackup у процесі, heartbeat легальний).
    if requires_force_resync(
        &current_status,
        last_seen_at,
        created_at,
        Utc::now().naive_utc(),
        chrono::Duration::days(7),
    ) {
        network::log_node_event(
            &pool,
            Some(node_id),
            "reject_stale",
            "warn",
            serde_json::json!({
                "reason": "offline_over_7_days",
                "current_status": current_status,
                "last_seen_at": last_seen_at,
                "created_at": created_at,
            }),
        )
        .await;
        return Err(NodeErr::Gone(
            "force-resync required: вузол був офлайн понад 7 днів;              запустіть примусовий ресинк (status=syncing) щоб відновити              через новий pg_basebackup"
                .to_string(),
        ));
    }

    // Новий статус: явне значення з тіла (валідується проти дозволених) →
    // overridden на 'lagging' якщо лаг > порогу.
    let mut new_status = current_status.clone();
    if let Some(s) = body
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !HEARTBEAT_STATUSES.contains(&s) {
            return Err(NodeErr::BadRequest(format!(
                "status: недопустиме значення '{s}'"
            )));
        }
        new_status = s.to_string();
    }
    if body.lag_bytes.is_some_and(|l| l > LAG_THRESHOLD_BYTES) {
        new_status = "lagging".to_string();
    }

    // host вузла — клієнтська адреса (та сама, що для rate-limit; вузол
    // ходить на primary напряму, X-Forwarded-For/X-Real-IP ставить proxy).
    let host = {
        let k = network::client_key(&headers);
        if k == "unknown" {
            None
        } else {
            Some(k)
        }
    };

    sqlx::query(
        "UPDATE network_nodes \
         SET status = $2::public.node_status, last_seen_at = now(), \
             host = COALESCE($3, host), \
             app_version = COALESCE($4, app_version), \
             replication_lag_bytes = COALESCE($5, replication_lag_bytes), \
             db_size_bytes = COALESCE($6, db_size_bytes), \
             updated_at = now() \
         WHERE id = $1",
    )
    .bind(node_id)
    .bind(&new_status)
    .bind(host)
    .bind(body.app_version)
    .bind(body.lag_bytes)
    .bind(body.db_size_bytes)
    .execute(&pool)
    .await?;

    if new_status != current_status {
        network::log_node_event(
            &pool,
            Some(node_id),
            "status_change",
            if new_status == "offline" {
                "warn"
            } else {
                "info"
            },
            serde_json::json!({
                "old_status": current_status,
                "new_status": new_status,
                "lag_bytes": body.lag_bytes,
            }),
        )
        .await;
    }

    Ok(Json(serde_json::json!({
        "id": node_id,
        "status": new_status,
    })))
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
    pub replication_lag_bytes: Option<i64>,
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
                host, app_version, last_seen_at, replication_lag_bytes, db_size_bytes, \
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
    /// join|joined|node_created|heartbeat|status_change|promoted|
    /// repoint_requested|archived|resync_requested|degraded_local|
    /// primary_restored|sync_error|reject_stale.
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

// ─── POST /admin/network-nodes/:id/archive | force-resync (owner) ───────────

/// Спільний owner-хелпер статус-переходів (archive/force-resync).
async fn owner_node_status(
    state: &AppState,
    claims: crate::auth::Claims,
    node_id: String,
    to_status: &str,
    action: &str,
) -> Result<Json<serde_json::Value>, NodeErr> {
    let pool = pool(state)?;
    let owner_id = auth_routes::require_owner(state, &claims).await?;
    let node_id = parse_uuid(&node_id, "node_id")?;

    // Поточний стан (для 404 та audit-контексту точки).
    let row: Option<(Option<Uuid>, String)> =
        sqlx::query_as("SELECT store_id, status::text FROM network_nodes WHERE id = $1")
            .bind(node_id)
            .fetch_optional(&pool)
            .await?;
    let (store_id, current) = match row {
        Some(r) => r,
        None => return Err(NodeErr::NotFound("Вузол не знайдено".to_string())),
    };
    // Архівація — термінальний стан: ресинк архівованого неможливий.
    if current == "archived" && to_status != "archived" {
        return Err(NodeErr::Conflict(
            "Вузол виведено з мережі — повторний ресинк недоступний".to_string(),
        ));
    }
    if current != to_status {
        // archive: status='archived'; force-resync: status='syncing',
        // last_seen_at=NULL (клієнт має заново пройти basebackup — ЕТАП 16).
        sqlx::query(
            "UPDATE network_nodes \
             SET status = $2::public.node_status, updated_at = now(), \
                 last_seen_at = CASE WHEN $2::text = 'syncing' THEN NULL ELSE last_seen_at END \
             WHERE id = $1",
        )
        .bind(node_id)
        .bind(to_status)
        .execute(&pool)
        .await?;
    }
    network::audit(
        &pool,
        owner_id,
        action,
        "network_node",
        node_id,
        store_id,
        serde_json::json!({"from": current, "to": to_status}),
    )
    .await;
    // Діагностичний журнал (рішення Творця): owner-переходи стану вузла.
    let (event_name, level) = if to_status == "archived" {
        ("archived", "warn")
    } else {
        ("resync_requested", "warn")
    };
    network::log_node_event(
        &pool,
        Some(node_id),
        event_name,
        level,
        serde_json::json!({ "from": current, "to": to_status }),
    )
    .await;
    Ok(Json(serde_json::json!({
        "id": node_id,
        "status": to_status,
    })))
}

/// Архівація вузла: status='archived'. Реального DROP ROLE/slot НЕ робимо —
/// це ЕТАП 16+; зараз лише позначка (термінальний стан, ідемпотентно).
pub async fn archive_node(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, NodeErr> {
    owner_node_status(&state, claims, node_id, "archived", "network_node_archived").await
}

/// Примусовий ресинк: status='syncing', last_seen_at=NULL. Реального
/// pg_basebackup НЕ запускаємо — це ЕТАП 16; зараз лише стан-перехід.
pub async fn force_resync_node(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, NodeErr> {
    owner_node_status(
        &state,
        claims,
        node_id,
        "syncing",
        "network_node_force_resynced",
    )
    .await
}

// ─── Юніт-тести (без БД) ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn node_token_is_48_hex() {
        for _ in 0..50 {
            let t = gen_node_token();
            assert_eq!(t.len(), 48);
            assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn replication_password_is_24_hex() {
        for _ in 0..50 {
            let p = gen_replication_password();
            assert_eq!(p.len(), 24);
            assert!(p.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn short_id_is_first_8_hex_of_uuid() {
        let id = Uuid::new_v4();
        let short: String = id.simple().to_string().chars().take(8).collect();
        assert_eq!(short.len(), 8);
        assert!(short.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(format!("replicator_{short}").len(), 19);
        assert_eq!(format!("standby_{short}").len(), 16);
    }

    #[test]
    fn heartbeat_statuses_exclude_archived() {
        assert!(!HEARTBEAT_STATUSES.contains(&"archived"));
        assert_eq!(HEARTBEAT_STATUSES.len(), 5);
    }

    // ── ЕТАП 20 §13: захист від застарілих standby ────────────────────────

    fn dt(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    #[test]
    fn offline_over_7_days_requires_force_resync() {
        let now = dt(2026, 9, 10);
        let created = now - chrono::Duration::days(30); // вузол створений давно
        let stale = now - chrono::Duration::days(8); // офлайн > 7 днів
        let fresh = now - chrono::Duration::days(1); // ще в межах вікна
        assert!(requires_force_resync(
            "offline",
            Some(stale),
            created,
            now,
            chrono::Duration::days(7)
        ));
        assert!(!requires_force_resync(
            "offline",
            Some(fresh),
            created,
            now,
            chrono::Duration::days(7)
        ));
        // Межа: рівно 7 днів тому — ще НЕ застарілий (порівняння суворе).
        let boundary = now - chrono::Duration::days(7);
        assert!(!requires_force_resync(
            "offline",
            Some(boundary),
            created,
            now,
            chrono::Duration::days(7)
        ));
    }

    #[test]
    fn never_seen_node_stale_unless_syncing() {
        let now = dt(2026, 9, 10);
        // last_seen_at IS NULL — ніколи не слатав heartbeat: застарілий ЛИШЕ
        // якщо створений давніше за stale_after (created_at — база для NULL).
        // Тут вузол старий → застарілий.
        let old_created = now - chrono::Duration::days(30);
        assert!(requires_force_resync(
            "provisioning",
            None,
            old_created,
            now,
            chrono::Duration::days(7)
        ));
        assert!(!requires_force_resync(
            "syncing",
            None,
            old_created,
            now,
            chrono::Duration::days(7)
        ));
        // Навіть зі старим last_seen_at syncing-вузол пропускається.
        let ancient = dt(2026, 6, 1);
        assert!(!requires_force_resync(
            "syncing",
            Some(ancient),
            old_created,
            now,
            chrono::Duration::days(7)
        ));
    }

    #[test]
    fn active_fresh_node_not_stale() {
        let now = dt(2026, 9, 10);
        let created = now - chrono::Duration::days(30);
        assert!(!requires_force_resync(
            "active",
            Some(now),
            created,
            now,
            chrono::Duration::days(7)
        ));
        assert!(!requires_force_resync(
            "lagging",
            Some(now - chrono::Duration::days(2)),
            created,
            now,
            chrono::Duration::days(7)
        ));
    }

    /// Регресія (вічний 410 на першому heartbeat): свіжий вузол після join має
    /// last_seen_at=NULL, бо це поле виставляє ЛИШЕ прийнятий heartbeat. Такий
    /// вузол НЕ застарілий, доки created_at у межах вікна — перший heartbeat
    /// має прийматися (інакше catch-22: вузол ніколи не стане 'active').
    #[test]
    fn never_seen_fresh_node_is_accepted() {
        let now = dt(2026, 9, 10);
        assert!(!requires_force_resync(
            "provisioning",
            None,
            now - chrono::Duration::hours(1),
            now,
            chrono::Duration::days(7)
        ));
        // Той самий NULL, але вузол створений давніше за stale_after → застарілий.
        assert!(requires_force_resync(
            "provisioning",
            None,
            now - chrono::Duration::days(8),
            now,
            chrono::Duration::days(7)
        ));
    }

    #[test]
    fn primary_db_info_never_panics() {
        let (host, port, db) = primary_db_info();
        assert!(!host.is_empty());
        assert!(port > 0);
        let _ = db; // порожня назва можлива лише коли DSN недоступний
    }
}
