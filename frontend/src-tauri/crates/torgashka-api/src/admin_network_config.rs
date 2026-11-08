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
// Конфіг-файл (schema_version=2) — UTF-8 JSON, придатний для передачі
// USB/файлообмінником:
//   {
//     "schema_version": 2,
//     "network_id": "<uuid власника мережі>",
//     "server_url": "http://127.0.0.1:8000",
//     "exported_at": "2026-08-06T12:00:00Z",
//     "store": { "id": "<uuid>", "name": "...", "activation_code": "ABCD2345" },
//     "db": { "host", "port", "database", "user", "password_encrypted"?, "key_b64"? }
//   }
// db.key_b64 — AES-ключ (base64, 32 байти), яким зашифровано password_encrypted
// у db_sources.toml: файл САМОДОСТАТНІЙ для переносу (v2). Пара
// password_encrypted+key_b64 вкладається лише за include_db_password=true;
// ключ недоступний → помилка (файл без ключа не віддається).
// Сумісність версій: import приймає schema_version 1 (старі файли пристроїв,
// без db.key_b64) і 2 (поточний); непідтримувана/відсутня/нечислова → 400.
// export завжди пише поточну 2.
// Безпека (Етап 5):
//   - лише role=owner (require_owner у auth_routes, спільний БЕЗ дублів);
//   - password_encrypted — AES-256-GCM-захищений текст (db_sources.toml),
//     включається ЛИШЕ за include_db_password=true; plaintext ніколи;
//   - key_b64 у audit_log НЕ потрапляє (payload не розширюється: див. export);
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
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
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
    // Імпорт/експорт мережевого конфігу торкається реєстру точок (`stores`)
    // у ВЛАСНІЙ БД вузла.
    state.write_pool_or_err().map_err(NetCfgErr::Internal)
}

// ─── Модель конфіг-файлу мережі (schema_version=2) ──────────────────────────

const SCHEMA_VERSION: u32 = 2;
/// Версії конфіг-файлу, які ПРИЙМАЄ import_config — включно зі СТАРИМИ файлами
/// пристроїв: 1 — без db.key_b64; 2 — поточний, самодостатній (із ключем БД).
/// export завжди пише поточну SCHEMA_VERSION.
const SUPPORTED_SCHEMA_VERSIONS: [u64; 2] = [1, 2];
/// Дефолтний server_url (коли ні body.server_url, ні env не задано).
const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8000";

#[derive(Debug, Serialize, Deserialize)]
struct NetCfgStore {
    id: Uuid,
    name: String,
    activation_code: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NetCfgDb {
    host: String,
    port: u16,
    database: String,
    user: String,
    /// AES-256-GCM-захищений пароль (як у db_sources.toml); ЛИШЕ якщо
    /// include_db_password=true. Ніколи plaintext.
    #[serde(skip_serializing_if = "Option::is_none")]
    password_encrypted: Option<String>,
    /// AES-ключ (base64 32 байти), яким зашифровано `password_encrypted` —
    /// той самий, що в db_sources.toml (.dbkey / env TORGASHKA_DBKEY).
    /// Вкладається ЛИШЕ разом із password_encrypted (v2, самодостатній файл);
    /// у старому файлі (schema_version=1) поля немає → None (сумісність).
    #[serde(skip_serializing_if = "Option::is_none")]
    key_b64: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
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

/// Чиста частина [`active_db_block`]: активне джерело з УЖЕ завантаженого
/// конфігу → db-блок конфіг-файлу (None — активного джерела немає).
///
/// `include_pw=true` вкладає ПАРУ `password_encrypted` + `key_b64` (той самий
/// ключ, яким пароль зашифровано у `db_sources.toml`) — інакше файл не був би
/// самодостатнім на цільовому пристрої. Плейнтекст не потрапляє НІКОЛИ.
/// Пароль зашифровано, а ключ недоступний (немає env/.dbkey) → **помилка**:
/// віддати файл без ключа або з плейнтекстом заборонено.
fn db_block_from(
    cfg: &db_sources::DbSourcesFile,
    cfg_path: &Path,
    include_pw: bool,
) -> Result<Option<NetCfgDb>, NetCfgErr> {
    let Some(active_id) = cfg.active.as_deref() else {
        return Ok(None);
    };
    let Some((_, src)) = cfg.sources.iter().find(|(id, _)| id == active_id) else {
        return Ok(None);
    };

    let mut password_encrypted = None;
    let mut key_b64 = None;
    if include_pw {
        if let Some(enc) = src.password_encrypted.as_deref().filter(|s| !s.is_empty()) {
            // Текст DbSourcesError СВІДОМО не переповідаємо: KeyMissing у
            // db_sources.rs формулює «ключ не валідний hex: {кандидат}» і міг би
            // винести матеріал ключа у відповідь/лог. Тут — лише безпечний текст.
            let key = db_sources::resolve_key(cfg_path).map_err(|_| {
                NetCfgErr::Internal(format!(
                    "пароль активного джерела зашифровано, але AES-ключ недоступний \
                     (немає/не читається .dbkey поруч із db_sources.toml, а env {} \
                     не задано або невірний). Покладіть .dbkey (base64 32 байти) \
                     поруч із db_sources.toml або задайте {}, чи експортуйте конфіг \
                     без пароля (include_db_password=false)",
                    db_sources::DBKEY_ENV,
                    db_sources::DBKEY_ENV
                ))
            })?;
            password_encrypted = Some(enc.to_string());
            key_b64 = Some(B64.encode(key));
        }
    }

    Ok(Some(NetCfgDb {
        host: src.host.clone(),
        port: src.port,
        database: src.database.clone(),
        user: src.user.clone(),
        password_encrypted,
        key_b64,
    }))
}

/// Активне джерело з db_sources.toml → db-блок конфіга (None — джерела немає).
/// password_encrypted (і key_b64) включаються лише за include_pw.
fn active_db_block(include_pw: bool) -> Result<Option<NetCfgDb>, NetCfgErr> {
    let Some(cfg) = db_sources::load()? else {
        return Ok(None);
    };
    let cfg_path = db_sources::existing_path().unwrap_or_else(db_sources::write_path);
    db_block_from(&cfg, &cfg_path, include_pw)
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
/// сервер не приймає конфігурацію підключення ззовні). Номер schema_version
/// перевіряє ґейт import_config: приймаються 1 (старі файли пристроїв) і 2.
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
    // Приймаються ВСІ підтримувані версії (1 — старі файли пристроїв,
    // 2 — поточний): сумісність важливіша за строгість. Відсутній, нечисловий
    // або невідомий номер → 400, причому у тексті — ФАКТИЧНЕ значення з файлу.
    let schema_raw = v.get("schema_version");
    if !schema_raw
        .and_then(Value::as_u64)
        .is_some_and(|ver| SUPPORTED_SCHEMA_VERSIONS.contains(&ver))
    {
        let shown = match schema_raw {
            Some(raw) => raw.to_string(),
            None => "відсутня".to_string(),
        };
        return Err(NetCfgErr::BadRequest(format!(
            "Непідтримувана schema_version={shown} — підтримуються 1 (старі файли пристроїв) і 2 (з ключем БД)"
        )));
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
    use std::path::PathBuf;

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

    // ─── db-блок: пароль + ключ (schema_version=2, самодостатній файл) ────────

    /// Env `TORGASHKA_DBKEY` — процес-глобальний, тому ВСІ тести, що ходять у
    /// ключ, тримають цей м'ютекс і на час тесту явно вимикають env: тоді ключ
    /// детерміновано береться з `.dbkey` поряд із конфігом (як на пристрої).
    static KEY_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Вимикає env-ключ на час тесту; на виході (Drop) відновлює як було.
    struct KeyEnvOff(Option<String>);

    impl KeyEnvOff {
        fn new() -> Self {
            let prev = std::env::var(db_sources::DBKEY_ENV).ok();
            std::env::remove_var(db_sources::DBKEY_ENV);
            Self(prev)
        }
    }

    impl Drop for KeyEnvOff {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var(db_sources::DBKEY_ENV, v),
                None => std::env::remove_var(db_sources::DBKEY_ENV),
            }
        }
    }

    fn lock_key_env() -> (std::sync::MutexGuard<'static, ()>, KeyEnvOff) {
        let g = KEY_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let e = KeyEnvOff::new();
        (g, e)
    }

    fn source_with(password_encrypted: Option<String>) -> db_sources::DbSource {
        db_sources::DbSource {
            label: Some("тест".to_string()),
            host: "127.0.0.1".to_string(),
            port: 55432,
            database: "shop".to_string(),
            user: "shop".to_string(),
            password_encrypted,
            status: None,
        }
    }

    /// `db_sources.toml` з одним активним джерелом. Пароль шифрується СПРАВЖНІМ
    /// ключем (у каталозі з'являється `.dbkey`) — тим самим, який має забрати export.
    fn cfg_file_with_password(
        dir: &Path,
        plain: Option<&str>,
    ) -> (PathBuf, db_sources::DbSourcesFile) {
        let cfg_path = dir.join("db_sources.toml");
        let enc = plain.map(|p| db_sources::encrypt_password(&cfg_path, p).expect("шифрування"));
        let cfg = db_sources::DbSourcesFile {
            active: Some("main".to_string()),
            sources: vec![("main".to_string(), source_with(enc))],
        };
        db_sources::save_to(&cfg_path, &cfg).expect("запис db_sources.toml");
        (cfg_path, cfg)
    }

    #[test]
    fn export_block_has_password_and_key_only_with_include_pw() {
        let (_sync, _env) = lock_key_env();
        let dir = tempfile::tempdir().unwrap();
        let (cfg_path, cfg) = cfg_file_with_password(dir.path(), Some("s3cret"));

        let on = db_block_from(&cfg, &cfg_path, true)
            .unwrap()
            .expect("db-блок");
        // password_encrypted у файлі — БАЙТ-У-БАЙТ той самий, що в db_sources.toml.
        let enc_in_toml = cfg.sources[0].1.password_encrypted.clone().unwrap();
        assert_eq!(on.password_encrypted.as_deref(), Some(enc_in_toml.as_str()));

        let json = serde_json::to_value(&on).unwrap();
        assert!(
            json.get("password_encrypted").is_some(),
            "include_pw=true → пароль є"
        );
        assert!(json.get("key_b64").is_some(), "include_pw=true → ключ є");
        let raw = serde_json::to_string(&on).unwrap();
        assert!(!raw.contains("s3cret"), "плейнтексту у файлі немає: {raw}");

        let off = db_block_from(&cfg, &cfg_path, false).unwrap().unwrap();
        let off_json = serde_json::to_value(&off).unwrap();
        assert!(
            off_json.get("password_encrypted").is_none(),
            "без include_pw пароля немає"
        );
        assert!(
            off_json.get("key_b64").is_none(),
            "без include_pw ключа немає"
        );
        // решта полів блоку не змінилась
        assert_eq!(off_json["host"], "127.0.0.1");
        assert_eq!(off_json["port"], 55432);
        assert_eq!(off_json["database"], "shop");
        assert_eq!(off_json["user"], "shop");
    }

    #[test]
    fn exported_key_decodes_to_same_32_bytes_and_decrypts_password() {
        let (_sync, _env) = lock_key_env();
        let dir = tempfile::tempdir().unwrap();
        let (cfg_path, cfg) = cfg_file_with_password(dir.path(), Some("s3cret"));
        let block = db_block_from(&cfg, &cfg_path, true).unwrap().unwrap();
        let key_b64 = block.key_b64.clone().expect("key_b64 у файлі");

        // «Цільовий пристрій»: порожній env + `.dbkey` із рядком із файла.
        let target = dir.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        let target_cfg = target.join("db_sources.toml");
        std::fs::write(&target_cfg, db_sources::to_string(&cfg).unwrap()).unwrap();
        std::fs::write(target.join(".dbkey"), &key_b64).unwrap();

        // (1) той САМИЙ ключ: decode_key_str приймає рядок і дає РІВНО 32 байти.
        let src_key = db_sources::resolve_key(&cfg_path).unwrap();
        let decoded = db_sources::resolve_key(&target_cfg).unwrap();
        assert_eq!(
            decoded, src_key,
            "key_b64 з файла == ключ активного джерела"
        );
        assert_eq!(decoded.len(), 32);
        // форма base64: unpadded 43 симв. або padded 44 (обидві їсть decode_key_str)
        assert!(
            key_b64.len() == 43 || key_b64.len() == 44,
            "len={}",
            key_b64.len()
        );

        // (2) фактичний AES-256-GCM: ключ із файла розшифровує пароль із файла.
        let plain =
            db_sources::decrypt_password(&target_cfg, block.password_encrypted.as_deref().unwrap())
                .expect("decrypt_password");
        assert_eq!(plain, "s3cret");

        // (3) для контролю: без ключа (інший каталог) той самий текст не читається.
        let broken = dir.path().join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        assert!(db_sources::resolve_key(&broken.join("db_sources.toml")).is_err());
    }

    #[test]
    fn include_pw_without_encrypted_password_is_ok_and_keyless() {
        let (_sync, _env) = lock_key_env();
        let dir = tempfile::tempdir().unwrap();
        let (cfg_path, cfg) = cfg_file_with_password(dir.path(), None);
        assert!(
            !dir.path().join(".dbkey").exists(),
            "ключа не існує взагалі"
        );

        let block = db_block_from(&cfg, &cfg_path, true)
            .unwrap()
            .expect("db-блок");
        assert!(block.password_encrypted.is_none());
        assert!(block.key_b64.is_none(), "немає пароля → немає ключа");
    }

    #[test]
    fn include_pw_without_key_is_error_not_silent_file() {
        let (_sync, _env) = lock_key_env();
        let dir = tempfile::tempdir().unwrap();
        // Пароль зашифровано В ІНШОМУ каталозі (там є .dbkey), а конфіг, з якого
        // експортуємо, лежить у каталозі БЕЗ ключа → ключ недоступний.
        let enc_dir = dir.path().join("enc");
        std::fs::create_dir_all(&enc_dir).unwrap();
        let enc = db_sources::encrypt_password(&enc_dir.join("db_sources.toml"), "s3cret").unwrap();
        let cfg_path = dir.path().join("no-key").join("db_sources.toml");
        std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        let cfg = db_sources::DbSourcesFile {
            active: Some("main".to_string()),
            sources: vec![("main".to_string(), source_with(Some(enc)))],
        };
        assert!(
            !dir.path().join("no-key/.dbkey").exists(),
            "ключа у каталозі немає"
        );
        assert!(db_sources::resolve_key(&cfg_path).is_err(), "ключа немає");

        match db_block_from(&cfg, &cfg_path, true) {
            Err(NetCfgErr::Internal(msg)) => {
                assert!(msg.contains("AES-ключ недоступний"), "msg={msg}");
                assert!(
                    msg.contains(db_sources::DBKEY_ENV),
                    "підказка про env: {msg}"
                );
                assert!(
                    !msg.contains("s3cret"),
                    "секрет не тече в текст помилки: {msg}"
                );
            }
            other => panic!("очікували Err(Internal), отримали {other:?}"),
        }

        // Без пароля ключ не потрібен — експорт без include_db_password працює.
        let off = db_block_from(&cfg, &cfg_path, false)
            .unwrap()
            .expect("db-блок");
        assert!(off.password_encrypted.is_none() && off.key_b64.is_none());
    }

    // ─── Сумісність: файли schema_version=1 на пристроях ─────────────────────

    /// Реальний v1-файл (з db-блоком, але БЕЗ key_b64) — як на пристроях зараз.
    fn v1_file_json() -> String {
        serde_json::json!({
            "schema_version": 1,
            "network_id": "11111111-1111-1111-1111-111111111111",
            "server_url": "http://127.0.0.1:8000",
            "exported_at": "2026-08-06T12:00:00Z",
            "store": {
                "id": "22222222-2222-2222-2222-222222222222",
                "name": "Магазин 1",
                "activation_code": "ABCD2345"
            },
            "db": {
                "host": "127.0.0.1", "port": 5432, "database": "shop", "user": "shop",
                "password_encrypted": "AAAA"
            }
        })
        .to_string()
    }

    #[test]
    fn v1_file_without_key_b64_still_deserializes() {
        let f: NetCfgFile = serde_json::from_str(&v1_file_json()).expect("v1 → NetCfgFile");
        assert_eq!(f.schema_version, 1);
        assert_eq!(f.store.activation_code, "ABCD2345");
        let db = f.db.expect("db-блок є");
        assert!(
            db.key_b64.is_none(),
            "у v1 ключа не було → None, не помилка"
        );
        assert_eq!(db.password_encrypted.as_deref(), Some("AAAA"));

        // v1 без db-блоку (експорт без include_db_password)
        let no_db = serde_json::json!({
            "schema_version": 1,
            "network_id": "11111111-1111-1111-1111-111111111111",
            "server_url": "http://127.0.0.1:8000",
            "exported_at": "2026-08-06T12:00:00Z",
            "store": {
                "id": "22222222-2222-2222-2222-222222222222",
                "name": "Магазин 1",
                "activation_code": "ABCD2345"
            }
        })
        .to_string();
        let f2: NetCfgFile = serde_json::from_str(&no_db).expect("v1 без db → NetCfgFile");
        assert!(f2.db.is_none());

        // db-блок без пароля (джерело без password_encrypted)
        let no_pw = serde_json::json!({
            "schema_version": 1,
            "network_id": "11111111-1111-1111-1111-111111111111",
            "server_url": "http://127.0.0.1:8000",
            "exported_at": "2026-08-06T12:00:00Z",
            "store": {
                "id": "22222222-2222-2222-2222-222222222222",
                "name": "Магазин 1",
                "activation_code": "ABCD2345"
            },
            "db": { "host": "h", "port": 1, "database": "d", "user": "u" }
        })
        .to_string();
        let f3: NetCfgFile = serde_json::from_str(&no_pw).expect("db без пароля → NetCfgFile");
        let db3 = f3.db.expect("db-блок");
        assert!(db3.password_encrypted.is_none() && db3.key_b64.is_none());
    }

    #[test]
    fn v1_and_v2_files_parse_as_import_input() {
        // Той самий шлях, що в import_config: Value → NetCfgFileIn.
        let v1: Value = serde_json::from_str(&v1_file_json()).unwrap();
        let f1: NetCfgFileIn = serde_json::from_value(v1).expect("v1 → NetCfgFileIn");
        assert_eq!(f1.store.activation_code, "ABCD2345");
        assert_eq!(f1.server_url, "http://127.0.0.1:8000");

        // v2 з ПОВНИМ db-блоком (password_encrypted + key_b64): невідомі поля
        // (schema_version, db, key_b64) ігноруються, формат читається.
        let v2 = serde_json::json!({
            "schema_version": 2,
            "network_id": "11111111-1111-1111-1111-111111111111",
            "server_url": "http://127.0.0.1:8000",
            "exported_at": "2026-09-12T10:00:00Z",
            "store": {
                "id": "22222222-2222-2222-2222-222222222222",
                "name": "Магазин 1",
                "activation_code": "WXYZ6789"
            },
            "db": {
                "host": "10.0.0.5", "port": 5432, "database": "shop", "user": "shop",
                "password_encrypted": "ZZZ", "key_b64": "KEYSECRET"
            }
        });
        let f2: NetCfgFileIn = serde_json::from_value(v2).expect("v2 → NetCfgFileIn");
        assert_eq!(f2.store.activation_code, "WXYZ6789");
        assert_eq!(f2.store.id, "22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn exported_file_declares_schema_version_2_and_roundtrips() {
        let f = NetCfgFile {
            schema_version: SCHEMA_VERSION,
            network_id: Uuid::nil(),
            server_url: "http://127.0.0.1:8000".to_string(),
            exported_at: "2026-09-12T10:00:00Z".to_string(),
            store: NetCfgStore {
                id: Uuid::nil(),
                name: "M".to_string(),
                activation_code: "A".to_string(),
            },
            db: None,
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["schema_version"], 2);
        assert!(
            v.get("db").is_none(),
            "db відсутній → поле не серіалізується"
        );
        // round-trip власного v2-файлу з ключем
        let with_db = NetCfgFile {
            db: Some(NetCfgDb {
                host: "h".to_string(),
                port: 5432,
                database: "d".to_string(),
                user: "u".to_string(),
                password_encrypted: Some("enc".to_string()),
                key_b64: Some("key".to_string()),
            }),
            ..f
        };
        let s = serde_json::to_string(&with_db).unwrap();
        let back: NetCfgFile = serde_json::from_str(&s).unwrap();
        let db = back.db.expect("db");
        assert_eq!(db.key_b64.as_deref(), Some("key"));
        assert_eq!(db.password_encrypted.as_deref(), Some("enc"));
        assert_eq!(back.schema_version, 2);
    }
}
