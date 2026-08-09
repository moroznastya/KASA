// ─────────────────────────────────────────────────────────────────────────────
// categories_v2 — Rust-гілка категорій v2 (дезактивація Python, CRIT-роути).
// 1:1 з Python backend/app/api/v2/categories.py:
//   GET    /api/v2/categories            — список (page/size/search) + total
//   GET    /api/v2/categories/tree       — дерево (рекурсія по parent_id)
//   GET    /api/v2/categories/{id}       — деталі (404)
//   POST   /api/v2/categories            — створення (201; 400 exists, 404 parent)
//   PUT    /api/v2/categories/{id}       — оновлення (404; 400 self-parent/exists)
//   DELETE /api/v2/categories/{id}       — видалення (204; 404)
// Авторизація: JWT глобально (як Python v2 — без require_admin).
// Монтуються під TORGASHKA_RUST_READDIRS=1; інакше — fallback → 410 (дезактивація).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use torgashka_domain::{CategoryCreateInput, CategoryDto, CategoryUpdateInput, WriteError};

use crate::AppState;

/// Помилки хендлерів категорій v2 → HTTP (1:1 з Python v2).
#[derive(Debug)]
pub enum CatV2Error {
    /// 404/400 — з WriteError (NotFound → 404, BadRequest → 400).
    Service(WriteError),
    /// 422 Pydantic-валідація.
    Validation(Value),
    /// 403 — Rust-гілка вимкнена (не має статись).
    Forbidden(String),
}

impl From<WriteError> for CatV2Error {
    fn from(e: WriteError) -> Self {
        CatV2Error::Service(e)
    }
}

impl From<torgashka_domain::DirectoryError> for CatV2Error {
    fn from(e: torgashka_domain::DirectoryError) -> Self {
        match e {
            torgashka_domain::DirectoryError::NotFound(msg) => {
                CatV2Error::Service(WriteError::NotFound(msg))
            }
            other => CatV2Error::Service(WriteError::BadRequest(other.to_string())),
        }
    }
}

fn v422(vtype: &str, loc: &[&str], msg: &str, input: &str) -> CatV2Error {
    CatV2Error::Validation(serde_json::json!({
        "detail": [{
            "type": vtype,
            "loc": loc,
            "msg": msg,
            "input": input,
        }]
    }))
}

impl IntoResponse for CatV2Error {
    fn into_response(self) -> Response {
        match self {
            CatV2Error::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            CatV2Error::Service(WriteError::NotFound(msg)) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            CatV2Error::Service(WriteError::BadRequest(msg)) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
            CatV2Error::Service(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"detail": e.to_string()})),
            )
                .into_response(),
            CatV2Error::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"detail": msg})),
            )
                .into_response(),
        }
    }
}

fn read_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn torgashka_domain::ReadDirectories + Send + Sync>, CatV2Error> {
    state
        .readdirs
        .clone()
        .ok_or_else(|| CatV2Error::Forbidden("Rust-гілка довідників вимкнена".to_string()))
}

fn write_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn torgashka_domain::WriteDirectories + Send + Sync>, CatV2Error> {
    state
        .write
        .clone()
        .ok_or_else(|| CatV2Error::Forbidden("Rust-гілка довідників вимкнена".to_string()))
}

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, CatV2Error> {
    Uuid::parse_str(&raw).map_err(|_| {
        v422(
            "uuid_parsing",
            &["path", field],
            "Input should be a valid UUID, invalid character: expected an optional prefix of `urn:uuid:` followed by [0-9a-fA-F-], found `...` at position 0",
            &raw,
        )
    })
}

// ─── Відповіді (1:1 з Python v2 CategoryResponse/CategoryTreeResponse) ─────

fn cat_v2(c: &CategoryDto) -> Value {
    // sort_order/is_active — константи: у БД колонок немає (як Python ORM-модель,
    // яка повертає дефолти 0/true через Pydantic).
    serde_json::json!({
        "id": c.id,
        "name": c.name,
        "parent_id": c.parent_id,
        "description": c.description,
        "sort_order": 0,
        "is_active": true,
    })
}

fn tree_node(c: &CategoryDto, children: Vec<Value>) -> Value {
    serde_json::json!({
        "id": c.id,
        "name": c.name,
        "parent_id": c.parent_id,
        "description": c.description,
        "children": children,
    })
}

// ─── Парсери тіла (Pydantic-валідація 1:1) ─────────────────────────────────

fn parse_create(v: &Value) -> Result<CategoryCreateInput, CatV2Error> {
    let name = v
        .get("name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| v422("missing", &["body", "name"], "Field required", ""))?;
    if name.is_empty() {
        return Err(v422(
            "string_too_short",
            &["body", "name"],
            "String should have at least 1 character",
            name,
        ));
    }
    if name.chars().count() > 255 {
        return Err(v422(
            "string_too_long",
            &["body", "name"],
            "String should have at most 255 characters",
            name,
        ));
    }
    Ok(CategoryCreateInput {
        name: name.to_string(),
        // Python: entity description має дефолт "" → завжди зберігається "".
        description: Some(
            v.get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string(),
        ),
        parent_id: v
            .get("parent_id")
            .and_then(|p| p.as_str())
            .map(|s| {
                Uuid::parse_str(s).map_err(|_| {
                    v422(
                        "uuid_parsing",
                        &["body", "parent_id"],
                        "Input should be a valid UUID",
                        s,
                    )
                })
            })
            .transpose()?,
    })
}

fn parse_update(v: &Value) -> Result<CategoryUpdateInput, CatV2Error> {
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .map(|s| {
            if s.is_empty() {
                Err(v422(
                    "string_too_short",
                    &["body", "name"],
                    "String should have at least 1 character",
                    s,
                ))
            } else if s.chars().count() > 255 {
                Err(v422(
                    "string_too_long",
                    &["body", "name"],
                    "String should have at most 255 characters",
                    s,
                ))
            } else {
                Ok(s.to_string())
            }
        })
        .transpose()?;
    // Python v2: `if data.parent_id is not None` — null НЕ оновлює
    // (кореневою через v2 зробити неможливо — як Python).
    let parent_id = match v.get("parent_id") {
        Some(Value::String(s)) => {
            let id = Uuid::parse_str(s).map_err(|_| {
                v422(
                    "uuid_parsing",
                    &["body", "parent_id"],
                    "Input should be a valid UUID",
                    s,
                )
            })?;
            Some(Some(id))
        }
        // Python: parent_id=null або відсутній → НЕ оновлюється.
        _ => None,
    };
    // Python v2: `if data.description is not None` — null НЕ оновлює.
    let description = match v.get("description") {
        Some(Value::String(s)) => Some(Some(s.clone())),
        _ => None,
    };
    Ok(CategoryUpdateInput {
        name,
        description,
        parent_id,
    })
}

// ─── Хендлери ───────────────────────────────────────────────────────────────

/// GET /api/v2/categories?page=&size=&search=
pub async fn list(
    State(state): State<AppState>,
    Query(raw): Query<ListQuery>,
) -> Result<Json<Value>, CatV2Error> {
    let (page, size) = raw.page_size();
    let search = raw.search.as_deref();
    let repo = read_repo(&state)?;
    let page_dto = repo.search_categories(page, size, search).await?;
    Ok(Json(serde_json::json!({
        "items": page_dto.items.iter().map(cat_v2).collect::<Vec<_>>(),
        "total": page_dto.total,
        "page": page,
        "size": size,
    })))
}

/// GET /api/v2/categories/tree
pub async fn tree(State(state): State<AppState>) -> Result<Json<Value>, CatV2Error> {
    let repo = read_repo(&state)?;
    let all = repo.find_all_categories().await?;
    let nodes = build_tree(&all, None);
    Ok(Json(Value::Array(nodes)))
}

fn build_tree(all: &[CategoryDto], parent_id: Option<Uuid>) -> Vec<Value> {
    all.iter()
        .filter(|c| c.parent_id == parent_id)
        .map(|c| {
            let children = build_tree(all, Some(c.id));
            tree_node(c, children)
        })
        .collect()
}

/// GET /api/v2/categories/{id}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, CatV2Error> {
    let id = path_uuid(id, "category_id")?;
    let repo = read_repo(&state)?;
    let cat = repo.get_category(id).await.map_err(|_| {
        CatV2Error::Service(WriteError::NotFound(format!(
            "Категорію з ID '{id}' не знайдено"
        )))
    })?;
    Ok(Json(cat_v2(&cat)))
}

/// POST /api/v2/categories → 201
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), CatV2Error> {
    let input = parse_create(&body)?;
    let repo = write_repo(&state)?;
    // Python v2: exists_by_name → 400.
    if repo.category_name_exists(&input.name, None).await? {
        return Err(CatV2Error::Service(WriteError::BadRequest(format!(
            "Категорія з назвою '{}' вже існує",
            input.name
        ))));
    }
    // parent → 404 (робить create_category).
    let saved = repo.create_category(&input).await?;
    Ok((StatusCode::CREATED, Json(cat_v2(&saved))))
}

/// PUT /api/v2/categories/{id} → 200
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, CatV2Error> {
    let id = path_uuid(id, "category_id")?;
    let input = parse_update(&body)?;
    let repo = write_repo(&state)?;
    let read = read_repo(&state)?;
    // Python v2: 404 якщо не знайдено.
    let current = read.get_category(id).await.map_err(|_| {
        CatV2Error::Service(WriteError::NotFound(format!(
            "Категорію з ID '{id}' не знайдено"
        )))
    })?;
    // Python v2: exists_by_name з exclude_id → 400.
    if let Some(name) = &input.name {
        if name != &current.name && repo.category_name_exists(name, Some(id)).await? {
            return Err(CatV2Error::Service(WriteError::BadRequest(format!(
                "Категорія з назвою '{name}' вже існує"
            ))));
        }
    }
    // self-parent → 400, parent → 404 (робить update_category).
    let saved = repo.update_category(id, &input).await?;
    Ok(Json(cat_v2(&saved)))
}

/// DELETE /api/v2/categories/{id} → 204
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, CatV2Error> {
    let id = path_uuid(id, "category_id")?;
    let repo = write_repo(&state)?;
    repo.delete_category(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Query-параметри (як Python v2: page ge=1, size 1..=1000) ──────────────

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
    pub search: Option<String>,
}

impl ListQuery {
    fn page_size(&self) -> (i64, i64) {
        let page = self.page.unwrap_or(1).max(1);
        let size = self.size.unwrap_or(50).clamp(1, 1000);
        (page, size)
    }
}
