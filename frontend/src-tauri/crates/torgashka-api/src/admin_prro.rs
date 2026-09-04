// ─────────────────────────────────────────────────────────────────────────────
// admin_prro — «Один магазин — один ПРРО» (централізоване per-store
// налаштування фіскального реєстру точки).
// ─────────────────────────────────────────────────────────────────────────────
// Роут (окремий /admin/* роутер БЕЗ store_middleware; RBAC — owner|admin
// через require_admin + перевірку ролі; касир → 403):
//
//   GET /api/v1/admin/stores/:store_id/prro-settings
//       → { store_id, store_name, scope:"store", editable:true,
//           configured, settings{prro_fn,prro_tn,prro_zn,mode,url},
//           key{...}, last_shift{...}|null, settings_updated_at }
//   PUT /api/v1/admin/stores/:store_id/prro-settings  (multipart form)
//       → те саме після збереження + audit_log (action=prro_settings_updated)
//
// ── Per-store модель (закриває аномалію Етапа 5) ─────────────────────────────
// Раніше prro_settings / prro_shifts / prro_queue_items були глобальними
// таблицями БЕЗ store_id і БЕЗ RLS, а КЕП жив в ОДНОМУ файлі .prro_keystore.json
// (PrroKeyStore::default) — PUT налаштувань «точки» записав би спільний
// реєстр для ВСІХ точок (звідси read-only заглушка в admin_prro.rs Етапа 5).
// Тепер:
//   • prro_settings/shifts/queue мають store_id NOT NULL + RLS (міграція в
//     prro/schema.rs + db.rs ensure_schema + fresh schema.sql); існуючі
//     глобальні рядки перенесено першому активному магазину;
//   • ключі налаштувань — (store_id, key_name); PUT нижче пише ТІЛЬКИ
//     конфіг вказаної точки: конфіг магазину Б не затирає конфіг магазину А;
//   • КЕП точки — окреме сховище PrroKeyStore::for_store(store_id):
//     .prro_keystore_<store_id>.json + .prro_master_<store_id>.key; файл
//     ключа — certs/prro-{mode}/{store_id}/ (однакові імена файлів різних
//     точок не конфліктують);
//   • каса магазину бачить оновлений конфіг через /api/v2/prro/settings
//     (репозиторій працює під StoreCtx X-Store-Id).
//
// Безпека: у GET НІКОЛИ не повертаються ключ КЕП і пароль — лише булеві
// ознаки наявності та назва файлу (не секрет). signer_serial (серійний №
// сертифіката) — публічний атрибут сертифіката з останньої зміни точки.
// ─────────────────────────────────────────────────────────────────────────────
use std::path::Path;

use axum::{
    extract::{Extension, Multipart, Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::{PgPool, Row};
use torgashka_prro::prro::{
    config_mode, settings, PrroKeyStore, PrroSettingsError, KEY_PRRO_FN, KEY_PRRO_MODE,
    KEY_PRRO_TN, KEY_PRRO_URL, KEY_PRRO_ZN,
};
use uuid::Uuid;

use crate::{auth::Claims, auth_routes, network, AppState};

// ─── Помилки → HTTP ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AdminPrroErr {
    Auth(auth_routes::AuthRouteError),
    Forbidden(String),
    BadRequest(String),
    NotFound(String),
    Db(sqlx::Error),
}

impl From<auth_routes::AuthRouteError> for AdminPrroErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        AdminPrroErr::Auth(e)
    }
}

impl From<sqlx::Error> for AdminPrroErr {
    fn from(e: sqlx::Error) -> Self {
        AdminPrroErr::Db(e)
    }
}

impl From<PrroSettingsError> for AdminPrroErr {
    fn from(e: PrroSettingsError) -> Self {
        AdminPrroErr::BadRequest(e.to_string())
    }
}

impl IntoResponse for AdminPrroErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            AdminPrroErr::Auth(e) => e.into_response(),
            AdminPrroErr::Forbidden(m) => body(StatusCode::FORBIDDEN, m),
            AdminPrroErr::BadRequest(m) => body(StatusCode::BAD_REQUEST, m),
            AdminPrroErr::NotFound(m) => body(StatusCode::NOT_FOUND, m),
            AdminPrroErr::Db(e) => {
                eprintln!("[torgashka-api] admin_prro: помилка БД: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": "Внутрішня помилка сервера"})),
                )
                    .into_response()
            }
        }
    }
}

fn pool(state: &AppState) -> Result<PgPool, AdminPrroErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| AdminPrroErr::BadRequest("write_pool не ініціалізовано".to_string()))
}

/// RBAC адмін-ПРРО: лише owner|admin (store_manager та cashier → 403).
async fn require_owner_or_admin(
    state: &AppState,
    claims: &Claims,
) -> Result<Uuid, AdminPrroErr> {
    let actor_id = auth_routes::require_admin(state, claims)
        .await
        .map_err(AdminPrroErr::Auth)?;
    if !matches!(claims.role.as_str(), "owner" | "admin") {
        return Err(AdminPrroErr::Forbidden(
            "Налаштування ПРРО точки доступні лише власнику або адміністратору мережі"
                .to_string(),
        ));
    }
    Ok(actor_id)
}

// ─── DTO ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PrroSettingsView {
    pub prro_fn: String,
    pub prro_tn: String,
    pub prro_zn: String,
    pub mode: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct PrroKeyStatusDto {
    /// "env" | "keystore" | "none" — джерело файлу ключа точки.
    pub source: &'static str,
    pub file_configured: bool,
    /// Базова назва файлу ключа (не секрет).
    pub file_name: Option<String>,
    pub password_configured: bool,
    /// Серійний № сертифіката КЕП (з останньої зміни точки; публічний).
    pub signer_serial: Option<String>,
    pub signer_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PrroLastShiftDto {
    pub shift_number: i32,
    pub status: String,
    pub opened_at: Option<NaiveDateTime>,
    pub closed_at: Option<NaiveDateTime>,
    pub receipt_count: i32,
    pub zreport_number: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StorePrroDto {
    pub store_id: Uuid,
    pub store_name: String,
    /// "store" — модель: окремий ПРРО-конфіг/зміни/черга на точку.
    pub scope: &'static str,
    /// true — централізований per-store PUT підтримується.
    pub editable: bool,
    /// Людське пояснення (None — редагування доступне, аномалію закрито).
    pub reason: Option<String>,
    /// ПРРО точки готовий до фіскалізації: задані prro_fn та url (+ключ КЕП).
    pub configured: bool,
    pub settings: PrroSettingsView,
    pub key: PrroKeyStatusDto,
    pub last_shift: Option<PrroLastShiftDto>,
    /// Останнє оновлення налаштувань ЦІЄЇ точки (max updated_at; null — нічого).
    pub settings_updated_at: Option<NaiveDateTime>,
}

const KEYS: [&str; 5] = ["prro_fn", "prro_tn", "prro_zn", "mode", "url"];

/// Читає per-store стан ПРРО точки (без секретів).
async fn read_store_prro(db: &PgPool, store_id: Uuid) -> Result<StorePrroDto, AdminPrroErr> {
    let (store_name,): (String,) = sqlx::query_as("SELECT name FROM stores WHERE id = $1")
        .bind(store_id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AdminPrroErr::NotFound("Точку не знайдено".to_string()))?;

    // Налаштування САМЕ цієї точки: (store_id, key_name).
    let keys_sql: Vec<String> = KEYS.iter().map(|s| s.to_string()).collect();
    let rows = sqlx::query(
        "SELECT key_name, value FROM prro_settings WHERE store_id = $1 AND key_name = ANY($2)",
    )
    .bind(store_id)
    .bind(&keys_sql)
    .fetch_all(db)
    .await?;
    let mut vals: std::collections::HashMap<String, String> = rows
        .iter()
        .filter_map(|r| {
            let k: String = r.get("key_name");
            let v: Option<String> = r.get("value");
            v.map(|v| (k, v))
        })
        .collect();

    let empty = || String::new();
    let prro_fn = vals.remove("prro_fn").unwrap_or_else(empty);
    let prro_tn = vals.remove("prro_tn").unwrap_or_else(empty);
    let prro_zn = vals.remove("prro_zn").unwrap_or_else(empty);
    let mode = vals.remove("mode").unwrap_or_else(empty);
    let url = vals.remove("url").unwrap_or_else(empty);

    let (settings_updated_at,): (Option<NaiveDateTime>,) =
        sqlx::query_as(
            "SELECT max(updated_at)::timestamp FROM prro_settings \
             WHERE store_id = $1 AND key_name = ANY($2)",
        )
        .bind(store_id)
        .bind(&keys_sql)
        .fetch_one(db)
        .await?;

    // Остання зміна ТОЧКИ (signer — серійний № сертифіката КЕП).
    let last_shift: Option<PrroLastShiftDto> = {
        let row = sqlx::query(
            "SELECT shift_number, status::text AS status, opened_at::timestamp AS opened_at,
                    closed_at::timestamp AS closed_at, receipt_count, zreport_number
             FROM prro_shifts WHERE store_id = $1
             ORDER BY opened_at DESC LIMIT 1",
        )
        .bind(store_id)
        .fetch_optional(db)
        .await?;
        row.map(|r| PrroLastShiftDto {
            shift_number: r.get("shift_number"),
            status: r.get("status"),
            opened_at: r.get("opened_at"),
            closed_at: r.get("closed_at"),
            receipt_count: r.get("receipt_count"),
            zreport_number: r.get("zreport_number"),
        })
    };

    // Стан ключа ЕЦП ТОЧКИ: БЕЗ секретів — лише наявність (keystore per-store;
    // decrypt_password НЕ викликаємо — не створюємо master-ключ на GET).
    let ks = PrroKeyStore::for_store(store_id);
    let env_file = std::env::var("PRRO_KEY_FILE")
        .ok()
        .filter(|s| !s.is_empty());
    let env_pwd = std::env::var("PRRO_KEY_PASSWORD")
        .ok()
        .filter(|s| !s.is_empty());
    let ks_path = ks.get_key_path().ok();
    let mut file_path = ks_path.clone().filter(|p| Path::new(p).is_file());
    let mut source: &'static str = if file_path.is_some() {
        "keystore"
    } else {
        "none"
    };
    // Legacy fallback: env PRRO_KEY_FILE — лише для одиночної інсталяції,
    // якщо per-store keystore точки ще не створено.
    if file_path.is_none() {
        if let Some(env) = env_file
            .clone()
            .filter(|p| Path::new(p).is_file())
        {
            file_path = Some(env);
            source = "env";
        }
    }
    let file_configured = file_path.as_ref().is_some_and(|p| Path::new(p).is_file());
    let password_configured = env_pwd.is_some() || ks.is_configured();
    let file_name = file_path
        .as_ref()
        .filter(|_| file_configured)
        .and_then(|p| {
            Path::new(p)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
        });

    let (signer_serial, signer_name): (Option<String>, Option<String>) = {
        let row = sqlx::query(
            "SELECT signer_serial, signer_name FROM prro_shifts
             WHERE store_id = $1 AND signer_serial IS NOT NULL
             ORDER BY opened_at DESC LIMIT 1",
        )
        .bind(store_id)
        .fetch_optional(db)
        .await?;
        match row {
            Some(r) => (r.get("signer_serial"), r.get("signer_name")),
            None => (None, None),
        }
    };

    let configured = !prro_fn.is_empty() && !url.is_empty();

    Ok(StorePrroDto {
        configured,
        store_id,
        store_name,
        scope: "store",
        editable: true,
        reason: None,
        settings: PrroSettingsView {
            prro_fn,
            prro_tn,
            prro_zn,
            mode,
            url,
        },
        key: PrroKeyStatusDto {
            source,
            file_configured,
            file_name,
            password_configured,
            signer_serial,
            signer_name,
        },
        last_shift,
        settings_updated_at,
    })
}

// ─── GET /api/v1/admin/stores/:store_id/prro-settings ────────────────────────

pub async fn prro_settings(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    AxumPath(store_id): AxumPath<String>,
) -> Result<Json<StorePrroDto>, AdminPrroErr> {
    require_owner_or_admin(&state, &claims).await?;
    let db = pool(&state)?;
    let store_id = Uuid::parse_str(&store_id)
        .map_err(|_| AdminPrroErr::BadRequest(format!("Невірний store_id: '{store_id}'")))?;
    Ok(Json(read_store_prro(&db, store_id).await?))
}

// ─── PUT /api/v1/admin/stores/:store_id/prro-settings (multipart) ────────────

/// Записує значення налаштування точки (upsert per-store).
async fn put_setting(
    db: &PgPool,
    store_id: Uuid,
    key: &str,
    value: &str,
) -> Result<(), AdminPrroErr> {
    sqlx::query(
        "INSERT INTO prro_settings (store_id, key_name, value, updated_at) \
         VALUES ($1, $2, $3, now()) \
         ON CONFLICT (store_id, key_name) \
         DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(store_id)
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

/// Валідація + збереження per-store (1:1 PrroSettingsUseCase::save_settings).
async fn apply_put(
    db: &PgPool,
    store_id: Uuid,
    key_file_content: Option<Vec<u8>>,
    key_file_name: Option<String>,
    key_file_path: Option<String>,
    key_password: Option<String>,
    prro_fn: Option<String>,
    prro_tn: Option<String>,
    prro_zn: Option<String>,
    mode: Option<String>,
    url: Option<String>,
    auto_fiscalize: Option<String>,
) -> Result<StorePrroDto, AdminPrroErr> {
    if let Some(m) = &mode {
        if m != "test" && m != "prod" {
            return Err(AdminPrroErr::BadRequest(format!(
                "Невідомий режим ПРРО: '{m}'. Допустимі: 'test', 'prod'"
            )));
        }
    }

    // Актуальний режим (для каталогу ключа) — поточний або новий.
    let current_mode: Option<(String,)> =
        sqlx::query_as("SELECT value FROM prro_settings WHERE store_id = $1 AND key_name = $2")
            .bind(store_id)
            .bind(KEY_PRRO_MODE)
            .fetch_optional(db)
            .await?;
    let target_mode = mode
        .clone()
        .unwrap_or_else(|| {
            current_mode
                .as_ref()
                .and_then(|(v,)| if v.is_empty() { None } else { Some(v.clone()) })
                .unwrap_or_else(config_mode)
        })
        .to_string();

    // 1. Ключ КЕП точки: файл (upload) або шлях (desktop/env), per-store.
    let ks = PrroKeyStore::for_store(store_id);
    let key_path: Option<String> = if let Some(content) = key_file_content {
        let name = key_file_name.as_deref().ok_or_else(|| {
            AdminPrroErr::BadRequest("Не вказано ім'я файлу ключа (key_file_name)".to_string())
        })?;
        Some(settings::save_uploaded_key(&content, name, &target_mode, store_id)?)
    } else if let Some(path) = key_file_path {
        if !path.is_empty() {
            Some(settings::copy_key_file(&path, &target_mode, store_id)?)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(path) = &key_path {
        let ext = Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        ks.save_key_path(path, if ext.is_empty() { None } else { Some(&ext) })
            .map_err(|e| AdminPrroErr::BadRequest(e.to_string()))?;
    }
    // 2. Пароль ключа (Fernet, master-ключ точки).
    if let Some(pw) = key_password {
        if !pw.is_empty() {
            ks.save_password_encrypted(&pw)
                .map_err(|e| AdminPrroErr::BadRequest(e.to_string()))?;
        }
    }

    // 3. Реквізити ПРРО точки (з валідацією формату, 1:1 settings.rs).
    if let Some(fn_val) = prro_fn {
        let fn_val = fn_val.trim();
        let digits_ok = !fn_val.is_empty()
            && fn_val.len() <= 15
            && fn_val.len() >= 5
            && fn_val.chars().all(|c| c.is_ascii_digit());
        if !digits_ok {
            return Err(AdminPrroErr::BadRequest(format!(
                "Невірний фіскальний номер (prro_fn): очікується 5–15 цифр, отримано '{fn_val}'"
            )));
        }
        put_setting(db, store_id, KEY_PRRO_FN, fn_val).await?;
    }
    if let Some(tn_val) = prro_tn {
        let tn_val = tn_val.trim();
        if !(5..=20).contains(&tn_val.len()) {
            return Err(AdminPrroErr::BadRequest(format!(
                "Невірний податковий номер (prro_tn): очікується 5–20 символів, отримано '{tn_val}'"
            )));
        }
        put_setting(db, store_id, KEY_PRRO_TN, tn_val).await?;
    }
    if let Some(zn_val) = prro_zn {
        let zn_val = zn_val.trim();
        if !(3..=30).contains(&zn_val.len()) {
            return Err(AdminPrroErr::BadRequest(format!(
                "Невірний заводський номер (prro_zn): очікується 3–30 символів, отримано '{zn_val}'"
            )));
        }
        put_setting(db, store_id, KEY_PRRO_ZN, zn_val).await?;
    }
    if let Some(m) = mode {
        put_setting(db, store_id, KEY_PRRO_MODE, m.trim()).await?;
    }
    if let Some(u) = url {
        let u = u.trim();
        if u.is_empty() {
            return Err(AdminPrroErr::BadRequest(
                "Невірний url фіскального сервера: порожній".to_string(),
            ));
        }
        put_setting(db, store_id, KEY_PRRO_URL, u).await?;
    }
    if let Some(af) = auto_fiscalize {
        let v = af.trim().to_lowercase();
        put_setting(
            db,
            store_id,
            "auto_fiscalize",
            if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
                "true"
            } else {
                "false"
            },
        )
        .await?;
    }

    read_store_prro(db, store_id).await
}

pub async fn prro_settings_put(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    AxumPath(store_id): AxumPath<String>,
    mut multipart: Multipart,
) -> Result<Json<StorePrroDto>, AdminPrroErr> {
    let actor_id = require_owner_or_admin(&state, &claims).await?;
    let db = pool(&state)?;
    let store_id = Uuid::parse_str(&store_id)
        .map_err(|_| AdminPrroErr::BadRequest(format!("Невірний store_id: '{store_id}'")))?;
    // 404 раніше за валідацію полів (читання назви нижче в apply).
    sqlx::query_scalar::<_, String>("SELECT name FROM stores WHERE id = $1")
        .bind(store_id)
        .fetch_optional(&db)
        .await?
        .ok_or_else(|| AdminPrroErr::NotFound("Точку не знайдено".to_string()))?;

    let mut key_file_content: Option<Vec<u8>> = None;
    let mut key_file_name: Option<String> = None;
    let mut key_file_path: Option<String> = None;
    let mut key_password: Option<String> = None;
    let mut prro_fn: Option<String> = None;
    let mut prro_tn: Option<String> = None;
    let mut prro_zn: Option<String> = None;
    let mut mode: Option<String> = None;
    let mut url: Option<String> = None;
    let mut auto_fiscalize: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if field.file_name().is_some() {
            key_file_name = field.file_name().map(str::to_string);
            key_file_content = field.bytes().await.ok().map(|b| b.to_vec());
        } else {
            let text = field.text().await.unwrap_or_default();
            match name.as_str() {
                "key_password" => key_password = Some(text),
                "prro_fn" => prro_fn = Some(text),
                "prro_tn" => prro_tn = Some(text),
                "prro_zn" => prro_zn = Some(text),
                "mode" => mode = Some(text),
                "url" => url = Some(text),
                "key_file_path" => key_file_path = Some(text),
                "auto_fiscalize" => auto_fiscalize = Some(text),
                _ => {}
            }
        }
    }

    let saved = apply_put(
        &db,
        store_id,
        key_file_content,
        key_file_name,
        key_file_path,
        key_password,
        prro_fn,
        prro_tn,
        prro_zn,
        mode,
        url,
        auto_fiscalize,
    )
    .await?;

    // Audit-слід (тільки після успішного збереження).
    network::audit(
        &db,
        actor_id,
        "prro_settings_updated",
        "store",
        store_id,
        Some(store_id),
        serde_json::json!({
            "configured": saved.configured,
            "prro_fn": saved.settings.prro_fn,
            "mode": saved.settings.mode,
            "key_file_configured": saved.key.file_configured,
        }),
    )
    .await;

    Ok(Json(saved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scope_is_store_editable() {
        let dto = StorePrroDto {
            store_id: Uuid::nil(),
            store_name: "t".into(),
            scope: "store",
            editable: true,
            reason: None,
            configured: false,
            settings: PrroSettingsView {
                prro_fn: String::new(),
                prro_tn: String::new(),
                prro_zn: String::new(),
                mode: "test".into(),
                url: String::new(),
            },
            key: PrroKeyStatusDto {
                source: "none",
                file_configured: false,
                file_name: None,
                password_configured: false,
                signer_serial: None,
                signer_name: None,
            },
            last_shift: None,
            settings_updated_at: None,
        };
        assert_eq!(dto.scope, "store");
        assert!(dto.editable);
    }
}
