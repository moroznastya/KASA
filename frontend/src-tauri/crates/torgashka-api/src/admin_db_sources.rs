// ─────────────────────────────────────────────────────────────────────────────
// admin_db_sources — «Джерело даних» (Етап 3 адмін-панелі власника мережі,
// ТЗ розділи 2.4 / 5.8)
// ─────────────────────────────────────────────────────────────────────────────
// Роути (окремий /admin/* роутер БЕЗ store_middleware; RBAC owner|store_manager
// |admin через auth_routes::require_admin, як admin.rs):
//
//   GET    /api/v1/admin/db-sources                     → { active, config_path, sources[] }
//   POST   /api/v1/admin/db-sources                     → створити джерело
//   PUT    /api/v1/admin/db-sources/:id                 → редагувати (пароль — опційно)
//   DELETE /api/v1/admin/db-sources/:id                 → видалити (НЕ активне; 409 якщо active)
//   POST   /api/v1/admin/db-sources/:id/test            → реальний пінг (TCP + SELECT 1)
//   POST   /api/v1/admin/db-sources/:id/activate        → test + зберегти active
//   POST   /api/v1/admin/db-sources/export-dump         → pg_dump активної (або вказаної) БД (plain SQL)
//   GET    /api/v1/admin/db-sources/dumps               → список дампів (для імпорту)
//   POST   /api/v1/admin/db-sources/import-dump         → psql у вибране джерело
//
// Конфігурація: db_sources.toml (0600) — torgashka_infrastructure::db_sources.
// Пароль зберігається ЛИШЕ шифрованим (AES-256-GCM, ключ TORGASHKA_DBKEY/.dbkey).
//
// ⚠️ SWITCH БЕЗ ГОРЯЧОГО ПЕРЕПІДКЛЮЧЕННЯ (stability_first, рішення зафіксоване):
// пули фасаду створюються один раз при serve_listener (кожен сервіс — окремий
// пул/репозиторій в AppState); атомарно замінити їх у рантаймі неможливо без
// ризику. Тому activate: (1) реально перевіряє з'єднання з джерелом, (2) пише
// active у db_sources.toml, (3) повертає applied_immediately=false з чесною
// відповіддю «застосується після перезапуску». Жоден існуючий роут не чіпає
// робочий пул. resolve_database_url() при старті підхоплює активне джерело.
//
// pg_dump/psql запускаються через tokio::process (пароль — лише в env PGPASSWORD
// дочірнього процесу, не в argv). За відсутності бінарників у PATH — зрозуміла
// помилка (не імітація).
//
// ⚠️ Формат дампу — plain SQL (-Fp), імпорт — psql з ПЕРЕД-обробкою:
// pg_dump >=17 пише у дамп `SET transaction_timeout = 0` (GUC з PG17) — на
// сервері PG16 pg_restore падає з "unrecognized configuration parameter".
// Пресанітайз коментує цей SET i рядки `\restrict` (psql 17) — дамп
// імпортується на будь-яку версію. Після psql — обов'язкова ПЕРЕВІРКА схеми
// (users/stores існують): psql без ON_ERROR_STOP не сигналізує помилки
// exit-кодом, тому успіх підтверджується перевіркою таблиць.
// ─────────────────────────────────────────────────────────────────────────────
use std::path::PathBuf;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;

use torgashka_infrastructure::db_sources::{
    self, build_url, decrypt_password, encrypt_password, DbSource, DbSourcesError,
};
use torgashka_infrastructure::provision::{self, ProvisionError};

use crate::{auth::Claims, auth_routes, AppState};

// ─── Помилки → HTTP ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum DbSrcErr {
    Auth(auth_routes::AuthRouteError),
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl From<auth_routes::AuthRouteError> for DbSrcErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        DbSrcErr::Auth(e)
    }
}

impl From<DbSourcesError> for DbSrcErr {
    fn from(e: DbSourcesError) -> Self {
        DbSrcErr::BadRequest(e.to_string())
    }
}

impl From<ProvisionError> for DbSrcErr {
    fn from(e: ProvisionError) -> Self {
        match e {
            ProvisionError::DatabaseExists(db) => DbSrcErr::Conflict(format!(
                "База даних '{db}' уже існує на цільовому сервері — виберіть інше ім'я"
            )),
            // Санація (ADR-0007 §D): `ProvisionError` несе `reason`/`stderr`
            // psql — сирий текст інструментів; клієнту — людський
            // текст, причина — у stderr-лог. Статус 400 не змінюється.
            other => {
                eprintln!("[torgashka-api] db-sources: provision: {other}");
                DbSrcErr::BadRequest(provision_human(&other))
            }
        }
    }
}

/// Людський текст для помилок провіжну (без `reason`/`stderr` інструментів).
fn provision_human(e: &ProvisionError) -> String {
    match e {
        ProvisionError::DatabaseExists(db) => {
            format!("База даних '{db}' уже існує на цільовому сервері — виберіть інше ім'я")
        }
        ProvisionError::Connect { host, port, .. } => format!(
            "Не вдалося підключитись до {host}:{port} суперкористувачем [DB_ERROR] — \
             перевірте host/port/логін/пароль (подробиці в логі сервісу)"
        ),
        ProvisionError::Failed(_) => "Не вдалося створити базу даних на цільовому сервері \
             [DB_ERROR] — подробиці в логі сервісу"
            .to_string(),
    }
}

impl IntoResponse for DbSrcErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            DbSrcErr::Auth(e) => e.into_response(),
            DbSrcErr::BadRequest(m) => body(StatusCode::BAD_REQUEST, m),
            DbSrcErr::NotFound(m) => body(StatusCode::NOT_FOUND, m),
            DbSrcErr::Conflict(m) => body(StatusCode::CONFLICT, m),
            DbSrcErr::Internal(m) => {
                eprintln!("[torgashka-api] db-sources: помилка: {m}");
                body(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Внутрішня помилка сервера".to_string(),
                )
            }
        }
    }
}

// ─── DTO ─────────────────────────────────────────────────────────────────────

/// Чистий view джерела (API ніколи не віддає password_encrypted).
#[derive(Debug, Serialize)]
pub struct DbSourceDto {
    pub id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub has_password: bool,
    pub is_active: bool,
    /// Статус провіжинінгу: Some("provisioned_pending_activation") = БД створено
    /// через /provision, але джерело ще НЕ активоване власником. None = звичайне.
    pub status: Option<String>,
}

/// POST /db-sources — створення. `id` — ключ таблиці [sources.<id>] у toml.
#[derive(Debug, Deserialize)]
pub struct DbSourceCreate {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// Пароль у plaintext приходить з UI по LAN/localhost; у файл НЕ пишеться.
    #[serde(default)]
    pub password: Option<String>,
}

/// PUT /db-sources/:id — часткове оновлення (None-поле = без змін).
#[derive(Debug, Deserialize, Default)]
pub struct DbSourceUpdate {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    /// Some("") = очистити пароль; Some(x) = перешифрувати; None = без змін.
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExportDumpBody {
    /// Якщо не задано — дамп активного джерела.
    #[serde(default)]
    pub source_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportDumpBody {
    /// Джерело-приймач (БД має існувати; для нової БД — спершу створити її
    /// на сервері та додати джерело через POST /db-sources).
    pub source_id: String,
    /// Ім'я файлу зі списку GET /db-sources/dumps (лише basename — без шляхів).
    pub file: String,
    /// true → pg_restore --clean --if-exists (очистити об'єкти БД призначення
    /// перед відновленням). Небезпечно: застосовується лише за явним запитом.
    #[serde(default)]
    pub clean: bool,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub active: Option<String>,
    pub config_path: String,
    pub sources: Vec<DbSourceDto>,
}

#[derive(Debug, Serialize)]
pub struct TestResponse {
    pub ok: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct ActivateResponse {
    pub active: String,
    /// Гаряче перепідключення пулів НЕ реалізовано (stability_first): активне
    /// джерело застосується при наступному старті сервісу.
    pub applied_immediately: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct DumpInfo {
    pub file: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

#[derive(Debug, Serialize)]
pub struct ExportResponse {
    pub file: String,
    pub path: String,
    pub size_bytes: u64,
    pub source_id: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub ok: bool,
    pub source_id: String,
    pub file: String,
}

/// Одноразові суперкористувацькі кредити (провіжинінг). НЕ зберігаються,
/// НЕ логуються, НЕ включаються у відповідь.
#[derive(Debug, Deserialize)]
pub struct ProvisionSuperuser {
    pub user: String,
    pub password: String,
}

/// POST /admin/db-sources/provision — створення НОВОЇ БД на цільовому кластері
/// + повна схема + роль torgashka_app (див. torgashka_infrastructure::provision).
#[derive(Debug, Deserialize)]
pub struct DbSourceProvision {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub host: String,
    pub port: u16,
    /// Ім'я НОВОЇ БД: ^[a-z_][a-z0-9_]{0,62}$ (має бути ВІЛЬНИМ на сервері).
    pub database: String,
    pub superuser: ProvisionSuperuser,
}

#[derive(Debug, Serialize)]
pub struct ProvisionResponse {
    pub source: DbSourceDto,
    pub message: String,
}

// ─── Хелпери ────────────────────────────────────────────────────────────────

async fn actor_claims(state: &AppState, claims: &Claims) -> Result<(), DbSrcErr> {
    auth_routes::require_admin(state, claims)
        .await
        .map(|_| ())
        .map_err(DbSrcErr::Auth)
}

// Спільний owner-check — auth_routes::require_owner (без дублів):
// require_admin (401/403) + жорстка вимога role=owner (admin/store_manager → 403).

fn id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Безпечне ім'я НОВОЇ БД: ^[a-z_][a-z0-9_]{0,62}$ (PG limit 63).
fn db_name_valid(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => return false,
    }
    let mut n = 1usize;
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
        n += 1;
        if n > 63 {
            return false;
        }
    }
    !name.is_empty()
}

fn path_source_id(raw: String) -> Result<String, DbSrcErr> {
    if id_valid(&raw) {
        Ok(raw)
    } else {
        Err(DbSrcErr::BadRequest(format!(
            "Невірний id джерела '{raw}' (дозволені літери/цифри/_/-)"
        )))
    }
}

/// Завантажує конфіг або порожній (файлу ще немає — створюється при CRUD).
fn cfg_or_empty() -> Result<db_sources::DbSourcesFile, DbSrcErr> {
    Ok(db_sources::load()?.unwrap_or_default())
}

fn find_source<'a>(cfg: &'a db_sources::DbSourcesFile, id: &str) -> Result<&'a DbSource, DbSrcErr> {
    cfg.sources
        .iter()
        .find(|(sid, _)| sid == id)
        .map(|(_, s)| s)
        .ok_or_else(|| DbSrcErr::NotFound(format!("Джерело даних '{id}' не знайдено")))
}

fn find_source_mut<'a>(
    cfg: &'a mut db_sources::DbSourcesFile,
    id: &str,
) -> Result<&'a mut DbSource, DbSrcErr> {
    cfg.sources
        .iter_mut()
        .find(|(sid, _)| sid == id)
        .map(|(_, s)| s)
        .ok_or_else(|| DbSrcErr::NotFound(format!("Джерело даних '{id}' не знайдено")))
}

fn dto(cfg: &db_sources::DbSourcesFile, id: &str, src: &DbSource) -> DbSourceDto {
    DbSourceDto {
        id: id.to_string(),
        label: src.label.clone().unwrap_or_else(|| id.to_string()),
        host: src.host.clone(),
        port: src.port,
        database: src.database.clone(),
        user: src.user.clone(),
        has_password: src
            .password_encrypted
            .as_deref()
            .map(|p| !p.is_empty())
            .unwrap_or(false),
        is_active: cfg.active.as_deref() == Some(id),
        status: src.status.clone(),
    }
}

/// Розшифровує пароль джерела (або порожній рядок, якщо пароль не задано).
fn source_password(src: &DbSource) -> Result<String, DbSrcErr> {
    match &src.password_encrypted {
        Some(enc) if !enc.is_empty() => {
            let path = db_sources::existing_path()
                .ok_or_else(|| DbSrcErr::BadRequest("db_sources.toml не знайдено".to_string()))?;
            Ok(decrypt_password(&path, enc)?)
        }
        _ => Ok(String::new()),
    }
}

/// URL джерела для sqlx / pg_* (пароль — у пам'яті процесу, не у файлі).
fn source_url(src: &DbSource) -> Result<String, DbSrcErr> {
    let pw = source_password(src)?;
    Ok(build_url(src, &pw))
}

/// Реальний пінг джерела: TCP-з'єднання + SELECT 1. Таймаут 4 c — жорсткий
/// (tokio::time::timeout навколо всієї операції): недосяжний host не підвішує
/// admin-запит на 30+ секунд дефолтного connect timeout sqlx.
/// Помилка перевірки з'єднання: сирий текст (лише лог) + стабільний машинний
/// клас (тіло 400). Тіло відповіді НЕ має містити тексту PostgreSQL/sqlx.
struct PingError {
    /// `[DB_ERROR <sqlstate>]` / `[DB_ERROR]` — те, що бачить клієнт.
    class: String,
    /// Сирий текст драйвера/PG — для логу.
    raw: String,
}

async fn ping_source(url: &str) -> Result<u64, PingError> {
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(std::time::Duration::from_secs(4), async {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(3))
            .connect(url)
            .await
            .map_err(|e| PingError {
                class: torgashka_infrastructure::db_error::db_error_class(&e),
                raw: e.to_string(),
            })?;
        let r = sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&pool)
            .await
            .map_err(|e| PingError {
                class: torgashka_infrastructure::db_error::db_error_class(&e),
                raw: e.to_string(),
            });
        pool.close().await;
        r
    })
    .await;
    match result {
        Ok(Ok(_)) => Ok(start.elapsed().as_millis() as u64),
        Ok(Err(e)) => Err(e),
        // Таймаут — не текст БД (sqlx-помилки немає): людська причина і в тілі,
        // і в логі; сирий текст сюди не потрапляє взагалі.
        Err(_) => Err(PingError {
            class: "таймаут з'єднання (4 c): host/port недосяжні або БД не відповідає".to_string(),
            raw: "таймаут з'єднання (4 c): host/port недосяжні або БД не відповідає".to_string(),
        }),
    }
}

/// Ім'я файлу дампу: torgashka_<id>_<YYYYmmdd_HHMMSS>.sql (plain SQL).
fn dump_filename(source_id: &str) -> String {
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    format!("torgashka_{source_id}_{ts}.sql")
}

/// Версійно-нейтральна перед-обробка plain-SQL дампу:
///  - прибирає рядки `\restrict …` (psql 17; старі psql не розуміють);
///  - коментує `SET transaction_timeout …` (GUC PG17 — на PG16/PG15 pg_restore
///    і psql падають; значення 0 = вимкнено, за замовчуванням однаково).
fn sanitize_dump_sql(sql: &str) -> String {
    sql.lines()
        .map(|l| {
            let t = l.trim_start();
            if t.starts_with("\\restrict") || t.starts_with("SET transaction_timeout") {
                format!("-- {l}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn stderr_tail(stderr: &[u8]) -> String {
    let s = String::from_utf8_lossy(stderr);
    let s = s.trim();
    if s.len() > 1500 {
        format!("…{}", &s[s.len() - 1500..])
    } else {
        s.to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /api/v1/admin/db-sources
// ─────────────────────────────────────────────────────────────────────────────

pub async fn list_sources(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<ListResponse>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    let cfg = db_sources::load()?.unwrap_or_default();
    let sources = cfg
        .sources
        .iter()
        .map(|(id, src)| dto(&cfg, id, src))
        .collect();
    let config_path = db_sources::existing_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| db_sources::write_path().display().to_string());
    Ok(Json(ListResponse {
        active: cfg.active.clone(),
        config_path,
        sources,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/admin/db-sources — створити джерело
// ─────────────────────────────────────────────────────────────────────────────

pub async fn create_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<DbSourceCreate>,
) -> Result<(StatusCode, Json<DbSourceDto>), DbSrcErr> {
    actor_claims(&state, &claims).await?;
    if !id_valid(&body.id) {
        return Err(DbSrcErr::BadRequest(format!(
            "Невірний id '{}' (дозволені літери/цифри/_/-, до 64 символів)",
            body.id
        )));
    }
    if body.host.trim().is_empty() || body.database.trim().is_empty() || body.user.trim().is_empty()
    {
        return Err(DbSrcErr::BadRequest(
            "host, database і user не можуть бути порожніми".to_string(),
        ));
    }
    if body.port == 0 {
        return Err(DbSrcErr::BadRequest("port має бути 1..65535".to_string()));
    }
    let mut cfg = cfg_or_empty()?;
    if cfg.sources.iter().any(|(id, _)| id == &body.id) {
        return Err(DbSrcErr::Conflict(format!(
            "Джерело '{}' уже існує",
            body.id
        )));
    }
    let cfg_path = db_sources::write_path();
    let password_encrypted = match body.password.as_deref() {
        Some(p) if !p.is_empty() => Some(encrypt_password(&cfg_path, p)?),
        _ => None,
    };
    let src = DbSource {
        label: body.label.filter(|l| !l.trim().is_empty()),
        host: body.host.trim().to_string(),
        port: body.port,
        database: body.database.trim().to_string(),
        user: body.user.trim().to_string(),
        password_encrypted,
        status: None,
    };
    cfg.sources.push((body.id.clone(), src));
    db_sources::save(&cfg)?;
    let created = dto(
        &cfg,
        &body.id,
        &cfg.sources.last().expect("тільки-що додано").1,
    );
    eprintln!(
        "[torgashka-api] db-sources: створено джерело '{}' (файл {})",
        body.id,
        db_sources::existing_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    );
    Ok((StatusCode::CREATED, Json(created)))
}

// ─────────────────────────────────────────────────────────────────────────────
// PUT /api/v1/admin/db-sources/:id — редагування
// ─────────────────────────────────────────────────────────────────────────────

pub async fn update_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<DbSourceUpdate>,
) -> Result<Json<DbSourceDto>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    let id = path_source_id(id)?;
    let mut cfg = cfg_or_empty()?;
    let src = find_source_mut(&mut cfg, &id)?;
    if let Some(l) = body.label {
        src.label = Some(l);
    }
    if let Some(h) = body.host {
        if h.trim().is_empty() {
            return Err(DbSrcErr::BadRequest(
                "host не може бути порожнім".to_string(),
            ));
        }
        src.host = h;
    }
    if let Some(p) = body.port {
        if p == 0 {
            return Err(DbSrcErr::BadRequest("port має бути 1..65535".to_string()));
        }
        src.port = p;
    }
    if let Some(d) = body.database {
        if d.trim().is_empty() {
            return Err(DbSrcErr::BadRequest(
                "database не може бути порожнім".to_string(),
            ));
        }
        src.database = d;
    }
    if let Some(u) = body.user {
        if u.trim().is_empty() {
            return Err(DbSrcErr::BadRequest(
                "user не може бути порожнім".to_string(),
            ));
        }
        src.user = u;
    }
    if let Some(pw) = body.password {
        // Some("") — очистити пароль; Some(non-empty) — перешифрувати.
        let cfg_path = db_sources::existing_path().unwrap_or_else(db_sources::write_path);
        src.password_encrypted = if pw.is_empty() {
            None
        } else {
            Some(encrypt_password(&cfg_path, &pw)?)
        };
    }
    db_sources::save(&cfg)?;
    let updated = dto(&cfg, &id, find_source(&cfg, &id)?);
    Ok(Json(updated))
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /api/v1/admin/db-sources/:id — видалити НЕактивне джерело
// ─────────────────────────────────────────────────────────────────────────────

pub async fn delete_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    let id = path_source_id(id)?;
    let mut cfg = cfg_or_empty()?;
    let idx = cfg
        .sources
        .iter()
        .position(|(sid, _)| sid == &id)
        .ok_or_else(|| DbSrcErr::NotFound(format!("Джерело даних '{id}' не знайдено")))?;
    if cfg.active.as_deref() == Some(id.as_str()) {
        return Err(DbSrcErr::Conflict(format!(
            "Джерело '{id}' активне — спершу зробіть активним інше джерело (або видаліть його)"
        )));
    }
    cfg.sources.remove(idx);
    if cfg.sources.is_empty() {
        cfg.active = None;
    }
    db_sources::save(&cfg)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "removed": id,
        "active": cfg.active,
    })))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/admin/db-sources/:id/test — реальний пінг (TCP + SELECT 1)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn test_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<TestResponse>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    let id = path_source_id(id)?;
    let cfg = db_sources::load()?.unwrap_or_default();
    let src = find_source(&cfg, &id)?;
    let url = source_url(src)?;
    match ping_source(&url).await {
        Ok(latency_ms) => Ok(Json(TestResponse {
            ok: true,
            latency_ms,
        })),
        Err(e) => {
            // Сирий текст драйвера/PG → лог; клієнту — host:port + клас (§4).
            eprintln!(
                "[torgashka-api] db-sources: пінг '{id}' ({}:{}): {}",
                src.host, src.port, e.raw
            );
            Err(DbSrcErr::BadRequest(format!(
                "Не вдалося підключитись до {}:{} (джерело '{id}'): {}",
                src.host, src.port, e.class
            )))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/admin/db-sources/:id/activate
// ─────────────────────────────────────────────────────────────────────────────
// ПЕРЕД перемиканням — обов'язкова перевірка з'єднання. При провалі джерело
// НЕ активується (400 з причиною). При успіху — active збережено у
// db_sources.toml; гарячого перепідключення пулів НЕМАЄ (див. шапку модуля):
// чесна відповідь applied_immediately=false + message про рестарт.
// ─────────────────────────────────────────────────────────────────────────────

pub async fn activate_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<ActivateResponse>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    let id = path_source_id(id)?;
    let mut cfg = db_sources::load()?.unwrap_or_default();
    let src = find_source(&cfg, &id)?.clone();
    // 1) Обов'язкова перевірка з'єднання ПЕРЕД перемиканням.
    let url = source_url(&src)?;
    if let Err(e) = ping_source(&url).await {
        eprintln!(
            "[torgashka-api] db-sources: activate '{id}' ({}:{}): {}",
            src.host, src.port, e.raw
        );
        return Err(DbSrcErr::BadRequest(format!(
            "Джерело '{id}' ({}:{}) недосяжне: {}; активним НЕ зроблено — \
             перевірте host/port/пароль",
            src.host, src.port, e.class
        )));
    }
    // 2) Збереження active у db_sources.toml.
    cfg.active = Some(id.clone());
    db_sources::save(&cfg)?;
    eprintln!(
        "[torgashka-api] db-sources: активовано джерело '{id}' (застосується після рестарту)"
    );
    Ok(Json(ActivateResponse {
        active: id,
        applied_immediately: false,
        message:
            "Джерело з'єднання перевірено і збережено як активне. Гаряче перепідключення пулів не виконується (стабільність): зміна застосується ПІСЛЯ ПЕРЕЗАПУСКУ сервісу. Каси на попередньому джерелі працюють до рестарту без перерв.".to_string(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/admin/hub-snapshot — знімок УСІЄЇ активної БД хаба для провіжну
// вузлів (ADR-0008 «Варіант B», контракт R3).
// ─────────────────────────────────────────────────────────────────────────────
// Чому саме тут і саме на роутері admin_network (обґрунтування):
//   * модуль: активне джерело, `find_source`/`source_url`/`ping_source`,
//     `find_binary`, `stderr_tail`, `DbSrcErr` і `actor_claims` — приватні в
//     ЦЬОМУ модулі; інший файл вимагав би `pub(crate)` на п'яти хелперах
//     (розширення внутрішнього API модуля заради одного роута);
//   * роутер: `admin_network` — БЕЗ `store_middleware` і З `auth_middleware`:
//     знімок — це ВСЯ БД мережі, а не дані однієї точки, тому X-Store-Id тут
//     не лише зайвий, а й хибний скоуп (той самий аргумент, що в R1 для
//     `sync_snapshot`-роутера). Роль перевіряє `actor_claims` (= require_admin:
//     owner|store_manager|admin; 401 без токена, 403 для device/cashier).
//
// Формат `--format=custom` (-Fc) ОБОВ'ЯЗКОВИЙ: plain SQL (-Fp) `pg_restore`
// вузла не читає (R2-провіжн викликає саме `pg_restore`).
//
// Каталог — `sync_snapshot::snapshot_dir()` (НЕ `dumps_dir()`): знімок мусить
// з'явитися там, звідки його віддає `GET /api/v1/sync/snapshot` (R1) — тоді
// «хаб створив → вузол одразу забрав» працює без перезапуску й без копіювання
// (каталог читається на кожен запит).
//
// Ротації/видалення старих знімків НЕМАЄ: політики ретеншну не існує
// (docs/operations/hub-and-nodes.md §11.1 Б7) — вигадувати її заборонено.

/// Тіло `POST /api/v1/admin/hub-snapshot`: порожнє (`{}`), як в export-dump.
#[derive(Debug, Default, Deserialize)]
pub struct HubSnapshotBody {}

/// Відповідь 200 (snake_case — як решта HTTP-DTO db-sources; camelCase у
/// проєкті лише для Tauri-команд).
#[derive(Debug, Serialize)]
pub struct HubSnapshotResponse {
    pub ok: bool,
    pub file_name: String,
    pub bytes: u64,
    pub sha256: String,
    pub path: String,
}

/// Чисте ім'я знімка: `<database>_YYYYMMDD_HHMMSS.<ext>` (без суфікса колізії).
fn snapshot_file_name(database: &str, ts: &str) -> String {
    format!(
        "{database}_{ts}.{}",
        crate::sync_snapshot::SNAPSHOT_EXTENSION
    )
}

/// Те саме ім'я із суфіксом колізії `-01`, `-02`… (`n == 0` → без суфікса).
/// Наявні файли НЕ перезаписуються: два знімки в одну секунду = два файли.
fn snapshot_file_name_no(database: &str, ts: &str, n: u32) -> String {
    if n == 0 {
        snapshot_file_name(database, ts)
    } else {
        format!(
            "{database}_{ts}-{n:02}.{}",
            crate::sync_snapshot::SNAPSHOT_EXTENSION
        )
    }
}

/// Вільний шлях під знімок у каталозі (перевірка існування файла, не «на віру»).
fn unique_snapshot_path(
    dir: &std::path::Path,
    database: &str,
    ts: &str,
) -> Result<PathBuf, DbSrcErr> {
    for n in 0..=99u32 {
        let candidate = dir.join(snapshot_file_name_no(database, ts, n));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(DbSrcErr::Conflict(format!(
        "у каталозі вже 100 знімків з міткою {database}_{ts} — ім'я не підібрати"
    )))
}

/// `POST /api/v1/admin/hub-snapshot` → створити знімок активної БД хаба.
pub async fn create_hub_snapshot(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(_body): Json<HubSnapshotBody>,
) -> Result<Json<HubSnapshotResponse>, DbSrcErr> {
    actor_claims(&state, &claims).await?;

    let cfg = db_sources::load()?.unwrap_or_default();
    let source_id = cfg.active.clone().ok_or_else(|| {
        DbSrcErr::BadRequest(
            "Активне джерело не задано — знімок створюється лише з активної БД хаба".to_string(),
        )
    })?;
    let src = find_source(&cfg, &source_id)?.clone();

    // 1) Превентивна перевірка з'єднання — зрозуміла помилка ДО pg_dump.
    let url = source_url(&src)?;
    if let Err(e) = ping_source(&url).await {
        eprintln!(
            "[torgashka-api] hub-snapshot: джерело '{source_id}' ({}:{}): {}",
            src.host, src.port, e.raw
        );
        return Err(DbSrcErr::BadRequest(format!(
            "Джерело '{source_id}' ({}:{}) недосяжне: {}; знімок не створено",
            src.host, src.port, e.class
        )));
    }

    // 2) Бінарник + каталог знімків (той самий, звідки його віддає R1).
    let pg_dump = db_sources::find_binary("pg_dump")?;
    let dir = crate::sync_snapshot::snapshot_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| DbSrcErr::Internal(format!("не вдалося створити {}: {e}", dir.display())))?;

    // 3) Ім'я файла: <database>_YYYYMMDD_HHMMSS.dump (+ -NN при колізії).
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let out_path = unique_snapshot_path(&dir, &src.database, &ts)?;

    // 4) pg_dump -Fc. Порт/хост/БД — ТІЛЬКИ з конфіга джерела (жодних дефолтів).
    let pw = source_password(&src)?;
    let output = tokio::process::Command::new(&pg_dump)
        .arg("--format=custom")
        .arg("--no-owner")
        .arg("--no-privileges")
        .arg(format!("--file={}", out_path.display()))
        .arg("-h")
        .arg(&src.host)
        .arg("-p")
        .arg(src.port.to_string())
        .arg("-U")
        .arg(&src.user)
        .arg("-d")
        .arg(&src.database)
        .env("PGPASSWORD", &pw)
        .output()
        .await
        .map_err(|e| DbSrcErr::Internal(format!("pg_dump не запустився: {e}")))?;

    // 5) Будь-яка невдача → файл прибрати (жодних обрізаних знімків у каталозі).
    if !output.status.success() {
        let tail = stderr_tail(&output.stderr);
        let _ = std::fs::remove_file(&out_path);
        return Err(DbSrcErr::BadRequest(format!(
            "pg_dump -Fc завершився з помилкою (джерело '{source_id}'): {tail}"
        )));
    }
    let bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    if bytes == 0 {
        let _ = std::fs::remove_file(&out_path);
        return Err(DbSrcErr::BadRequest(format!(
            "pg_dump створив порожній знімок (0 байт) — файл прибрано: {}",
            out_path.display()
        )));
    }

    // 6) sha256 — із ЗАПИСАНОГО файла (а не «з повітря»).
    let data = std::fs::read(&out_path)
        .map_err(|e| DbSrcErr::Internal(format!("не вдалося прочитати знімок: {e}")))?;
    let sha256 = crate::sync_snapshot::sha256_hex(&data);
    let file_name = out_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    eprintln!(
        "[torgashka-api] hub-snapshot: знімок створено {} ({} байт, sha256 {sha256})",
        out_path.display(),
        bytes
    );
    Ok(Json(HubSnapshotResponse {
        ok: true,
        file_name,
        bytes,
        sha256,
        path: out_path.display().to_string(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/admin/db-sources/export-dump — pg_dump активної БД
// ─────────────────────────────────────────────────────────────────────────────

pub async fn export_dump(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ExportDumpBody>,
) -> Result<Json<ExportResponse>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    let cfg = db_sources::load()?.unwrap_or_default();
    let source_id = match body.source_id {
        Some(sid) => sid,
        None => cfg.active.clone().ok_or_else(|| {
            DbSrcErr::BadRequest("Активне джерело не задано — вкажіть source_id".to_string())
        })?,
    };
    let src = find_source(&cfg, &source_id)?.clone();
    // Превентивна перевірка з'єднання — зрозуміла помилка до запуску pg_dump.
    let url = source_url(&src)?;
    if let Err(e) = ping_source(&url).await {
        eprintln!(
            "[torgashka-api] db-sources: dump '{source_id}' ({}:{}): {}",
            src.host, src.port, e.raw
        );
        return Err(DbSrcErr::BadRequest(format!(
            "Джерело '{source_id}' ({}:{}) недосяжне: {}; дамп не створено",
            src.host, src.port, e.class
        )));
    }
    let pg_dump = db_sources::find_binary("pg_dump")?;
    let dumps_dir = db_sources::dumps_dir();
    std::fs::create_dir_all(&dumps_dir).map_err(|e| {
        DbSrcErr::Internal(format!("не вдалося створити {}: {e}", dumps_dir.display()))
    })?;
    let file_name = dump_filename(&source_id);
    let out_path = dumps_dir.join(&file_name);

    let pw = source_password(&src)?;
    // Plain SQL (-Fp): універсальний формат для psql будь-якої версії
    // (див. шапку модуля — чому не custom/pg_restore).
    let output = tokio::process::Command::new(&pg_dump)
        .arg("--no-owner")
        .arg("--format=plain")
        .arg(format!("--file={}", out_path.display()))
        .arg("-h")
        .arg(&src.host)
        .arg("-p")
        .arg(src.port.to_string())
        .arg("-U")
        .arg(&src.user)
        .arg("-d")
        .arg(&src.database)
        .env("PGPASSWORD", &pw)
        .output()
        .await
        .map_err(|e| DbSrcErr::Internal(format!("pg_dump не запустився: {e}")))?;
    if !output.status.success() {
        let tail = stderr_tail(&output.stderr);
        let _ = std::fs::remove_file(&out_path);
        return Err(DbSrcErr::BadRequest(format!(
            "pg_dump завершився з помилкою (джерело '{source_id}'): {tail}"
        )));
    }
    let size_bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "[torgashka-api] db-sources: дамп створено {} ({} байт)",
        out_path.display(),
        size_bytes
    );
    Ok(Json(ExportResponse {
        file: file_name,
        path: out_path.display().to_string(),
        size_bytes,
        source_id,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /api/v1/admin/db-sources/dumps — список дампів (для вибору в імпорті)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn list_dumps(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<DumpInfo>>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    let dumps_dir = db_sources::dumps_dir();
    let mut items = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dumps_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Ok(meta) = std::fs::metadata(&path) {
                let modified = meta
                    .modified()
                    .ok()
                    .map(|t| {
                        chrono::DateTime::<chrono::Local>::from(t)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();
                items.push(DumpInfo {
                    file: entry.file_name().to_string_lossy().to_string(),
                    size_bytes: meta.len(),
                    modified_at: modified,
                });
            }
        }
    }
    items.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(Json(items))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/admin/db-sources/import-dump — pg_restore у вибране джерело
// ─────────────────────────────────────────────────────────────────────────────

pub async fn import_dump(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ImportDumpBody>,
) -> Result<Json<ImportResponse>, DbSrcErr> {
    actor_claims(&state, &claims).await?;
    if !id_valid(&body.source_id) {
        return Err(DbSrcErr::BadRequest(format!(
            "Невірний id джерела '{}'",
            body.source_id
        )));
    }
    if body.file.contains('/') || body.file.contains('\\') || body.file.contains("..") {
        return Err(DbSrcErr::BadRequest(
            "file має бути ім'ям файлу зі списку дампів (без шляхів)".to_string(),
        ));
    }
    let cfg = db_sources::load()?.unwrap_or_default();
    let src = find_source(&cfg, &body.source_id)?.clone();
    // Приймач має бути досяжним до запуску pg_restore.
    let url = source_url(&src)?;
    if let Err(e) = ping_source(&url).await {
        eprintln!(
            "[torgashka-api] db-sources: import у '{}' ({}:{}): {}",
            body.source_id, src.host, src.port, e.raw
        );
        return Err(DbSrcErr::BadRequest(format!(
            "Джерело-приймач '{}' ({}:{}) недосяжне: {}; імпорт не виконано",
            body.source_id, src.host, src.port, e.class
        )));
    }
    let dumps_dir = db_sources::dumps_dir();
    let file_path: PathBuf = dumps_dir.join(&body.file);
    if !file_path.is_file() {
        return Err(DbSrcErr::NotFound(format!(
            "Дамп '{}' не знайдено в {}",
            body.file,
            dumps_dir.display()
        )));
    }
    let psql_bin = db_sources::find_binary("psql")?;
    let pw = source_password(&src)?;

    // Перед-обробка дампу: коментуємо несумісні зі старішими серверами/psql
    // рядки (transaction_timeout з pg_dump >=17, метакоманда restrict psql 17) —
    // дамп стає версійно-нейтральним (див. sanitize_dump_sql нижче).
    let original = std::fs::read_to_string(&file_path).map_err(|e| {
        DbSrcErr::Internal(format!("дамп не читається ({}): {e}", file_path.display()))
    })?;
    let sanitized = sanitize_dump_sql(&original);

    // clean=true: спершу повністю очищаємо схему public БД призначення.
    if body.clean {
        let out = tokio::process::Command::new(&psql_bin)
            .arg("-h")
            .arg(&src.host)
            .arg("-p")
            .arg(src.port.to_string())
            .arg("-U")
            .arg(&src.user)
            .arg("-d")
            .arg(&src.database)
            .arg("-c")
            .arg("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .env("PGPASSWORD", &pw)
            .output()
            .await
            .map_err(|e| DbSrcErr::Internal(format!("psql (clean) не запустився: {e}")))?;
        if !out.status.success() {
            return Err(DbSrcErr::BadRequest(format!(
                "не вдалося очистити схему призначення (джерело '{}'): {}",
                body.source_id,
                stderr_tail(&out.stderr)
            )));
        }
    }

    // Імпорт через stdin (без тимчасових файлів). psql БЕЗ ON_ERROR_STOP:
    // окремі несумісні SET/коментарі не зупиняють відновлення; реальний успіх
    // перевіряємо після — наявністю ключових таблиць (нижче).
    let mut cmd = tokio::process::Command::new(&psql_bin);
    cmd.arg("-h")
        .arg(&src.host)
        .arg("-p")
        .arg(src.port.to_string())
        .arg("-U")
        .arg(&src.user)
        .arg("-d")
        .arg(&src.database)
        .env("PGPASSWORD", &pw)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| DbSrcErr::Internal(format!("psql не запустився: {e}")))?;
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(sanitized.as_bytes())
            .await
            .map_err(|e| DbSrcErr::Internal(format!("не вдалося передати дамп у psql: {e}")))?;
        stdin.flush().await.ok();
    } // stdin закривається при drop → psql дочитує файл.
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| DbSrcErr::Internal(format!("psql завершився з помилкою очікування: {e}")))?;

    // Обов'язкова перевірка: ключові таблиці схеми імпортовано.
    let verify_url = source_url(&src)?;
    let verify_ok = {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect(&verify_url)
            .await;
        match pool {
            Ok(p) => {
                let users: Result<bool, _> =
                    sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
                        .fetch_one(&p)
                        .await;
                let stores: Result<bool, _> =
                    sqlx::query_scalar("SELECT to_regclass('public.stores') IS NOT NULL")
                        .fetch_one(&p)
                        .await;
                p.close().await;
                users.ok() == Some(true) && stores.ok() == Some(true)
            }
            Err(_) => false,
        }
    };
    if !verify_ok {
        return Err(DbSrcErr::BadRequest(format!(
            "імпорт у джерело '{}' не завершився успішно (таблиці users/stores не знайдено після psql);              хвіст виводу: {}",
            body.source_id,
            stderr_tail(&output.stderr)
        )));
    }
    if !output.status.success() {
        // exit != 0 при ON_ERROR_STOP=off можливий лише за фатальних помилок.
        return Err(DbSrcErr::BadRequest(format!(
            "psql завершився з кодом {} (джерело '{}'): {}",
            output.status.code().unwrap_or(-1),
            body.source_id,
            stderr_tail(&output.stderr)
        )));
    }
    eprintln!(
        "[torgashka-api] db-sources: дамп '{}' імпортовано у джерело '{}' (перевірено: users/stores присутні)",
        body.file, body.source_id
    );
    Ok(Json(ImportResponse {
        ok: true,
        source_id: body.source_id,
        file: body.file,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/admin/db-sources/provision — Бекенд provisioning БД (Етап 1
// sync-offline). Owner-only (require_owner: admin/store_manager → 403).
// ─────────────────────────────────────────────────────────────────────────────
// Логіка (деталі — torgashka_infrastructure::provision):
//   1. валідація id / database / host / port / superuser;
//   2. підключення суперкористувачем до host:port (БД postgres) + SELECT 1
//      (не-localhost → sslmode=require);
//   3. CREATE DATABASE <database> TEMPLATE template0 (існує → 409);
//   4. повна схема SCHEMA_SQL (перевикористання, без копій);
//   5. роль torgashka_app (NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE,
//      автогенерований пароль) + GRANT-и на НОВУ БД;
//   6. при частковому збої — DROP DATABASE IF EXISTS (cleanup у provision);
//   7. збереження джерела у db_sources.toml: пароль ЛИШЕ через encrypt_password
//      (AES-256-GCM), user=torgashka_app, status=provisioned_pending_activation;
//   8. активація — НЕ автоматична: окремим POST /:id/activate (старий роут).
// Суперкредити: одноразові, у відповідь/файл/логи НЕ потрапляють.
// ─────────────────────────────────────────────────────────────────────────────

pub async fn provision_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<DbSourceProvision>,
) -> Result<(StatusCode, Json<ProvisionResponse>), DbSrcErr> {
    let _owner_id = auth_routes::require_owner(&state, &claims)
        .await
        .map_err(DbSrcErr::Auth)?;

    // ── 1. Валідація ────────────────────────────────────────────────────────
    if !id_valid(&body.id) {
        return Err(DbSrcErr::BadRequest(format!(
            "Невірний id '{}' (дозволені літери/цифри/_/-, до 64 символів)",
            body.id
        )));
    }
    if !db_name_valid(&body.database) {
        return Err(DbSrcErr::BadRequest(format!(
            "Невірне ім'я нової БД '{}' (дозволені: ^[a-z_][a-z0-9_]{{0,62}}$ — малі літери/цифри/підкреслення, початок з літери або _)",
            body.database
        )));
    }
    if body.host.trim().is_empty() {
        return Err(DbSrcErr::BadRequest(
            "host не може бути порожнім".to_string(),
        ));
    }
    if body.port == 0 {
        return Err(DbSrcErr::BadRequest("port має бути 1..65535".to_string()));
    }
    if body.superuser.user.trim().is_empty() || body.superuser.password.is_empty() {
        return Err(DbSrcErr::BadRequest(
            "superuser.user і superuser.password обов'язкові (одноразові кредити, не зберігаються)"
                .to_string(),
        ));
    }
    // Джерело з таким id не має існувати у локальному конфігу.
    let mut cfg = cfg_or_empty()?;
    if cfg.sources.iter().any(|(sid, _)| sid == &body.id) {
        return Err(DbSrcErr::Conflict(format!(
            "Джерело '{}' уже існує",
            body.id
        )));
    }

    // ── 2–6. Провіжинінг БД (інфраструктура; cleanup при збої — там) ───────
    let target = provision::ProvisionTarget {
        host: body.host.trim().to_string(),
        port: body.port,
        database: body.database.trim().to_string(),
        superuser: provision::SuperuserCreds {
            user: body.superuser.user.trim().to_string(),
            password: body.superuser.password.clone(),
        },
    };
    let outcome = provision::provision_database(&target).await?;
    // ← суперкредити більше не потрібні; у жодній гілці нижче не логуються.

    // ── 7. Збереження джерела: пароль ЛИШЕ зашифрований (AES-256-GCM) ──────
    let cfg_path = db_sources::write_path();
    let encrypted = match encrypt_password(&cfg_path, &outcome.app_password) {
        Ok(e) => e,
        Err(e) => {
            // БД вже створена на сервері — прибрати, щоб повтор був чистим.
            provision::cleanup_provisioned_database(&target).await;
            return Err(DbSrcErr::Internal(format!(
                "не вдалося зашифрувати пароль джерела (БД '{0}' прибрано): {e}",
                outcome.database
            )));
        }
    };
    let src = DbSource {
        label: body.label.filter(|l| !l.trim().is_empty()),
        host: target.host.clone(),
        port: target.port,
        database: outcome.database.clone(),
        user: outcome.app_user.clone(),
        password_encrypted: Some(encrypted),
        status: Some("provisioned_pending_activation".to_string()),
    };
    cfg.sources.push((body.id.clone(), src));
    if let Err(e) = db_sources::save(&cfg) {
        provision::cleanup_provisioned_database(&target).await;
        return Err(DbSrcErr::Internal(format!(
            "не вдалося зберегти db_sources.toml (БД '{0}' прибрано): {e}",
            outcome.database
        )));
    }
    let created = dto(&cfg, &body.id, find_source(&cfg, &body.id)?);
    eprintln!(
        "[torgashka-api] db-sources: provision створено джерело '{}' (БД '{}', НЕ активовано)",
        body.id, outcome.database
    );
    Ok((
        StatusCode::CREATED,
        Json(ProvisionResponse {
            source: created,
            message:
                "Базу даних створено та схему накатано. Джерело НЕ активовано: активація — окремим підтвердженням власника (POST /:id/activate) після перевірки з'єднання."
                    .to_string(),
        }),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Юніт-тести чистих частин hub-snapshot (без БД і без pg_dump)
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod hub_snapshot_tests {
    use super::*;

    /// Ім'я знімка: `<database>_YYYYMMDD_HHMMSS.dump` — саме той формат, який
    /// очікує фронт/док і який читає `sync_snapshot::newest_dump` (розширення .dump).
    #[test]
    fn snapshot_file_name_matches_frozen_format() {
        assert_eq!(
            snapshot_file_name("torgashka", "20260912_101530"),
            "torgashka_20260912_101530.dump"
        );
        assert_eq!(
            snapshot_file_name("pos_system_fresh", "20260101_000000"),
            "pos_system_fresh_20260101_000000.dump"
        );
        assert!(snapshot_file_name("db", "x").ends_with(".dump"));
    }

    /// Колізія в межах секунди: суфікс `-01`, `-02`… (не перезаписує наявний файл).
    #[test]
    fn snapshot_file_name_collision_suffix() {
        assert_eq!(
            snapshot_file_name_no("torgashka", "20260912_101530", 0),
            "torgashka_20260912_101530.dump",
            "n=0 — це основний файл без суфікса"
        );
        assert_eq!(
            snapshot_file_name_no("torgashka", "20260912_101530", 1),
            "torgashka_20260912_101530-01.dump"
        );
        assert_eq!(
            snapshot_file_name_no("torgashka", "20260912_101530", 12),
            "torgashka_20260912_101530-12.dump"
        );
    }

    /// `unique_snapshot_path` реально дивиться на каталог: зайняте ім'я → наступне.
    #[test]
    fn unique_snapshot_path_skips_existing_files() {
        let dir = std::env::temp_dir().join(format!(
            "torgashka_hub_snap_unit_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let ts = "20260912_101530";
        let first = unique_snapshot_path(&dir, "torgashka", ts).expect("1-й шлях");
        assert!(first.ends_with("torgashka_20260912_101530.dump"));
        std::fs::write(&first, b"x").expect("створення файла");
        let second = unique_snapshot_path(&dir, "torgashka", ts).expect("2-й шлях");
        assert_eq!(
            second.file_name().map(|n| n.to_string_lossy().to_string()),
            Some("torgashka_20260912_101530-01.dump".to_string())
        );
        assert_ne!(first, second, "наявний файл не перезаписуємо");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Порожнє тіло `{}` мусить десеріалізуватись (фронт робить `api.post(url, {})`).
    #[test]
    fn hub_snapshot_body_accepts_empty_object() {
        let body: HubSnapshotBody = serde_json::from_str("{}").expect("{} має прийматись");
        let _ = body;
        // зайві поля ігноруються (як у решті DTO db-sources)
        let _: HubSnapshotBody = serde_json::from_str(r#"{"ignored":1}"#).expect("ignored ok");
    }

    /// JSON-відповіді — snake_case (як решта HTTP-DTO; не camelCase Tauri).
    #[test]
    fn hub_snapshot_response_is_snake_case() {
        let v = serde_json::to_value(HubSnapshotResponse {
            ok: true,
            file_name: "torgashka_20260912_101530.dump".to_string(),
            bytes: 123,
            sha256: "abc".to_string(),
            path: "/tmp/x.dump".to_string(),
        })
        .expect("json");
        for key in ["ok", "file_name", "bytes", "sha256", "path"] {
            assert!(v.get(key).is_some(), "немає ключа {key} у {v}");
        }
        assert!(
            v.get("fileName").is_none(),
            "camelCase у HTTP-DTO заборонено"
        );
    }
}
