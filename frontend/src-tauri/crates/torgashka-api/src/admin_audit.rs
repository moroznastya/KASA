// ─────────────────────────────────────────────────────────────────────────────
// admin_audit — «Аудит-лог» (Етап 5 адмін-панелі власника мережі, ТЗ 5.9).
// ─────────────────────────────────────────────────────────────────────────────
// Роут (окремий /admin/* роутер БЕЗ store_middleware; RBAC owner|store_manager
// |admin через auth_routes::require_admin, як admin.rs / admin_reports.rs):
//
//   GET /api/v1/admin/audit-log
//       ?from=YYYY-MM-DD&to=YYYY-MM-DD&actor=<uuid>&author=<name-підрядок>
//       &action=<exact>&store_id=<uuid>&page=1&size=25
//       → { items[], total, page, size, pages }
//
// Читання таблиці audit_log (слід адмін-дій: створення/зміна точок, каси,
// працівники — див. network::audit / admin.rs). ТІЛЬКИ перегляд (вимога ТЗ
// 5.9). Записи НЕ редагуються і НЕ видаляються через API.
//
// Семантика (зафіксовано для тестів і UI):
//  • created_at — timestamp WITHOUT time zone (як пише застосунок); фільтр
//    from/to порівнює created_at::date (наївні дати, без зсуву зони).
//  • Пагінація: page≥1, size 1..=100 (default 25), сортування created_at
//    DESC, id DESC (стабільний порядок).
//  • Імена авторів — LEFT JOIN users (поточне ім'я; при видаленому
//    користувачі — NULL); store_name — LEFT JOIN stores (поточна назва).
//  • Усі фільтри опційні; порожній список — 200 з total=0 (не помилка).
//  • action — точний збіг (у БД snake_case: store_updated, worker_created,
//    device_blocked, device_unblocked, device_archived, activation_code_generated);
//    author — підрядок по users.name (ILIKE); actor — exact uuid користувача.
// ─────────────────────────────────────────────────────────────────────────────
use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{auth::Claims, auth_routes, AppState};

// ─── Помилки → HTTP ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AdminAuditErr {
    Auth(auth_routes::AuthRouteError),
    BadRequest(String),
    Db(sqlx::Error),
}

impl From<auth_routes::AuthRouteError> for AdminAuditErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        AdminAuditErr::Auth(e)
    }
}

impl From<sqlx::Error> for AdminAuditErr {
    fn from(e: sqlx::Error) -> Self {
        AdminAuditErr::Db(e)
    }
}

impl IntoResponse for AdminAuditErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            AdminAuditErr::Auth(e) => e.into_response(),
            AdminAuditErr::BadRequest(m) => body(StatusCode::BAD_REQUEST, m),
            AdminAuditErr::Db(e) => {
                eprintln!("[torgashka-api] admin_audit: помилка БД: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"detail": "Внутрішня помилка сервера"})),
                )
                    .into_response()
            }
        }
    }
}

fn pool(state: &AppState) -> Result<sqlx::PgPool, AdminAuditErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| AdminAuditErr::BadRequest("write_pool не ініціалізовано".to_string()))
}

/// Парсер дати YYYY-MM-DD → NaiveDate (400 з текстом помилки).
fn parse_date(raw: &str, field: &str) -> Result<NaiveDate, AdminAuditErr> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        AdminAuditErr::BadRequest(format!("Невірний {field}: '{raw}' — очікується YYYY-MM-DD"))
    })
}

fn parse_uuid(raw: &str, field: &str) -> Result<Uuid, AdminAuditErr> {
    Uuid::parse_str(raw)
        .map_err(|_| AdminAuditErr::BadRequest(format!("Невірний {field}: '{raw}'")))
}

fn parse_page(raw: Option<u32>, field: &str) -> Result<u32, AdminAuditErr> {
    match raw {
        None => Ok(1),
        Some(v) if v >= 1 => Ok(v),
        Some(v) => Err(AdminAuditErr::BadRequest(format!(
            "{field} має бути ≥ 1 (отримано {v})"
        ))),
    }
}

fn parse_size(raw: Option<u32>) -> Result<u32, AdminAuditErr> {
    match raw {
        None => Ok(25),
        Some(v) if (1..=100).contains(&v) => Ok(v),
        Some(v) => Err(AdminAuditErr::BadRequest(format!(
            "size має бути в межах 1..=100 (отримано {v})"
        ))),
    }
}

// ─── Query / DTO ─────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct AuditLogQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    /// exact actor_user_id (uuid).
    pub actor: Option<String>,
    /// підрядок по users.name (ILIKE).
    pub author: Option<String>,
    /// exact action (snake_case, як зберігає network::audit).
    pub action: Option<String>,
    pub store_id: Option<String>,
    pub page: Option<u32>,
    pub size: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AuditLogItemDto {
    pub id: Uuid,
    pub created_at: String,
    pub action: String,
    pub actor_user_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub actor_login: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub store_id: Option<Uuid>,
    pub store_name: Option<String>,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct AuditLogPageDto {
    pub items: Vec<AuditLogItemDto>,
    pub total: i64,
    pub page: u32,
    pub size: u32,
    pub pages: i64,
}

fn fmt_naive(dt: NaiveDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

// ─── GET /api/v1/admin/audit-log ─────────────────────────────────────────────

pub async fn audit_log(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<AuditLogQuery>,
) -> Result<Json<AuditLogPageDto>, AdminAuditErr> {
    auth_routes::require_admin(&state, &claims)
        .await
        .map_err(AdminAuditErr::Auth)?;
    let db = pool(&state)?;

    // Фільтри: 6 опційних параметрів у фіксованому порядку → 6 місць $1..$6.
    let from = match &q.from {
        Some(raw) => Some(parse_date(raw, "from")?),
        None => None,
    };
    let to = match &q.to {
        Some(raw) => Some(parse_date(raw, "to")?),
        None => None,
    };
    if let (Some(f), Some(t)) = (&from, &to) {
        if f > t {
            return Err(AdminAuditErr::BadRequest(format!(
                "from ({f}) пізніше ніж to ({t})"
            )));
        }
    }
    let actor = match &q.actor {
        Some(raw) if !raw.is_empty() => Some(parse_uuid(raw, "actor")?),
        _ => None,
    };
    let store_id = match &q.store_id {
        Some(raw) if !raw.is_empty() => Some(parse_uuid(raw, "store_id")?),
        _ => None,
    };
    let author = q.author.clone().filter(|s| !s.trim().is_empty());
    let action = q.action.clone().filter(|s| !s.trim().is_empty());
    let page = parse_page(q.page, "page")?;
    let size = parse_size(q.size)?;

    // WHERE з 6 фіксованими слотами (NULL → фільтр вимкнено). author — ILIKE
    // по users.name, тому у COUNT теж LEFT JOIN users.
    const WHERE: &str = " WHERE ($1::date IS NULL OR al.created_at::date >= $1)
          AND ($2::date IS NULL OR al.created_at::date <= $2)
          AND ($3::uuid IS NULL OR al.actor_user_id = $3)
          AND ($4::text IS NULL OR u.name ILIKE '%' || $4 || '%')
          AND ($5::text IS NULL OR al.action = $5)
          AND ($6::uuid IS NULL OR al.store_id = $6)";

    let (total,): (i64,) = sqlx::query_as(&format!(
        "SELECT count(*) FROM audit_log al LEFT JOIN users u ON u.id = al.actor_user_id {WHERE}"
    ))
    .bind(from)
    .bind(to)
    .bind(actor)
    .bind(author.clone())
    .bind(action.clone())
    .bind(store_id)
    .fetch_one(&db)
    .await?;

    let offset = (page as i64 - 1) * size as i64;
    let rows = sqlx::query(&format!(
        "SELECT al.id, al.created_at, al.action,
                al.actor_user_id, u.name AS actor_name, u.login AS actor_login,
                al.entity_type, al.entity_id, al.store_id,
                st.name AS store_name, al.payload
         FROM audit_log al
         LEFT JOIN users u  ON u.id  = al.actor_user_id
         LEFT JOIN stores st ON st.id = al.store_id
         {WHERE}
         ORDER BY al.created_at DESC, al.id DESC
         OFFSET $7 LIMIT $8"
    ))
    .bind(from)
    .bind(to)
    .bind(actor)
    .bind(author.clone())
    .bind(action.clone())
    .bind(store_id)
    .bind(offset)
    .bind(size as i64)
    .fetch_all(&db)
    .await?;

    let items = rows
        .iter()
        .map(|r| AuditLogItemDto {
            id: r.get("id"),
            created_at: fmt_naive(r.get::<NaiveDateTime, _>("created_at")),
            action: r.get("action"),
            actor_user_id: r.get("actor_user_id"),
            actor_name: r.get("actor_name"),
            actor_login: r.get("actor_login"),
            entity_type: r.get("entity_type"),
            entity_id: r.get("entity_id"),
            store_id: r.get("store_id"),
            store_name: r.get("store_name"),
            payload: r.get("payload"),
        })
        .collect();

    let pages = if total == 0 {
        0
    } else {
        (total + size as i64 - 1) / size as i64
    };
    Ok(Json(AuditLogPageDto {
        items,
        total,
        page,
        size,
        pages,
    }))
}
