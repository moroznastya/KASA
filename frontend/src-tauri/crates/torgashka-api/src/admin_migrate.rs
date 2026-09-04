// ─────────────────────────────────────────────────────────────────────────────
// admin_migrate — Міграція існуючих інсталяцій (Етап 6, ТЗ §9)
// ─────────────────────────────────────────────────────────────────────────────
//   POST /api/v1/admin/migrate/legacy  (require_admin + role owner|admin)
//
// Сценарій: власник має ОДИНОЧНУ інсталяцію (локальна каса + дані в БД) БЕЗ
// адмін-панелі мережі. Після оновлення ПЗ майстер «Перетворити на мережу»
// викликає цей ендпоінт. Він виконує крок 2 §9:
//   1. ensure stores — перша АКТИВНА точка (якщо точок немає — створює
//      нову; якщо setup виконувався раніше — використовує наявну);
//   2. реєструє поточну касу/сервер як device зі статусом 'active' БЕЗ коду
//      активації (вона вже «своя») і маркером source='legacy_migration';
//   3. пише audit_log запис (action='legacy_migration').
// Дані інсталяції НЕ змінюються — стають видимі через адмін-панель.
//
// ІДЕМПОТЕНТНІСТЬ: на рівні БД — partial UNIQUE INDEX
// ux_devices_legacy_migration_store (один legacy-пристрій на точку) +
// перевірка наявного запису. Повторний виклик НЕ дублює device і не пише
// зайвий audit-запис. Якщо в точки вже є каси (активовані звичайним кодом) —
// новий legacy-запис НЕ створюється (мережа вже налаштована): повертається
// наявний пристрій з created_device=false.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth_routes, network, AppState};

// ─── Помилки → HTTP ({"detail": msg}) ───────────────────────────────────────

#[derive(Debug)]
pub enum MigrateErr {
    Auth(auth_routes::AuthRouteError),
    Forbidden(String),
    BadRequest(String),
    Db(sqlx::Error),
}

impl From<auth_routes::AuthRouteError> for MigrateErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        MigrateErr::Auth(e)
    }
}

impl From<sqlx::Error> for MigrateErr {
    fn from(e: sqlx::Error) -> Self {
        MigrateErr::Db(e)
    }
}

impl IntoResponse for MigrateErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            MigrateErr::Auth(e) => e.into_response(),
            MigrateErr::Forbidden(m) => body(StatusCode::FORBIDDEN, m),
            MigrateErr::BadRequest(m) => body(StatusCode::BAD_REQUEST, m),
            MigrateErr::Db(e) => {
                eprintln!("[torgashka-api] admin_migrate: помилка БД: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": "Внутрішня помилка сервера"})),
                )
                    .into_response()
            }
        }
    }
}

fn pool(state: &AppState) -> Result<PgPool, MigrateErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| MigrateErr::BadRequest("write_pool не ініціалізовано".to_string()))
}

/// Тіло POST /admin/migrate/legacy — обидва поля опційні.
#[derive(Debug, Deserialize, Default)]
pub struct MigrateBody {
    /// Назва точки, якщо жодної ще немає (за замовчуванням «Магазин 1»).
    pub store_name: Option<String>,
    /// Назва пристрою (за замовчуванням «{store} — основна каса»).
    pub device_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MigrateDeviceDto {
    pub id: Uuid,
    pub store_id: Uuid,
    pub name: String,
    pub status: String,
    /// 'legacy_migration' — пристрій, зареєстрований міграцією §9;
    /// None — пристрій, що вже існував (звичайна активація).
    pub source: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MigrateResponse {
    /// true — точку створила міграція (в БД не було жодної).
    pub created_store: bool,
    pub store: MigrateStoreDto,
    /// true — зареєстровано новий legacy-пристрій.
    pub created_device: bool,
    pub device: MigrateDeviceDto,
}

#[derive(Debug, Serialize)]
pub struct MigrateStoreDto {
    pub id: Uuid,
    pub name: String,
}

fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(s.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// POST /api/v1/admin/migrate/legacy — перетворити одиночну інсталяцію на
/// мережу (крок 2 §9). Ідемпотентно (див. шапку модуля).
pub async fn migrate_legacy(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(body): Json<MigrateBody>,
) -> Result<Json<MigrateResponse>, MigrateErr> {
    let db = pool(&state)?;
    // JWT-роль (без запиту в БД) + обов'язково owner|admin (не store_manager).
    let actor_id = auth_routes::require_admin(&state, &claims).await?;
    if !matches!(claims.role.as_str(), "owner" | "admin") {
        return Err(MigrateErr::Forbidden(
            "Міграція legacy-інсталяції доступна лише власнику мережі".to_string(),
        ));
    }
    let default_store_name = body.store_name.filter(|s| !s.trim().is_empty());
    let store_name = default_store_name.unwrap_or_else(|| "Магазин 1".to_string());

    // ── Крок 1: ensure stores (перша АКТИВНА точка; інакше — створити) ──
    let mut created_store = false;
    let mut store: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM stores WHERE is_active = true \
         ORDER BY created_at, id LIMIT 1",
    )
    .fetch_optional(&db)
    .await?;
    if store.is_none() {
        store = Some(
            sqlx::query_as(
                "INSERT INTO stores (name, is_active, created_at, updated_at) \
                 VALUES ($1, true, now(), now()) RETURNING id, name",
            )
            .bind(&store_name)
            .fetch_one(&db)
            .await?,
        );
        created_store = true;
    }
    let (store_id, store_name) = store.expect("store визначено вище");

    // ── Крок 2: існуючий legacy-пристрій точки? → повертаємо як є ──
    let existing: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, name, status::text FROM devices \
         WHERE store_id = $1 AND source = 'legacy_migration' LIMIT 1",
    )
    .bind(store_id)
    .fetch_optional(&db)
    .await?;
    if let Some((dev_id, dev_name, dev_status)) = existing {
        return Ok(Json(MigrateResponse {
            created_store,
            created_device: false,
            store: MigrateStoreDto {
                id: store_id,
                name: store_name,
            },
            device: MigrateDeviceDto {
                id: dev_id,
                store_id,
                name: dev_name,
                status: dev_status,
                source: Some("legacy_migration".to_string()),
            },
        }));
    }

    // ── Крок 3: точка вже має каси (звичайна активація) → не дублюємо ──
    let any_device: Option<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, name, status::text FROM devices WHERE store_id = $1 LIMIT 1")
            .bind(store_id)
            .fetch_optional(&db)
            .await?;
    if let Some((dev_id, dev_name, dev_status)) = any_device {
        return Ok(Json(MigrateResponse {
            created_store,
            created_device: false,
            store: MigrateStoreDto {
                id: store_id,
                name: store_name,
            },
            device: MigrateDeviceDto {
                id: dev_id,
                store_id,
                name: dev_name,
                status: dev_status,
                source: None,
            },
        }));
    }

    // ── Крок 4: реєстрація локальної каси БЕЗ коду активації ──
    // device_token_hash: обов'язкова колонка БД; legacy-пристрій не має
    // виданого токена (автентифікація каси лишається legacy X-Store-Id) —
    // зберігаємо sha256 випадкового значення, яке нікому не видається.
    let token_discard = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token_hash = sha256_hex(&token_discard);
    let device_name = body
        .device_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{store_name} — основна каса"));

    // ON CONFLICT DO NOTHING + partial UNIQUE (source) → race-safe ідемпотентність.
    let inserted: Option<(Uuid, String)> = sqlx::query_as(
        "INSERT INTO devices (id, store_id, name, device_token_hash, source, status, \
                              activated_at, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, 'legacy_migration', 'active'::public.device_status, \
                 now(), now(), now()) \
         ON CONFLICT DO NOTHING RETURNING id, name",
    )
    .bind(Uuid::new_v4())
    .bind(store_id)
    .bind(&device_name)
    .bind(&token_hash)
    .fetch_optional(&db)
    .await?;

    let (dev_id, dev_name, created_device) = match inserted {
        Some((id, name)) => (id, name, true),
        // Гонка: інший виклик уже створив legacy-пристрій — повертаємо його.
        None => {
            let (id, name) = sqlx::query_as(
                "SELECT id, name FROM devices \
                 WHERE store_id = $1 AND source = 'legacy_migration' LIMIT 1",
            )
            .bind(store_id)
            .fetch_one(&db)
            .await?;
            (id, name, false)
        }
    };

    // ── Крок 5: audit-слід (тільки при реальному створенні) ──
    if created_device {
        network::audit(
            &db,
            actor_id,
            "legacy_migration",
            "device",
            dev_id,
            Some(store_id),
            serde_json::json!({
                "source": "legacy_migration",
                "created_store": created_store,
                "created_device": true,
            }),
        )
        .await;
    }

    Ok(Json(MigrateResponse {
        created_store,
        created_device,
        store: MigrateStoreDto {
            id: store_id,
            name: store_name,
        },
        device: MigrateDeviceDto {
            id: dev_id,
            store_id,
            name: dev_name,
            status: "active".to_string(),
            source: Some("legacy_migration".to_string()),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_fallback() {
        let body = MigrateBody::default();
        assert_eq!(
            body.store_name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Магазин 1".to_string()),
            "Магазин 1"
        );
        assert_eq!(
            body.device_name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("{} — основна каса", "Мій магазин")),
            "Мій магазин — основна каса"
        );
    }
}
