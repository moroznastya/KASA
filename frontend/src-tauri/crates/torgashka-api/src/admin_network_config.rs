// ─────────────────────────────────────────────────────────────────────────────
// admin_network_config — генерація/імпорт конфіг-файлу мережі (Етап 3-backend,
// ОБОВ'ЯЗКОВА вимога Творця: перенесення мережі USB/файлообмінником)
// ─────────────────────────────────────────────────────────────────────────────
// Роути (окремий /admin/* роутер БЕЗ store_middleware; owner-only через
// auth_routes::require_owner: admin/store_manager → 403):
//
//   POST /api/v1/admin/network-config/export → 200 { filename, content }
//   POST /api/v1/admin/network-config/import → 200 { ok, network_id, store, server_url }
//
// Конфіг-файл (schema_version=1) — UTF-8 JSON, придатний для передачі
// USB/файлообмінником:
//   {
//     "schema_version": 1,
//     "network_id": "<uuid власника мережі>",
//     "server_url": "http://127.0.0.1:8000",
//     "exported_at": "2026-08-06T12:00:00Z",
//     "store": { "id": "<uuid>", "name": "...", "activation_code": "ABCD2345" },
//     "db": { "host", "port", "database", "user", "password_encrypted"? }
//   }
// Безпека (Етап 5):
//   - лише role=owner (require_owner у auth_routes, спільний БЕЗ дублів);
//   - password_encrypted — AES-256-GCM-захищений текст (db_sources.toml),
//     включається ЛИШЕ за include_db_password=true; plaintext ніколи;
//   - секрети/паролі не логуються і не потрапляють у помилки.
// db-блок присутній лише якщо у db_sources.toml є АКТИВНЕ джерело.
// Код активації пере-генерується через спільний
// network::ensure_activation_code (upsert) — без дублів з
// POST /admin/stores/:store_id/activation-code.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use torgashka_infrastructure::db_sources::{self, DbSourcesError};

use crate::{auth::Claims, auth_routes, network, AppState};

// ─── Помилки → HTTP ({"detail": msg}, як решта адмін-модулів) ───────────────

#[derive(Debug)]
pub enum NetCfgErr {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
    Db(sqlx::Error),
    DbSources(DbSourcesError),
    /// require_owner (auth_routes) — 401/403/404 як у auth-гілці.
    Auth(auth_routes::AuthRouteError),
    /// network::ensure_activation_code (Conflict/Db/...).
    Network(network::NetworkErr),
}

impl From<auth_routes::AuthRouteError> for NetCfgErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        NetCfgErr::Auth(e)
    }
}

impl From<sqlx::Error> for NetCfgErr {
    fn from(e: sqlx::Error) -> Self {
        NetCfgErr::Db(e)
    }
}

impl From<DbSourcesError> for NetCfgErr {
    fn from(e: DbSourcesError) -> Self {
        NetCfgErr::DbSources(e)
    }
}

impl From<network::NetworkErr> for NetCfgErr {
    fn from(e: network::NetworkErr) -> Self {
        NetCfgErr::Network(e)
    }
}

impl IntoResponse for NetCfgErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            NetCfgErr::BadRequest(m) => body(StatusCode::BAD_REQUEST, m),
            NetCfgErr::NotFound(m) => body(StatusCode::NOT_FOUND, m),
            NetCfgErr::Conflict(m) => body(StatusCode::CONFLICT, m),
            NetCfgErr::Auth(e) => e.into_response(),
            NetCfgErr::Network(e) => e.into_response(),
            NetCfgErr::Db(e) => {
                eprintln!("[torgashka-api] network-config: помилка БД: {e}");
                body(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Внутрішня помилка сервера".to_string(),
                )
            }
            NetCfgErr::DbSources(e) => {
                eprintln!("[torgashka-api] network-config: db_sources: {e}");
                body(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Конфігурація джерел даних недоступна".to_string(),
                )
            }
            NetCfgErr::Internal(m) => {
                eprintln!("[torgashka-api] network-config: помилка: {m}");
                body(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Внутрішня помилка сервера".to_string(),
                )
            }
        }
    }
}

/// Пул PostgreSQL фасаду (мережеві таблиці — у тій самій public-схемі).
fn pool(state: &AppState) -> Result<sqlx::PgPool, NetCfgErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| NetCfgErr::Internal("write_pool не ініціалізовано".to_string()))
}

// ─── Модель конфіг-файлу мережі (schema_version=1) ──────────────────────────

const SCHEMA_VERSION: u32 = 1;
/// Дефолтний server_url (коли ні body.server_url, ні env не задано).
const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8000";

#[derive(Debug, Serialize)]
struct NetCfgStore {
    id: Uuid,
    name: String,
    activation_code: String,
}

#[derive(Debug, Serialize)]
struct NetCfgDb {
    host: String,
    port: u16,
    database: String,
    user: String,
    /// AES-256-GCM-захищений пароль (як у db_sources.toml); ЛИШЕ якщо
    /// include_db_password=true. Ніколи plaintext.
    #[serde(skip_serializing_if = "Option::is_none")]
    password_encrypted: Option<String>,
}

#[derive(Debug, Serialize)]
struct NetCfgFile {
    schema_version: u32,
    network_id: Uuid,
    server_url: String,
    exported_at: String,
    store: NetCfgStore,
    #[serde(skip_serializing_if = "Option::is_none")]
    db: Option<NetCfgDb>,
}

// ─── DTO запитів/відповідей API ─────────────────────────────────────────────

/// POST /admin/network-config/export.
#[derive(Debug, Deserialize)]
pub struct NetCfgExportBody {
    pub store_id: String,
    /// Повний URL фасаду мережі, напр. "https://100.64.0.5:8000".
    /// Не задано → env TORGASHKA_FACADE_ADDR → "http://127.0.0.1:8000".
    #[serde(default)]
    pub server_url: Option<String>,
    /// true → додати db.password_encrypted (AES-256-GCM текст з db_sources.toml).
    #[serde(default)]
    pub include_db_password: Option<bool>,
}

/// 200: файл повертається рядком content (готовий для збереження USB/обмінником).
#[derive(Debug, Serialize)]
pub struct NetCfgExportResponse {
    pub filename: String,
    pub content: String,
}

/// POST /admin/network-config/import.
#[derive(Debug, Deserialize)]
pub struct NetCfgImportBody {
    /// Весь JSON конфіг-файлу як рядок.
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct NetCfgImportResponse {
    pub ok: bool,
    pub network_id: Uuid,
    pub store: NetCfgImportStore,
    pub server_url: String,
}

#[derive(Debug, Serialize)]
pub struct NetCfgImportStore {
    pub id: Uuid,
    pub name: String,
}

// ─── Чисті хелпери ──────────────────────────────────────────────────────────

/// Slug назви точки для імені файлу: ASCII-lowercase, не-алфавітно-цифрові
/// символи → '-', без подвійних/крайових дефісів. Пусто → "store".
fn store_slug(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !slug.is_empty() && !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "store".to_string()
    } else {
        slug
    }
}

/// Нормалізує URL фасаду: без схеми → додає "http://".
fn normalize_server_url(raw: &str) -> String {
    let t = raw.trim();
    if t.contains("://") {
        t.to_string()
    } else {
        format!("http://{t}")
    }
}

/// Пріоритет server_url: явний (body) → env TORGASHKA_FACADE_ADDR → дефолт.
fn resolve_server_url(explicit: Option<&str>) -> String {
    if let Some(v) = explicit {
        if !v.trim().is_empty() {
            return normalize_server_url(v);
        }
    }
    if let Ok(v) = std::env::var("TORGASHKA_FACADE_ADDR") {
        if !v.trim().is_empty() {
            return normalize_server_url(&v);
        }
    }
    DEFAULT_SERVER_URL.to_string()
}

/// Активне джерело з db_sources.toml → db-блок конфіга (None — джерела немає).
/// password_encrypted включається лише за include_pw.
fn active_db_block(include_pw: bool) -> Result<Option<NetCfgDb>, NetCfgErr> {
    let Some(cfg) = db_sources::load()? else {
        return Ok(None);
    };
    let Some(active_id) = cfg.active.as_deref() else {
        return Ok(None);
    };
    let Some((_, src)) = cfg.sources.iter().find(|(id, _)| id == active_id) else {
        return Ok(None);
    };
    Ok(Some(NetCfgDb {
        host: src.host.clone(),
        port: src.port,
        database: src.database.clone(),
        user: src.user.clone(),
        password_encrypted: if include_pw {
            src.password_encrypted.clone()
        } else {
            None
        },
    }))
}

// ─── POST /api/v1/admin/network-config/export ───────────────────────────────

pub async fn export_config(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<NetCfgExportBody>,
) -> Result<Json<NetCfgExportResponse>, NetCfgErr> {
    let pool = pool(&state)?;
    let owner_id = auth_routes::require_owner(&state, &claims).await?;

    let store_id = Uuid::parse_str(body.store_id.trim()).map_err(|_| {
        NetCfgErr::BadRequest(format!(
            "Невірний store_id '{}' — очікується UUID",
            body.store_id
        ))
    })?;

    // Точка має існувати (404).
    let store_name: String = sqlx::query_scalar("SELECT name FROM stores WHERE id = $1")
        .bind(store_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| NetCfgErr::NotFound(format!("Точку {store_id} не знайдено")))?;

    // (Пере)генерація коду активації точки — спільний хелпер network.rs.
    let code = network::ensure_activation_code(&pool, store_id, owner_id).await?;

    // network_id = власник мережі (перший owner у БД — єдиний у реальній мережі).
    let network_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE role = 'owner'::public.user_role \
         ORDER BY created_at, id LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?;
    let network_id = network_id.ok_or_else(|| {
        NetCfgErr::Internal("Власника мережі (role=owner) не знайдено".to_string())
    })?;

    let server_url = resolve_server_url(body.server_url.as_deref());
    let include_pw = body.include_db_password.unwrap_or(false);
    let db_block = active_db_block(include_pw)?;

    let exported_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let date = &exported_at[..10]; // YYYY-MM-DD → YYYYmmdd
    let date_compact = date.replace('-', "");
    let filename = format!(
        "torgashka-network-{}-{date_compact}.json",
        store_slug(&store_name)
    );

    let file = NetCfgFile {
        schema_version: SCHEMA_VERSION,
        network_id,
        server_url: server_url.clone(),
        exported_at,
        store: NetCfgStore {
            id: store_id,
            name: store_name.clone(),
            activation_code: code,
        },
        db: db_block,
    };
    let content = serde_json::to_string(&file)
        .map_err(|e| NetCfgErr::Internal(format!("не вдалося серіалізувати конфіг: {e}")))?;

    // Аудит: без секретів (пароль/код не пишемо в audit_log payload).
    network::audit(
        &pool,
        owner_id,
        "network_config_exported",
        "store",
        store_id,
        Some(store_id),
        serde_json::json!({
            "store_name": store_name,
            "has_db": file.db.is_some(),
            "include_db_password": include_pw,
            "filename": filename,
        }),
    )
    .await;

    Ok(Json(NetCfgExportResponse { filename, content }))
}

// ─── POST /api/v1/admin/network-config/import ───────────────────────────────

/// Вхідний конфіг-файл (розбір для валідації; db-блок у файлі ігнорується —
/// сервер не приймає конфігурацію підключення ззовні).
#[derive(Debug, Deserialize)]
struct NetCfgFileIn {
    network_id: String,
    server_url: String,
    store: NetCfgStoreIn,
}

#[derive(Debug, Deserialize)]
struct NetCfgStoreIn {
    id: String,
    activation_code: String,
}

pub async fn import_config(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<NetCfgImportBody>,
) -> Result<Json<NetCfgImportResponse>, NetCfgErr> {
    let pool = pool(&state)?;
    let _owner_id = auth_routes::require_owner(&state, &claims).await?;

    let content = body.content.trim();
    if content.is_empty() {
        return Err(NetCfgErr::BadRequest(
            "content не може бути порожнім".to_string(),
        ));
    }
    if content.len() > 1_000_000 {
        return Err(NetCfgErr::BadRequest(
            "content завеликий (ліміт 1 МБ)".to_string(),
        ));
    }
    // Спершу — валідний JSON + schema_version (зрозумілі 400).
    let v: Value = serde_json::from_str(content)
        .map_err(|e| NetCfgErr::BadRequest(format!("Невірний JSON у content: {e}")))?;
    if v.get("schema_version").and_then(Value::as_u64) != Some(SCHEMA_VERSION as u64) {
        return Err(NetCfgErr::BadRequest(
            "Непідтримувана schema_version — очікується 1".to_string(),
        ));
    }
    let f: NetCfgFileIn = serde_json::from_value(v)
        .map_err(|e| NetCfgErr::BadRequest(format!("Невірний формат конфіг-файлу мережі: {e}")))?;

    let store_id = Uuid::parse_str(f.store.id.trim()).map_err(|_| {
        NetCfgErr::BadRequest("Невірний store.id у конфіг-файлі — очікується UUID".to_string())
    })?;
    let network_id = Uuid::parse_str(f.network_id.trim()).map_err(|_| {
        NetCfgErr::BadRequest("Невірний network_id у конфіг-файлі — очікується UUID".to_string())
    })?;
    let server_url = f.server_url.trim().to_string();
    if server_url.is_empty() || server_url.len() > 512 {
        return Err(NetCfgErr::BadRequest(
            "server_url у конфіг-файлі порожній або задовгий".to_string(),
        ));
    }
    let code = f.store.activation_code.trim().to_uppercase();
    if code.is_empty() || code.len() > 9 {
        return Err(NetCfgErr::BadRequest(
            "Невірний activation_code у конфіг-файлі (очікується до 9 символів A-Z0-9)".to_string(),
        ));
    }

    // Точка з файлу має існувати (404).
    let db_name: String = sqlx::query_scalar("SELECT name FROM stores WHERE id = $1")
        .bind(store_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| NetCfgErr::NotFound(format!("Точку {store_id} не знайдено")))?;

    // Код активації має належати точці з файлу (400).
    let code_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM store_activation_codes WHERE code = $1 AND store_id = $2)",
    )
    .bind(&code)
    .bind(store_id)
    .fetch_one(&pool)
    .await?;
    if !code_ok {
        return Err(NetCfgErr::BadRequest(
            "Код активації не належить вказаній точці".to_string(),
        ));
    }

    network::audit(
        &pool,
        _owner_id,
        "network_config_imported",
        "store",
        store_id,
        Some(store_id),
        serde_json::json!({
            "store_name": db_name,
            "network_id": network_id,
            "server_url": server_url,
        }),
    )
    .await;

    Ok(Json(NetCfgImportResponse {
        ok: true,
        network_id,
        store: NetCfgImportStore {
            id: store_id,
            name: db_name,
        },
        server_url,
    }))
}

// ─── Unit-тести чистих хелперів ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_ascii_lowercase_dashes() {
        assert_eq!(store_slug("Білий магазин"), "store"); // кирилиця → пусто → "store"
        assert_eq!(store_slug("Magazin 1"), "magazin-1");
        assert_eq!(store_slug("  Cafe-Bar! "), "cafe-bar");
        assert_eq!(store_slug("---"), "store");
    }

    #[test]
    fn normalize_url_adds_scheme() {
        assert_eq!(
            normalize_server_url("100.64.0.5:8000"),
            "http://100.64.0.5:8000"
        );
        assert_eq!(
            normalize_server_url("https://vpn.example:8443"),
            "https://vpn.example:8443"
        );
        assert_eq!(
            normalize_server_url("127.0.0.1:8000"),
            "http://127.0.0.1:8000"
        );
    }

    #[test]
    fn server_url_priority_explicit_env_default() {
        // explicit wins
        assert_eq!(resolve_server_url(Some("https://x:8000")), "https://x:8000");
        // env wins over default
        std::env::set_var("TORGASHKA_FACADE_ADDR", "10.0.0.9:9000");
        assert_eq!(resolve_server_url(None), "http://10.0.0.9:9000");
        // explicit (even empty-string-like whitespace) still overrides env
        assert_eq!(resolve_server_url(Some("  ")), "http://10.0.0.9:9000");
        std::env::remove_var("TORGASHKA_FACADE_ADDR");
        assert_eq!(resolve_server_url(None), DEFAULT_SERVER_URL);
    }
}
