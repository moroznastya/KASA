// ─────────────────────────────────────────────────────────────────────────────
// admin — Адмін-панель власника мережі (Етап 1, ТЗ розділи 5.1–5.3)
// ─────────────────────────────────────────────────────────────────────────────
// Роути (окремий /admin/* роутер БЕЗ store_middleware — глобальні дії
// власника мережі, не прив'язані до X-Store-Id):
//
//   GET    /api/v1/admin/stores                        → [AdminStoreDto]
//   POST   /api/v1/admin/stores                        → створити точку + автоприв'язка
//   GET    /api/v1/admin/stores/:store_id              → AdminStoreDto (з лічильниками)
//   PUT    /api/v1/admin/stores/:store_id              → редагування (у т.ч. legal_name/edrpou)
//   DELETE /api/v1/admin/stores/:store_id              → АРХІВАЦІЯ (is_active=false)
//   GET    /api/v1/admin/stores/:store_id/workers      → працівники точки
//   POST   /api/v1/admin/stores/:store_id/workers      → створити працівника + прив'язка
//   POST   /api/v1/admin/users/:user_id/deactivate     → is_active=false (БЕЗ видалення)
//   POST   /api/v1/admin/users/:user_id/activate       → is_active=true
//   POST   /api/v1/admin/users/:user_id/reset-password → новий пароль
//   POST   /api/v1/admin/users/:user_id/reset-pin      → новий PIN
//
// RBAC: /admin/* — лише role owner|store_manager|admin (JWT claims),
// перевірка в auth_routes::require_admin (без запиту в БД).
//
// ПОВЕДІНКА АРХІВАЦІЇ ТОЧКИ (визначена і зафіксована тестом):
//   DELETE /admin/stores/:id → is_active=false. Якщо до точки прив'язані
//   НЕ архівовані каси (pending|active|blocked) — вони АРХІВУЮТЬСЯ разом
//   (status='deleted'; рядок у БД лишається). Архівована каса отримує 403 від
//   sync (auth.rs authenticate_device: "Пристрій заблоковано або видалено") —
//   це робить попередження UI «прив'язано N кас — після архівації вони
//   перестануть синхронізуватись» буквально правдивим. Відповідь містить
//   лічильник archived_devices. Фізичного видалення точок/кас НЕМАЄ.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use torgashka_domain::{AuthError, UserCreateInput, UserDto, UserUpdateInput};

use crate::{
    auth::{create_access_token, Claims},
    auth_routes, network, AppState,
};

// ─── Помилки → HTTP ({"detail": msg}, як решта модулів фасаду) ──────────────

#[derive(Debug)]
pub enum AdminErr {
    Auth(auth_routes::AuthRouteError),
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Db(sqlx::Error),
}

impl From<auth_routes::AuthRouteError> for AdminErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        AdminErr::Auth(e)
    }
}

impl From<AuthError> for AdminErr {
    fn from(e: AuthError) -> Self {
        AdminErr::Auth(auth_routes::AuthRouteError::Plain(e))
    }
}

impl From<sqlx::Error> for AdminErr {
    fn from(e: sqlx::Error) -> Self {
        AdminErr::Db(e)
    }
}

impl IntoResponse for AdminErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            AdminErr::Auth(e) => e.into_response(),
            AdminErr::BadRequest(m) => body(StatusCode::BAD_REQUEST, m),
            AdminErr::NotFound(m) => body(StatusCode::NOT_FOUND, m),
            AdminErr::Conflict(m) => body(StatusCode::CONFLICT, m),
            AdminErr::Db(e) => {
                eprintln!("[torgashka-api] admin: помилка БД: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": "Внутрішня помилка сервера"})),
                )
                    .into_response()
            }
        }
    }
}

/// Пул PostgreSQL фасаду (як network.rs — адмін-таблиці в тій самій схемі).
fn pool(state: &AppState) -> Result<sqlx::PgPool, AdminErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| AdminErr::BadRequest("write_pool не ініціалізовано".to_string()))
}

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, AdminErr> {
    Uuid::parse_str(&raw)
        .map_err(|_| AdminErr::BadRequest(format!("Невірний {field}: '{raw}' — очікується UUID")))
}

/// AuthService (Arc<dyn>) — через auth_routes::auth_repo (TORGASHKA_RUST_AUTH=1).
fn auth_svc(
    state: &AppState,
) -> Result<std::sync::Arc<dyn torgashka_domain::AuthService + Send + Sync>, AdminErr> {
    auth_routes::auth_repo(state).map_err(AdminErr::Auth)
}

// ─── DTO ────────────────────────────────────────────────────────────────────

/// Точка на рівні адмін-панелі власника (глобальний огляд; без user_stores.role —
/// адмінка працює з усіма точками мережі, RLS не задіяна через write_pool).
#[derive(Debug, Serialize)]
pub struct AdminStoreDto {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Юрособа/ФОП (ПРРО-вкладка; Етапи 4-6 — заглушка).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legal_name: Option<String>,
    /// Код ЄДРПОУ/ІПН (ПРРО-вкладка).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edrpou: Option<String>,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    /// Кількість НЕ архівованих кас (pending|active|blocked).
    pub devices_count: i64,
    /// Кількість прив'язаних працівників (user_stores).
    pub workers_count: i64,
}

/// Створення/редагування точки (адмін-рівень).
#[derive(Debug, Deserialize)]
pub struct AdminStoreUpsert {
    pub name: String,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub legal_name: Option<String>,
    #[serde(default)]
    pub edrpou: Option<String>,
    /// Лише для PUT: опційна зміна статусу (архівація/відновлення).
    #[serde(default)]
    pub is_active: Option<bool>,
}

/// Працівник точки (user_stores × users).
#[derive(Debug, Serialize)]
pub struct WorkerDto {
    pub id: Uuid,
    pub name: String,
    pub login: String,
    /// Глобальна роль (users.role): owner|store_manager|admin|cashier.
    pub role: String,
    pub is_active: bool,
    /// Роль на ЦІЙ точці (user_stores.role).
    pub store_role: String,
    pub is_default: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Тіло POST /admin/stores/:id/workers — створення працівника і прив'язка.
#[derive(Debug, Deserialize)]
pub struct WorkerCreateBody {
    pub name: String,
    #[serde(default)]
    pub login: Option<String>,
    pub password: String,
    #[serde(default)]
    pub pin_code: Option<String>,
    /// Глобальна роль: store_manager|admin|cashier (owner — лише через setup/БД).
    #[serde(default)]
    pub role: Option<String>,
    /// Роль на цій точці (user_stores.role); за замовчуванням = role.
    #[serde(default)]
    pub store_role: Option<String>,
}

fn parse_global_role(v: Option<&str>) -> Result<&'static str, AdminErr> {
    match v.unwrap_or("cashier").trim() {
        "store_manager" => Ok("store_manager"),
        "admin" => Ok("admin"),
        "cashier" => Ok("cashier"),
        other => Err(AdminErr::BadRequest(format!(
            "Невідома роль '{other}': очікується store_manager | admin | cashier (owner створюється лише через налаштування системи)"
        ))),
    }
}

fn parse_store_role(v: Option<&str>, fallback: &str) -> Result<&'static str, AdminErr> {
    match v.unwrap_or(fallback).trim() {
        "owner" => Ok("owner"),
        "store_manager" => Ok("store_manager"),
        "admin" => Ok("admin"),
        "cashier" => Ok("cashier"),
        other => Err(AdminErr::BadRequest(format!(
            "Невідома роль на точці '{other}': очікується owner | store_manager | admin | cashier"
        ))),
    }
}

// ─── Маппінг рядка → AdminStoreDto ───────────────────────────────────────────

fn store_dto_from_row(row: &sqlx::postgres::PgRow) -> AdminStoreDto {
    AdminStoreDto {
        id: row.get("id"),
        name: row.get("name"),
        address: row.try_get("address").ok().flatten(),
        phone: row.try_get("phone").ok().flatten(),
        legal_name: row.try_get("legal_name").ok().flatten(),
        edrpou: row.try_get("edrpou").ok().flatten(),
        is_active: row.try_get("is_active").unwrap_or(true),
        created_at: row
            .try_get("created_at")
            .unwrap_or_else(|_| chrono::Utc::now().naive_utc()),
        updated_at: row
            .try_get("updated_at")
            .unwrap_or_else(|_| chrono::Utc::now().naive_utc()),
        devices_count: row.try_get("devices_count").unwrap_or(0),
        workers_count: row.try_get("workers_count").unwrap_or(0),
    }
}

const STORE_SELECT: &str = r#"
    SELECT s.id, s.name, s.address, s.phone, s.legal_name, s.edrpou,
           s.is_active, s.created_at, s.updated_at,
           (SELECT count(*) FROM devices d
             WHERE d.store_id = s.id AND d.status::text <> 'deleted') AS devices_count,
           (SELECT count(*) FROM user_stores us WHERE us.store_id = s.id) AS workers_count
    FROM stores s
"#;

// ─── GET /api/v1/admin/stores ────────────────────────────────────────────────

pub async fn list_stores(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<AdminStoreDto>>, AdminErr> {
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let db = pool(&state)?;
    let rows = sqlx::query(&format!("{STORE_SELECT} ORDER BY s.created_at ASC"))
        .fetch_all(&db)
        .await?;
    Ok(Json(rows.iter().map(store_dto_from_row).collect()))
}

// ─── GET /api/v1/admin/stores/:store_id ─────────────────────────────────────

pub async fn get_store(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(store_id): Path<String>,
) -> Result<Json<AdminStoreDto>, AdminErr> {
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let db = pool(&state)?;
    let store_id = path_uuid(store_id, "store_id")?;
    let row = sqlx::query(&format!("{STORE_SELECT} WHERE s.id = $1"))
        .bind(store_id)
        .fetch_optional(&db)
        .await?;
    match row {
        Some(r) => Ok(Json(store_dto_from_row(&r))),
        None => Err(AdminErr::NotFound("Точку не знайдено".to_string())),
    }
}

// ─── POST /api/v1/admin/stores ──────────────────────────────────────────────

pub async fn create_store(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<AdminStoreUpsert>,
) -> Result<(StatusCode, Json<AdminStoreDto>), AdminErr> {
    let actor = auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let db = pool(&state)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AdminErr::BadRequest(
            "Назва точки не може бути порожньою".to_string(),
        ));
    }
    let mut tx = db.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO stores (name, address, phone, legal_name, edrpou, is_active,
                            created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, true, (now() AT TIME ZONE 'UTC')::timestamp,
                (now() AT TIME ZONE 'UTC')::timestamp)
        RETURNING id
        "#,
    )
    .bind(name)
    .bind(body.address.as_deref())
    .bind(body.phone.as_deref())
    .bind(body.legal_name.as_deref())
    .bind(body.edrpou.as_deref())
    .fetch_one(&mut *tx)
    .await?;
    let store_id: Uuid = row.get("id");
    // Автоприв'язка творця як власника точки (адмін-контекст без X-Store-Id).
    sqlx::query(
        r#"
        INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
        VALUES ($1, $2, 'owner', '{}'::jsonb, false, (now() AT TIME ZONE 'UTC')::timestamp)
        ON CONFLICT (user_id, store_id) DO NOTHING
        "#,
    )
    .bind(actor)
    .bind(store_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let full = sqlx::query(&format!("{STORE_SELECT} WHERE s.id = $1"))
        .bind(store_id)
        .fetch_one(&db)
        .await?;
    Ok((StatusCode::CREATED, Json(store_dto_from_row(&full))))
}

// ─── PUT /api/v1/admin/stores/:store_id ─────────────────────────────────────

pub async fn update_store(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(store_id): Path<String>,
    Json(body): Json<AdminStoreUpsert>,
) -> Result<Json<AdminStoreDto>, AdminErr> {
    let actor = auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let db = pool(&state)?;
    let store_id = path_uuid(store_id, "store_id")?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AdminErr::BadRequest(
            "Назва точки не може бути порожньою".to_string(),
        ));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stores WHERE id = $1)")
        .bind(store_id)
        .fetch_one(&db)
        .await?;
    if !exists {
        return Err(AdminErr::NotFound("Точку не знайдено".to_string()));
    }
    let row = sqlx::query(
        r#"
        UPDATE stores
        SET name = $2, address = $3, phone = $4, legal_name = $5, edrpou = $6,
            is_active = COALESCE($7, is_active),
            updated_at = (now() AT TIME ZONE 'UTC')::timestamp
        WHERE id = $1
        RETURNING id
        "#,
    )
    .bind(store_id)
    .bind(name)
    .bind(body.address.as_deref())
    .bind(body.phone.as_deref())
    .bind(body.legal_name.as_deref())
    .bind(body.edrpou.as_deref())
    .bind(body.is_active)
    .execute(&db)
    .await?;
    if row.rows_affected() == 0 {
        return Err(AdminErr::NotFound("Точку не знайдено".to_string()));
    }
    network::audit(
        &db,
        actor,
        "store_updated",
        "store",
        store_id,
        Some(store_id),
        serde_json::json!({"name": name}),
    )
    .await;
    let full = sqlx::query(&format!("{STORE_SELECT} WHERE s.id = $1"))
        .bind(store_id)
        .fetch_one(&db)
        .await?;
    Ok(Json(store_dto_from_row(&full)))
}

// ─── DELETE /api/v1/admin/stores/:store_id (АРХІВАЦІЯ) ──────────────────────
// Поведінка зафіксована: архівація каскадно архівує прив'язані не-архівовані
// каси (status='deleted') — після чого вони перестають синхронізуватись (403).
// Фізичного видалення точок/кас немає (безпека даних: каскадні FK).

pub async fn archive_store(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(store_id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminErr> {
    let actor = auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let db = pool(&state)?;
    let store_id = path_uuid(store_id, "store_id")?;

    let mut tx = db.begin().await?;
    let updated = sqlx::query(
        r#"
        UPDATE stores
        SET is_active = false, updated_at = (now() AT TIME ZONE 'UTC')::timestamp
        WHERE id = $1
        "#,
    )
    .bind(store_id)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AdminErr::NotFound("Точку не знайдено".to_string()));
    }
    // Каскадна архівація кас точки (повторний виклик — ідемпотентний).
    let archived: i64 = sqlx::query_scalar(
        r#"
        WITH archived AS (
            UPDATE devices
            SET status = 'deleted'::public.device_status,
                updated_at = (now() AT TIME ZONE 'UTC')::timestamp
            WHERE store_id = $1 AND status::text <> 'deleted'
            RETURNING id
        )
        SELECT count(*) FROM archived
        "#,
    )
    .bind(store_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    let full = sqlx::query(&format!("{STORE_SELECT} WHERE s.id = $1"))
        .bind(store_id)
        .fetch_one(&db)
        .await?;
    let dto = store_dto_from_row(&full);
    let warning = if archived > 0 {
        Some(format!(
            "Прив'язано {archived} кас — після архівації вони перестануть синхронізуватись"
        ))
    } else {
        None
    };
    network::audit(
        &db,
        actor,
        "store_archived",
        "store",
        store_id,
        Some(store_id),
        serde_json::json!({"archived_devices": archived}),
    )
    .await;
    Ok(Json(serde_json::json!({
        "store": dto,
        "archived_devices": archived,
        "warning": warning,
    })))
}

// ─── GET /api/v1/admin/stores/:store_id/workers ─────────────────────────────

pub async fn list_workers(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(store_id): Path<String>,
) -> Result<Json<Vec<WorkerDto>>, AdminErr> {
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let db = pool(&state)?;
    let store_id = path_uuid(store_id, "store_id")?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stores WHERE id = $1)")
        .bind(store_id)
        .fetch_one(&db)
        .await?;
    if !exists {
        return Err(AdminErr::NotFound("Точку не знайдено".to_string()));
    }
    let rows = sqlx::query(
        r#"
        SELECT u.id, u.name, u.login, u.role::text AS role, u.is_active,
               us.role AS store_role, us.is_default, u.created_at, u.updated_at
        FROM user_stores us
        JOIN users u ON u.id = us.user_id
        WHERE us.store_id = $1
        ORDER BY u.name ASC
        "#,
    )
    .bind(store_id)
    .fetch_all(&db)
    .await?;
    Ok(Json(
        rows.iter()
            .map(|r| WorkerDto {
                id: r.get("id"),
                name: r.get("name"),
                login: r.get("login"),
                role: r.get("role"),
                is_active: r.try_get("is_active").unwrap_or(true),
                store_role: r
                    .try_get("store_role")
                    .unwrap_or_else(|_| "cashier".to_string()),
                is_default: r.try_get("is_default").unwrap_or(false),
                created_at: r
                    .try_get("created_at")
                    .unwrap_or_else(|_| chrono::Utc::now().naive_utc()),
                updated_at: r
                    .try_get("updated_at")
                    .unwrap_or_else(|_| chrono::Utc::now().naive_utc()),
            })
            .collect(),
    ))
}

// ─── POST /api/v1/admin/stores/:store_id/workers ────────────────────────────
// Створює працівника (users) + прив'язує до точки (user_stores). Роль глобальна
// — store_manager|admin|cashier; роль на точці — store_role (default = глобальна).

pub async fn create_worker(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(store_id): Path<String>,
    Json(body): Json<WorkerCreateBody>,
) -> Result<(StatusCode, Json<WorkerDto>), AdminErr> {
    let actor = auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let db = pool(&state)?;
    let store_id = path_uuid(store_id, "store_id")?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AdminErr::BadRequest(
            "Ім'я працівника не може бути порожнім".to_string(),
        ));
    }
    let role = parse_global_role(body.role.as_deref())?;
    let store_role = parse_store_role(body.store_role.as_deref(), role)?;

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stores WHERE id = $1)")
        .bind(store_id)
        .fetch_one(&db)
        .await?;
    if !exists {
        return Err(AdminErr::NotFound("Точку не знайдено".to_string()));
    }

    let svc = auth_svc(&state)?;
    let created = svc
        .create_user(&UserCreateInput {
            name: name.to_string(),
            login: body.login.clone(),
            password: body.password.clone(),
            pin_code: body.pin_code.clone(),
            role: torgashka_domain::UserRole::parse(role)
                .unwrap_or(torgashka_domain::UserRole::Cashier),
            is_active: true,
            permissions: None,
        })
        .await?;

    sqlx::query(
        r#"
        INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
        VALUES ($1, $2, $3, '{}'::jsonb, false, (now() AT TIME ZONE 'UTC')::timestamp)
        ON CONFLICT (user_id, store_id) DO UPDATE
            SET role = EXCLUDED.role, is_default = EXCLUDED.is_default
        "#,
    )
    .bind(created.id)
    .bind(store_id)
    .bind(store_role)
    .execute(&db)
    .await?;

    network::audit(
        &db,
        actor,
        "worker_created",
        "user",
        created.id,
        Some(store_id),
        serde_json::json!({"name": name, "role": role, "store_role": store_role}),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(WorkerDto {
            id: created.id,
            name: created.name,
            login: created.login,
            role: created.role,
            is_active: created.is_active,
            store_role: store_role.to_string(),
            is_default: false,
            created_at: created.created_at,
            updated_at: created.updated_at,
        }),
    ))
}

// ─── POST /api/v1/admin/users/:user_id/deactivate|activate ──────────────────
// Деактивація = is_active=false. Рядок users у БД ЗАЛИШАЄТЬСЯ (на відміну від
// фізичного DELETE /users/:id). Повторна активація — is_active=true.

pub async fn deactivate_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Result<Json<UserDto>, AdminErr> {
    set_user_active(&state, &claims, user_id, false, "деактивувати").await
}

pub async fn activate_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Result<Json<UserDto>, AdminErr> {
    set_user_active(&state, &claims, user_id, true, "активувати").await
}

async fn set_user_active(
    state: &AppState,
    claims: &Claims,
    raw_id: String,
    is_active: bool,
    verb: &str,
) -> Result<Json<UserDto>, AdminErr> {
    let actor = auth_routes::require_admin(state, claims)
        .await
        .map_err(AdminErr::Auth)?;
    let user_id = path_uuid(raw_id, "user_id")?;
    if user_id == actor {
        return Err(AdminErr::Conflict(format!("Неможливо {verb} самого себе")));
    }
    let svc = auth_svc(state)?;
    let user = svc
        .update_user(
            user_id,
            &UserUpdateInput {
                is_active: Some(is_active),
                ..Default::default()
            },
        )
        .await?;
    Ok(Json(user))
}

// ─── POST /api/v1/admin/users/:user_id/reset-password | reset-pin ───────────

#[derive(Debug, Deserialize)]
pub struct PasswordBody {
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct PinBody {
    pub pin_code: String,
}

pub async fn reset_password(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
    Json(body): Json<PasswordBody>,
) -> Result<Json<UserDto>, AdminErr> {
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let user_id = path_uuid(user_id, "user_id")?;
    let password = body.password.trim();
    if password.chars().count() < 4 {
        return Err(AdminErr::BadRequest(
            "Пароль має містити щонайменше 4 символи".to_string(),
        ));
    }
    let svc = auth_svc(&state)?;
    let user = svc
        .update_user(
            user_id,
            &UserUpdateInput {
                password: Some(password.to_string()),
                ..Default::default()
            },
        )
        .await?;
    Ok(Json(user))
}

pub async fn reset_pin(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
    Json(body): Json<PinBody>,
) -> Result<Json<UserDto>, AdminErr> {
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminErr::Auth)?;
    let user_id = path_uuid(user_id, "user_id")?;
    let pin = body.pin_code.trim();
    let len = pin.chars().count();
    if !(4..=10).contains(&len) {
        return Err(AdminErr::BadRequest(
            "PIN має містити від 4 до 10 символів".to_string(),
        ));
    }
    let svc = auth_svc(&state)?;
    let user = svc
        .update_user(
            user_id,
            &UserUpdateInput {
                pin_code: Some(pin.to_string()),
                ..Default::default()
            },
        )
        .await?;
    Ok(Json(user))
}

/// Пере-випуск JWT не потрібен для цілей Етапа 1 — підтримка токена залишена
/// хелпером create_access_token (використовується в тестах/admin для симетрії).
#[allow(dead_code)]
fn _issue_access(state: &AppState, user: &UserDto) -> Result<String, AdminErr> {
    create_access_token(
        &user.id.to_string(),
        &user.role,
        &user.effective_permissions(),
        &state.jwt_secret,
    )
    .map_err(|e| AdminErr::BadRequest(format!("JWT: {e}")))
}

// ─── Юніт-тести (без БД) ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_role_parsing() {
        assert_eq!(
            parse_global_role(Some("store_manager")).unwrap(),
            "store_manager"
        );
        assert_eq!(parse_global_role(Some("admin")).unwrap(), "admin");
        assert_eq!(parse_global_role(Some("cashier")).unwrap(), "cashier");
        assert_eq!(parse_global_role(None).unwrap(), "cashier");
        assert!(
            parse_global_role(Some("owner")).is_err(),
            "owner через setup/БД"
        );
        assert!(parse_global_role(Some("root")).is_err());
    }

    #[test]
    fn store_role_parsing() {
        assert_eq!(parse_store_role(Some("owner"), "cashier").unwrap(), "owner");
        assert_eq!(
            parse_store_role(Some("store_manager"), "cashier").unwrap(),
            "store_manager"
        );
        assert_eq!(parse_store_role(None, "admin").unwrap(), "admin");
        assert!(parse_store_role(Some("root"), "cashier").is_err());
    }

    #[test]
    fn uuid_path_validation() {
        assert!(path_uuid("не-uuid".to_string(), "store_id").is_err());
        let u = Uuid::new_v4();
        assert_eq!(path_uuid(u.to_string(), "store_id").unwrap(), u);
    }
}
