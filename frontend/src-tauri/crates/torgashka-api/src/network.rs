// ─────────────────────────────────────────────────────────────────────────────
// network — мережевий рівень власника: активація та керування касами
// ─────────────────────────────────────────────────────────────────────────────
// Таблиці (коміт 5621f1e, Частина 3): public.devices,
// store_activation_codes, store_product_prices, audit_log, store_sync_state
// + enum device_status (pending|active|blocked|deleted). Цей модуль — ТІЛЬКИ
// API-шар поверх них:
//
//   POST /api/v1/devices/activate                 — ПУБЛІЧНА активація каси
//   POST /api/v1/admin/stores/:store_id/activation-code — код точки (owner)
//   GET  /api/v1/admin/devices[?store_id=]        — список пристроїв (owner)
//   POST /api/v1/admin/devices/:device_id/block   — заблокувати касу (owner)
//   POST /api/v1/admin/devices/:device_id/unblock — розблокувати (owner)
//   DELETE /api/v1/admin/devices/:device_id       — АРХІВАЦІЯ (status=deleted)
//
// Безпека пристрою: device_token (48 hex, крипто-випадковість через
// Uuid::new_v4 → getrandom) повертається ОДИН раз; у БД зберігається лише
// SHA-256-хеш (device_token_hash). /admin/* — тільки role admin|owner
// (require_admin з auth_routes: роль береться з JWT, БЕЗ запиту в БД).
//
// Rate limiting /devices/activate: 5 невдалих спроб з одного IP за 60 с →
// 429 (проста in-memory реалізація: Mutex<HashMap>). IP береться з
// X-Forwarded-For / X-Real-IP (фасад за reverse-proxy); без заголовків —
// спільний ключ "unknown".
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth_routes, AppState};

// ─── Помилки → HTTP ({"detail": msg}, як решта модулів фасаду) ──────────────

#[derive(Debug)]
pub enum NetworkErr {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    TooManyRequests(String),
    /// БД фасаду недоступна (write_pool=None).
    Unavailable(String),
    Db(sqlx::Error),
    /// require_admin (auth_routes) — 401/403/404 як у auth-гілці.
    Auth(auth_routes::AuthRouteError),
}

impl IntoResponse for NetworkErr {
    fn into_response(self) -> Response {
        match self {
            NetworkErr::BadRequest(m) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NetworkErr::Unauthorized(m) => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NetworkErr::Forbidden(m) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NetworkErr::NotFound(m) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NetworkErr::Conflict(m) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NetworkErr::TooManyRequests(m) => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"detail": m})),
            )
                .into_response(),
            NetworkErr::Unavailable(m) => {
                eprintln!("[torgashka-api] network: {m}");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"detail": "Сервіс недоступний"})),
                )
                    .into_response()
            }
            NetworkErr::Db(e) => {
                eprintln!("[torgashka-api] network: помилка БД: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": "Внутрішня помилка сервера"})),
                )
                    .into_response()
            }
            NetworkErr::Auth(e) => e.into_response(),
        }
    }
}

impl From<sqlx::Error> for NetworkErr {
    fn from(e: sqlx::Error) -> Self {
        NetworkErr::Db(e)
    }
}

fn parse_uuid(raw: &str, field: &str) -> Result<Uuid, NetworkErr> {
    Uuid::parse_str(raw).map_err(|_| {
        NetworkErr::BadRequest(format!(
            "Невірний {field}: '{raw}' — очікується UUID"
        ))
    })
}

/// Пул PostgreSQL фасаду (мережеві таблиці — у тій самій public-схемі).
fn pool(state: &AppState) -> Result<PgPool, NetworkErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| NetworkErr::Unavailable("write_pool не ініціалізовано".to_string()))
}

// ─── Rate limiting активації (in-memory, per-IP) ────────────────────────────

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

/// Ключ клієнта: X-Forwarded-For (перший) → X-Real-IP → "unknown".
fn client_key(headers: &HeaderMap) -> String {
    if let Some(v) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(ip) = v
            .split(',')
            .next()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return ip.to_string();
        }
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let ip = v.trim();
        if !ip.is_empty() {
            return ip.to_string();
        }
    }
    "unknown".to_string()
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

/// Реєструє невдалу спробу активації (невалідний код).
fn rate_register_fail(key: &str) {
    let mut m = rate_buckets().lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    let b = m
        .entry(key.to_string())
        .or_insert_with(|| RateBucket { fails: 0, window_started: now });
    if now.duration_since(b.window_started) > RATE_WINDOW {
        b.fails = 0;
        b.window_started = now;
    }
    b.fails += 1;
}

/// Успішна активація скидає лічильник для ключа.
fn rate_register_ok(key: &str) {
    rate_buckets()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(key);
}

// ─── Генерація секретів ─────────────────────────────────────────────────────

/// Алфавіт коду активації: A-Z0-9 БЕЗ 0/O/1/I.
/// 24 літери (без I,O) + 8 цифр (без 0,1) = 32 символи (5 біт/символ).
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// 8 символів коду активації. Випадковість — Uuid::new_v4 (v4 → getrandom,
/// 122 біти ентропії); rejection sampling (byte < 224 = 32×7) без bias.
fn gen_code() -> String {
    let mut out = String::with_capacity(8);
    while out.len() < 8 {
        for &byte in Uuid::new_v4().as_bytes() {
            if byte < 224 {
                out.push(CODE_ALPHABET[(byte % 32) as usize] as char);
                if out.len() == 8 {
                    break;
                }
            }
        }
    }
    out
}

/// Токен пристрою: 48 hex-символів (192 біти ентропії, 2× UUIDv4).
fn gen_device_token() -> String {
    let mut token = format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    token.truncate(48);
    token
}

/// SHA-256 (hex) — device_token_hash (оригінал токена не зберігається).
fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(s.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Ім'я каси: "Каса <суфікс fingerprint>" (останні 6 безпечних символів).
fn device_name(fingerprint: &str) -> String {
    let clean: String = fingerprint
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let clean = if clean.len() > 64 {
        &clean[clean.len() - 64..]
    } else {
        &clean[..]
    };
    let start = clean.len().saturating_sub(6);
    let suffix = clean[start..].trim();
    let suffix = if suffix.is_empty() { "unknown" } else { suffix };
    format!("Каса {suffix}")
}

// ─── Audit-слід admin-дій (таблиця audit_log, коміт 5621f1e) ────────────────

#[allow(clippy::too_many_arguments)]
pub(crate) async fn audit(
    pool: &PgPool,
    actor_user_id: Uuid,
    action: &str,
    entity_type: &str,
    entity_id: Uuid,
    store_id: Option<Uuid>,
    payload: serde_json::Value,
) {
    if let Err(e) = sqlx::query(
        "INSERT INTO audit_log (actor_user_id, action, entity_type, entity_id, store_id, payload) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(actor_user_id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(store_id)
    .bind(payload)
    .execute(pool)
    .await
    {
        // Аудит не ламає основну дію — лише лог.
        eprintln!("[torgashka-api] network: audit_log не записано: {e}");
    }
}

// ─── POST /api/v1/devices/activate (ПУБЛІЧНИЙ) ──────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ActivateBody {
    pub code: String,
    pub device_fingerprint: String,
}

#[derive(Debug, Serialize)]
pub struct ActivateResponse {
    pub device_token: String,
    pub device_id: Uuid,
    pub store_id: Uuid,
    pub store_name: String,
}

/// Публічна активація каси за кодом точки (без JWT).
pub async fn activate_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ActivateBody>,
) -> Result<Json<ActivateResponse>, NetworkErr> {
    let pool = pool(&state)?;

    let code = body.code.trim().to_uppercase();
    if code.is_empty() || code.len() > 9 {
        return Err(NetworkErr::BadRequest(
            "code: обов'язковий код активації (до 9 символів A-Z0-9)".to_string(),
        ));
    }
    let fingerprint = body.device_fingerprint.trim().to_string();
    if fingerprint.is_empty() || fingerprint.len() > 200 {
        return Err(NetworkErr::BadRequest(
            "device_fingerprint: обов'язковий ідентифікатор пристрою (до 200 символів)"
                .to_string(),
        ));
    }

    let key = client_key(&headers);
    if rate_blocked(&key) {
        return Err(NetworkErr::TooManyRequests(
            "Забагато невдалих спроб активації з цієї адреси. Спробуйте за хвилину".to_string(),
        ));
    }

    // Код → точка (JOIN stores: назва для відповіді каси).
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT c.store_id, s.name \
         FROM store_activation_codes c \
         JOIN stores s ON s.id = c.store_id \
         WHERE c.code = $1",
    )
    .bind(&code)
    .fetch_optional(&pool)
    .await?;

    let (store_id, store_name) = match row {
        Some(r) => r,
        None => {
            rate_register_fail(&key);
            return Err(NetworkErr::NotFound("Невірний код активації".to_string()));
        }
    };

    // Токен повертається один раз; у БД — лише SHA-256-хеш.
    let token = gen_device_token();
    let token_hash = sha256_hex(&token);
    let name = device_name(&fingerprint);

    let device_id: Uuid = sqlx::query_scalar(
        "INSERT INTO devices (store_id, name, device_token_hash, status, activated_at) \
         VALUES ($1, $2, $3, 'active'::public.device_status, now()) \
         RETURNING id",
    )
    .bind(store_id)
    .bind(&name)
    .bind(&token_hash)
    .fetch_one(&pool)
    .await?;

    rate_register_ok(&key);
    Ok(Json(ActivateResponse {
        device_token: token,
        device_id,
        store_id,
        store_name,
    }))
}

// ─── POST /api/v1/admin/stores/:store_id/activation-code ────────────────────

/// Код точки: створити, якщо немає; інакше — регенерувати (regenerated_at).
pub async fn generate_activation_code(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(store_id): Path<String>,
) -> Result<Json<serde_json::Value>, NetworkErr> {
    let pool = pool(&state)?;
    let admin_id = auth_routes::require_admin(&state, &claims)
        .await
        .map_err(NetworkErr::Auth)?;
    let store_id = parse_uuid(&store_id, "store_id")?;

    // Точка має існувати (FK store_activation_codes.store_id).
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM stores WHERE id = $1")
        .bind(store_id)
        .fetch_optional(&pool)
        .await?;
    if exists.is_none() {
        return Err(NetworkErr::NotFound("Точку не знайдено".to_string()));
    }
    let had_code: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM store_activation_codes WHERE store_id = $1)",
    )
    .bind(store_id)
    .fetch_one(&pool)
    .await?;

    // Upsert за store_id; UNIQUE(code) може конфліктувати з кодом ІНШОЇ
    // точки (23505) — генеруємо новий код і повторюємо (до 8 спроб).
    for _ in 0..8 {
        let code = gen_code();
        let res: Result<Option<String>, sqlx::Error> = sqlx::query_scalar(
            "INSERT INTO store_activation_codes (store_id, code, created_by) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (store_id) DO UPDATE \
               SET code = EXCLUDED.code, regenerated_at = now() \
             RETURNING code",
        )
        .bind(store_id)
        .bind(&code)
        .bind(admin_id)
        .fetch_optional(&pool)
        .await;

        match res {
            Ok(Some(final_code)) => {
                audit(
                    &pool,
                    admin_id,
                    "activation_code_generated",
                    "store",
                    store_id,
                    Some(store_id),
                    serde_json::json!({
                        "regenerated": had_code,
                        "code_length": final_code.len(),
                    }),
                )
                .await;
                return Ok(Json(serde_json::json!({"code": final_code})));
            }
            Ok(None) => {
                // INSERT ... RETURNING завжди повертає рядок — неочікувано.
                return Err(NetworkErr::Conflict(
                    "Не вдалося зберегти код активації".to_string(),
                ));
            }
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => continue,
            Err(e) => return Err(NetworkErr::Db(e)),
        }
    }
    Err(NetworkErr::Conflict(
        "Не вдалося згенерувати унікальний код активації (спробуйте ще раз)".to_string(),
    ))
}

// ─── GET /api/v1/admin/devices[?store_id=] ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DevicesQuery {
    pub store_id: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DeviceDto {
    pub id: Uuid,
    pub store_id: Uuid,
    pub name: String,
    pub status: String,
    pub app_version: Option<String>,
    pub last_seen_at: Option<NaiveDateTime>,
    pub activated_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    /// Назва торговельної точки (JOIN stores).
    pub store_name: String,
}

pub async fn list_devices(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<DevicesQuery>,
) -> Result<Json<Vec<DeviceDto>>, NetworkErr> {
    let pool = pool(&state)?;
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(NetworkErr::Auth)?;

    let store_filter = match q.store_id {
        None => None,
        Some(raw) => Some(parse_uuid(&raw, "store_id")?),
    };

    let base = "SELECT d.id, d.store_id, d.name, d.status::text AS status, \
                 d.app_version, d.last_seen_at, d.activated_at, d.created_at, \
                 s.name AS store_name \
                 FROM devices d JOIN stores s ON s.id = d.store_id";
    let devices: Vec<DeviceDto> = match store_filter {
        None => {
            sqlx::query_as(&format!("{base} ORDER BY d.created_at DESC, d.id"))
                .fetch_all(&pool)
                .await?
        }
        Some(sid) => {
            sqlx::query_as(&format!(
                "{base} WHERE d.store_id = $1 ORDER BY d.created_at DESC, d.id"
            ))
            .bind(sid)
            .fetch_all(&pool)
            .await?
        }
    };
    Ok(Json(devices))
}

// ─── POST /admin/devices/:id/block | /unblock, DELETE /admin/devices/:id ─────

/// Спільна логіка block/unblock (status: 'blocked' | 'active').
async fn set_device_status(
    state: &AppState,
    claims: crate::auth::Claims,
    device_id: String,
    new_status: &str,
    action: &str,
) -> Result<Json<serde_json::Value>, NetworkErr> {
    let pool = pool(state)?;
    let admin_id = auth_routes::require_admin(state, &claims)
        .await
        .map_err(NetworkErr::Auth)?;
    let device_id = parse_uuid(&device_id, "device_id")?;

    // Поточний стан (для 404 та audit-контексту точки).
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT store_id, status::text FROM devices WHERE id = $1",
    )
    .bind(device_id)
    .fetch_optional(&pool)
    .await?;
    let (store_id, current) = match row {
        Some(r) => r,
        None => return Err(NetworkErr::NotFound("Пристрій не знайдено".to_string())),
    };
    if current == "deleted" {
        return Err(NetworkErr::Conflict(
            "Пристрій архівовано — керування недоступне".to_string(),
        ));
    }

    sqlx::query(
        "UPDATE devices SET status = $2::public.device_status, updated_at = now() \
         WHERE id = $1",
    )
    .bind(device_id)
    .bind(new_status)
    .execute(&pool)
    .await?;

    audit(
        &pool,
        admin_id,
        action,
        "device",
        device_id,
        Some(store_id),
        serde_json::json!({"from": current, "to": new_status}),
    )
    .await;
    Ok(Json(serde_json::json!({
        "id": device_id,
        "status": new_status,
    })))
}

pub async fn block_device(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, NetworkErr> {
    set_device_status(&state, claims, device_id, "blocked", "device_blocked").await
}

pub async fn unblock_device(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, NetworkErr> {
    set_device_status(&state, claims, device_id, "active", "device_unblocked").await
}

/// DELETE /api/v1/admin/devices/:id — АРХІВАЦІЯ (status='deleted').
/// Фізичного видалення немає: пристрій лишається в історії (audit, sync-стан).
pub async fn delete_device(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, NetworkErr> {
    let pool = pool(&state)?;
    let admin_id = auth_routes::require_admin(&state, &claims)
        .await
        .map_err(NetworkErr::Auth)?;
    let device_id = parse_uuid(&device_id, "device_id")?;

    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT store_id, status::text FROM devices WHERE id = $1",
    )
    .bind(device_id)
    .fetch_optional(&pool)
    .await?;
    let (store_id, current) = match row {
        Some(r) => r,
        None => return Err(NetworkErr::NotFound("Пристрій не знайдено".to_string())),
    };

    // Ідемпотентна архівація: повторний DELETE архівованого → 200.
    if current != "deleted" {
        sqlx::query(
            "UPDATE devices SET status = 'deleted'::public.device_status, updated_at = now() \
             WHERE id = $1",
        )
        .bind(device_id)
        .execute(&pool)
        .await?;
    }
    audit(
        &pool,
        admin_id,
        "device_archived",
        "device",
        device_id,
        Some(store_id),
        serde_json::json!({"from": current, "to": "deleted"}),
    )
    .await;
    Ok(Json(serde_json::json!({
        "id": device_id,
        "status": "deleted",
    })))
}

// ─── Юніт-тести (без БД) ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_alphabet_excludes_ambiguous_chars() {
        let s = String::from_utf8_lossy(CODE_ALPHABET).to_string();
        assert_eq!(s.len(), 32, "алфавіт: 32 символи");
        for ch in ['0', 'O', '1', 'I'] {
            assert!(!s.contains(ch), "алфавіт не має містити {ch}");
        }
    }

    #[test]
    fn generated_code_is_8_chars_from_alphabet() {
        for _ in 0..200 {
            let c = gen_code();
            assert_eq!(c.len(), 8);
            assert!(
                c.chars().all(|ch| CODE_ALPHABET.contains(&(ch as u8))),
                "код '{c}' поза алфавітом"
            );
        }
    }

    #[test]
    fn generated_code_has_variety() {
        let mut codes = std::collections::HashSet::new();
        for _ in 0..200 {
            codes.insert(gen_code());
        }
        assert!(codes.len() > 180, "коди не мають повторюватись ({} унікальних з 200)", codes.len());
    }

    #[test]
    fn device_token_is_48_hex() {
        for _ in 0..50 {
            let t = gen_device_token();
            assert_eq!(t.len(), 48);
            assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn token_hash_is_sha256_hex() {
        let t = gen_device_token();
        let h = sha256_hex(&t);
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        // Детермінованість: той самий токен → той самий хеш.
        assert_eq!(h, sha256_hex(&t));
    }

    #[test]
    fn device_name_uses_fingerprint_suffix() {
        assert_eq!(device_name("KASA-ALPHA-7F3A9C"), "Каса 7F3A9C");
        assert_eq!(device_name("короткий"), "Каса unknown"); // усе відфільтровано
        assert_eq!(device_name("fp:1:2:3:4"), "Каса fp1234"); // ':' відфільтровано
        assert_eq!(device_name("::::"), "Каса unknown"); // нема безпечних символів
    }

    #[test]
    fn client_key_parses_forwarded_for() {
        let mut h = HeaderMap::new();
        assert_eq!(client_key(&h), "unknown");
        h.insert("x-forwarded-for", "203.0.113.7, 10.0.0.1".parse().unwrap());
        assert_eq!(client_key(&h), "203.0.113.7");
        h.remove("x-forwarded-for");
        h.insert("x-real-ip", "198.51.100.9".parse().unwrap());
        assert_eq!(client_key(&h), "198.51.100.9");
    }
}
