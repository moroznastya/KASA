// ─────────────────────────────────────────────────────────────────────────────
// admin_prro — «ПРРО централізовано» (Етап 5 адмін-панелі власника мережі,
// ТЗ 5.7) — READ-ONLY ОГЛЯД стану фіскального реєстру.
// ─────────────────────────────────────────────────────────────────────────────
// Роут (окремий /admin/* роутер БЕЗ store_middleware; RBAC owner|store_manager
// |admin через auth_routes::require_admin):
//
//   GET /api/v1/admin/stores/:store_id/prro-settings
//       → { store_id, store_name, scope, editable:false, reason,
//           configured, settings{...}, key{...}, last_shift{...}|null }
//
// ── АНОМАЛІЯ (дослідження моделі, перш ніж писати код) ─────────────────────
// Фактична модель ПРРО в PostgreSQL НЕ per-store:
//   • prro_settings  — key-value БЕЗ store_id (глобальний реєстр сервера);
//   • prro_shifts / prro_queue_items — БЕЗ store_id (фіскалізація через
//     ОДИН глобальний реєстр ПРРО);
//   • КЕП (ключ ЕЦП) НЕ зберігається в БД взагалі: це файл ключа + пароль
//     у keystore (.prro_keystore.json, Fernet) або env PRRO_KEY_FILE /
//     PRRO_KEY_PASSWORD (див. torgashka_prro::prro::settings::PrroKeyStore).
//   Ключі prro_settings (torgashka_prro::prro::models): prro_fn (RRO-номер =
//   фіскальний номер ФН), prro_tn (податковий №), prro_zn (заводський №),
//   mode (test/prod), url — глобальні для сервера, НЕ для окремої точки.
//
// Наслідок: централізований PUT «налаштувань ПРРО точки» НЕ підтримується
// моделлю. Запис за сценарієм «store/:id/prro-settings» записав би глобальний
// реєстр (для ВСІХ точок) — це брехня на рівні точки і ламало б інші точки.
// Тому:
//   • реалізовано GET — read-only огляд останнього відомого стану
//     глобального ПРРО-реєстру (той самий стан бачить будь-яка точка);
//   • PUT НЕ реалізовано (свідомо) — див. `editable:false` + reason;
//   • сигналізується вгору: для повноцінного per-store PUT потрібна зміна
//     моделі — store_id (або окрема таблиця store_prro_settings) у
//     prro_settings + прив'язка prro_shifts/queue до точки/каси + розподіл
//     змін через sync до кас (pull-агрегати master-даних).
//
// Безпека ключа ЕЦП: у GET НІКОЛИ не повертається ключ і пароль — лише
// булеві ознаки наявності (file_configured / password_configured) і назва
// файлу (не секрет). signer_serial (серійний № сертифіката) береться з
// prro_shifts — це публічний атрибут сертифіката, не ключовий матеріал.
// ─────────────────────────────────────────────────────────────────────────────
use std::path::Path;

use axum::{
    extract::{Extension, Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::Row;
use torgashka_prro::prro::PrroKeyStore;
use uuid::Uuid;

use crate::{auth::Claims, auth_routes, AppState};

// ─── Помилки → HTTP ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AdminPrroErr {
    Auth(auth_routes::AuthRouteError),
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

impl IntoResponse for AdminPrroErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            AdminPrroErr::Auth(e) => e.into_response(),
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

fn pool(state: &AppState) -> Result<sqlx::PgPool, AdminPrroErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| AdminPrroErr::BadRequest("write_pool не ініціалізовано".to_string()))
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
    /// "env" | "keystore" | "none" — джерело, звідки береться файл ключа.
    pub source: &'static str,
    pub file_configured: bool,
    /// Базова назва файлу ключа (не секрет).
    pub file_name: Option<String>,
    pub password_configured: bool,
    /// Серійний № сертифіката КЕП (з останньої фіскальної зміни; публічний).
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
    /// "global" — модель: один фіскальний реєстр на сервер (без store_id).
    pub scope: &'static str,
    /// false — централізований per-store PUT не підтримується моделлю.
    pub editable: bool,
    /// Людське пояснення обмеження (дублює docstring модуля).
    pub reason: String,
    /// ПРРО готовий до фіскалізації: задані prro_fn та url (+ключ КЕП).
    pub configured: bool,
    pub settings: PrroSettingsView,
    pub key: PrroKeyStatusDto,
    pub last_shift: Option<PrroLastShiftDto>,
    /// Останнє оновлення prro_settings (макс. updated_at; null — жодного ключа).
    pub settings_updated_at: Option<NaiveDateTime>,
}

// ─── GET /api/v1/admin/stores/:store_id/prro-settings ────────────────────────

pub async fn prro_settings(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    AxumPath(store_id): AxumPath<String>,
) -> Result<Json<StorePrroDto>, AdminPrroErr> {
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminPrroErr::Auth)?;
    let db = pool(&state)?;
    let store_id = Uuid::parse_str(&store_id)
        .map_err(|_| AdminPrroErr::BadRequest(format!("Невірний store_id: '{store_id}'")))?;

    let (store_name,): (String,) = sqlx::query_as("SELECT name FROM stores WHERE id = $1")
        .bind(store_id)
        .fetch_optional(&db)
        .await?
        .ok_or_else(|| AdminPrroErr::NotFound("Точку не знайдено".to_string()))?;

    // Глобальні ключі prro_settings (модель: без store_id — один реєстр).
    const KEYS: [&str; 5] = ["prro_fn", "prro_tn", "prro_zn", "mode", "url"];
    let keys_sql: Vec<String> = KEYS.iter().map(|s| s.to_string()).collect();
    let rows = sqlx::query("SELECT key_name, value FROM prro_settings WHERE key_name = ANY($1)")
        .bind(&keys_sql)
        .fetch_all(&db)
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
        sqlx::query_as("SELECT max(updated_at)::timestamp FROM prro_settings")
            .fetch_one(&db)
            .await?;

    // Остання фіскальна зміна (signer — серійний № сертифіката КЕП).
    let last_shift: Option<PrroLastShiftDto> = {
        let row = sqlx::query(
            "SELECT shift_number, status::text AS status, opened_at::timestamp AS opened_at,
                    closed_at::timestamp AS closed_at, receipt_count, zreport_number
             FROM prro_shifts
             ORDER BY opened_at DESC
             LIMIT 1",
        )
        .fetch_optional(&db)
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

    // Стан ключа ЕЦП: БЕЗ секретів — лише наявність. Тільки читання файлів
    // (get_key_path/is_configured не створюють master-ключ, на відміну від
    // decrypt_password — його тут НЕ викликаємо).
    let ks = PrroKeyStore::default();
    let env_file = std::env::var("PRRO_KEY_FILE")
        .ok()
        .filter(|s| !s.is_empty());
    let env_pwd = std::env::var("PRRO_KEY_PASSWORD")
        .ok()
        .filter(|s| !s.is_empty());
    let ks_path = ks.get_key_path().ok();
    let file_path = env_file
        .clone()
        .or_else(|| ks_path.clone().filter(|p| Path::new(p).is_file()));
    let source: &'static str = if env_file.is_some() {
        "env"
    } else if ks_path.is_some() {
        "keystore"
    } else {
        "none"
    };
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
             WHERE signer_serial IS NOT NULL
             ORDER BY opened_at DESC LIMIT 1",
        )
        .fetch_optional(&db)
        .await?;
        match row {
            Some(r) => (r.get("signer_serial"), r.get("signer_name")),
            None => (None, None),
        }
    };

    let configured = !prro_fn.is_empty() && !url.is_empty();

    Ok(Json(StorePrroDto {
        configured,
        store_id,
        store_name,
        scope: "global",
        editable: false,
        reason:
            "Модель зберігає ОДИН глобальний ПРРО-реєстр на сервер (prro_settings/prro_shifts без store_id); \
             КЕП — файл ключа поза БД. Per-store налаштування точки моделлю не підтримуються — показано \
             поточний стан спільного реєстру. Для централізованого per-store PUT потрібна зміна моделі \
             (store_id у prro_settings + розподіл через sync) — сигналізовано вгору."
            .to_string(),
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
    }))
}
