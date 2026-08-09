// ─────────────────────────────────────────────────────────────────────────────
// documents — Rust-гілка документів (етап 8, група 2)
// ─────────────────────────────────────────────────────────────────────────────
// 1:1 з Python v1/documents.py:
//   GET    /api/v1/documents                    — об'єднаний список 6 типів
//   POST   /api/v1/documents/batch-confirm      — пакетне підтвердження
//   DELETE /api/v1/documents/{id}?document_type= — видалення чернетки (204)
//   POST   /api/v1/documents/{id}/copy?document_type= — копіювання
//   GET    /api/v1/documents/export             — Excel/CSV (flat+detailed)
//   GET    /api/v1/documents/{id}/print?document_type= — дані для друку
// Авторизація: list/export — будь-яка JWT-роль (middleware); batch/delete/copy
//   — require_admin (як Python); print — optional (Bearer або ?token=, шлях
//   публічний для middleware — auth у хендлері, 1:1 get_current_user_optional).
// Монтуються лише під TORGASHKA_RUST_DOCUMENTS=1; інакше — fallback → 410 (дезактивація).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use torgashka_application::DocumentsServiceFacade;
use torgashka_domain::{BatchConfirmInput, DocListQuery, DocumentsError, DocumentsService, ExportQuery};

use crate::{auth::Claims, auth_routes::AuthRouteError, AppState};

/// Помилки хендлерів документів → HTTP (1:1 з Python).
#[derive(Debug)]
pub enum DocErr {
    Service(DocumentsError),
    /// 422 Pydantic-валідація.
    Validation(Value),
    /// 401/403 — auth (print optional та require_admin).
    Auth(AuthRouteError),
    /// 403 — Rust-гілка вимкнена.
    Forbidden(String),
}

impl From<DocumentsError> for DocErr {
    fn from(e: DocumentsError) -> Self {
        DocErr::Service(e)
    }
}

fn v422(vtype: &str, loc: &[&str], msg: &str, input: &str) -> DocErr {
    DocErr::Validation(json!({
        "detail": [{
            "type": vtype,
            "loc": loc,
            "msg": msg,
            "input": input,
        }]
    }))
}

impl IntoResponse for DocErr {
    fn into_response(self) -> Response {
        match self {
            DocErr::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            DocErr::Service(DocumentsError::NotFound(msg)) => {
                (StatusCode::NOT_FOUND, Json(json!({"detail": msg}))).into_response()
            }
            DocErr::Service(DocumentsError::BadRequest(msg)) => {
                (StatusCode::BAD_REQUEST, Json(json!({"detail": msg}))).into_response()
            }
            DocErr::Service(DocumentsError::Infrastructure(msg)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"detail": msg})),
            )
                .into_response(),
            DocErr::Auth(e) => e.into_response(),
            DocErr::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(json!({"detail": msg}))).into_response()
            }
        }
    }
}

/// require_admin документів (1:1 Python AuthService.require_admin), незалежний
/// від TORGASHKA_RUST_AUTH: перевіряє роль через documents-пул (той самий PostgreSQL).
async fn require_admin_docs(state: &AppState, claims: &Claims) -> Result<Uuid, DocErr> {
    let pool = state
        .documents_pool
        .clone()
        .ok_or_else(|| DocErr::Forbidden("Rust-гілка документів вимкнена".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        DocErr::Auth(AuthRouteError::Plain(torgashka_domain::AuthError::Unauthorized(
            "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
        )))
    })?;
    let row = sqlx::query("SELECT role::text, is_active FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| DocErr::Service(DocumentsError::Infrastructure(e.to_string())))?;
    let Some(row) = row else {
        return Err(DocErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Unauthorized("Користувача не знайдено".to_string()),
        )));
    };
    let is_active: bool = row.get("is_active");
    if !is_active {
        return Err(DocErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Forbidden("Користувач деактивований".to_string()),
        )));
    }
    let role: String = row.get("role");
    if role != "admin" {
        return Err(DocErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Forbidden(
                "Доступ заборонено: потрібна роль адміністратора".to_string(),
            ),
        )));
    }
    Ok(user_id)
}

fn doc_repo(
    state: &AppState,
) -> Result<std::sync::Arc<dyn DocumentsService + Send + Sync>, DocErr> {
    state
        .documents
        .clone()
        .ok_or_else(|| DocErr::Forbidden("Rust-гілка документів вимкнена".to_string()))
}

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, DocErr> {
    Uuid::parse_str(&raw).map_err(|_| {
        v422(
            "uuid_parsing",
            &["path", field],
            "Input should be a valid UUID",
            &raw,
        )
    })
}

/// Python `_parse_iso_date`: fromisoformat; невалідне → None (фільтр ігнорується).
fn parse_iso_date(s: Option<String>) -> Option<chrono::NaiveDateTime> {
    s.as_deref()
        .filter(|v| !v.is_empty())
        .and_then(|v| chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S%.f").ok())
        .or_else(|| {
            s.as_deref().filter(|v| !v.is_empty()).and_then(|v| {
                chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
                    .ok()
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap())
            })
        })
}

/// Python `_parse_ids`: список UUID через кому; невалідні ігноруються.
fn parse_ids(s: Option<String>) -> Vec<Uuid> {
    let mut out = Vec::new();
    if let Some(v) = s {
        for part in v.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Ok(u) = Uuid::parse_str(part) {
                out.push(u);
            }
        }
    }
    out
}

// ─── Query-параметри ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
    pub status: Option<String>,
    pub document_type: Option<String>,
    pub search: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub supplier_id: Option<String>,
    pub amount_from: Option<f64>,
    pub amount_to: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct TypeQuery {
    pub document_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExportQueryParams {
    pub ids: Option<String>,
    pub status: Option<String>,
    pub document_type: Option<String>,
    pub search: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub supplier_id: Option<String>,
    pub amount_from: Option<f64>,
    pub amount_to: Option<f64>,
    pub format: Option<String>,
    pub detailed: Option<bool>,
}

/// Python csv.writer(delimiter=';', quoting=QUOTE_MINIMAL) + utf-8-sig BOM.
fn csv_bytes(headers: &[String], rows: &[Vec<String>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]); // BOM
    let mut write_row = |cells: &[String]| {
        let line: Vec<String> = cells
            .iter()
            .map(|c| {
                if c.contains(';') || c.contains('"') || c.contains('\n') || c.contains('\r') {
                    format!("\"{}\"", c.replace('"', "\"\""))
                } else {
                    c.clone()
                }
            })
            .collect();
        out.extend_from_slice(line.join(";").as_bytes());
        out.extend_from_slice(b"\r\n"); // Python csv.writer → \r\n
    };
    write_row(headers);
    for r in rows {
        write_row(r);
    }
    out
}

/// XLSX (вміст/структура еквівалентні Python openpyxl; байти можуть відрізнятись —
/// різні бібліотеки — дозволено контрактом міграції).
fn xlsx_bytes(headers: &[String], rows: &[Vec<String>]) -> Result<Vec<u8>, DocErr> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Документи")
        .map_err(|e| DocErr::Service(DocumentsError::Infrastructure(e.to_string())))?;
    let fmt_header = rust_xlsxwriter::Format::new()
        .set_bold()
        .set_font_name("Arial")
        .set_font_size(11)
        .set_font_color(rust_xlsxwriter::Color::White)
        .set_background_color(rust_xlsxwriter::Color::RGB(0x4472C4))
        .set_align(rust_xlsxwriter::FormatAlign::Center)
        .set_align(rust_xlsxwriter::FormatAlign::VerticalCenter)
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let fmt_cell = rust_xlsxwriter::Format::new()
        .set_align(rust_xlsxwriter::FormatAlign::Left)
        .set_align(rust_xlsxwriter::FormatAlign::VerticalCenter)
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    for (idx, h) in headers.iter().enumerate() {
        ws.write_string_with_format(0, idx as u16, h, &fmt_header)
            .map_err(|e| DocErr::Service(DocumentsError::Infrastructure(e.to_string())))?;
    }
    for (r_idx, row) in rows.iter().enumerate() {
        for (c_idx, val) in row.iter().enumerate() {
            ws.write_string_with_format(r_idx as u32 + 1, c_idx as u16, val, &fmt_cell)
                .map_err(|e| DocErr::Service(DocumentsError::Infrastructure(e.to_string())))?;
        }
    }
    // Автоширина (Python: min(max_len + 4, 50))
    for (idx, h) in headers.iter().enumerate() {
        let mut max_len = h.chars().count();
        for row in rows {
            if let Some(v) = row.get(idx) {
                max_len = max_len.max(v.chars().count());
            }
        }
        ws.set_column_width(idx as u16, (max_len + 4).min(50) as f64)
            .map_err(|e| DocErr::Service(DocumentsError::Infrastructure(e.to_string())))?;
    }
    let data = wb
        .save_to_buffer()
        .map_err(|e| DocErr::Service(DocumentsError::Infrastructure(e.to_string())))?;
    Ok(data)
}

// ─── Хендлери ───────────────────────────────────────────────────────────────

/// GET /api/v1/documents — список (Python list_documents).
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<torgashka_domain::DocListDto>, DocErr> {
    let page = q.page.unwrap_or(1);
    if page < 1 {
        return Err(v422(
            "greater_than_equal",
            &["query", "page"],
            "Input should be greater than or equal to 1",
            &page.to_string(),
        ));
    }
    let size = q.size.unwrap_or(20);
    if size < 1 {
        return Err(v422(
            "greater_than_equal",
            &["query", "size"],
            "Input should be greater than or equal to 1",
            &size.to_string(),
        ));
    }
    if size > 100 {
        return Err(v422(
            "less_than_equal",
            &["query", "size"],
            "Input should be less than or equal to 100",
            &size.to_string(),
        ));
    }
    let supplier_id = match q.supplier_id.as_deref() {
        Some(s) if !s.is_empty() => Some(Uuid::parse_str(s).map_err(|_| {
            v422(
                "uuid_parsing",
                &["query", "supplier_id"],
                "Input should be a valid UUID",
                s,
            )
        })?),
        _ => None,
    };
    let dt_from = parse_iso_date(q.date_from.clone());
    let dt_to = parse_iso_date(q.date_to.clone()).map(|mut d| {
        // Python: dt_to = dt_to.replace(hour=23, minute=59, second=59, microsecond=999999)
        d = d.date().and_hms_milli_opt(23, 59, 59, 999).unwrap_or(d);
        d
    });
    let repo = doc_repo(&state)?;
    let svc = DocumentsServiceFacade::new(repo);
    Ok(Json(
        svc.list(&DocListQuery {
            page,
            size,
            status: q.status,
            document_type: q.document_type,
            search: q.search,
            date_from: dt_from,
            date_to: dt_to,
            supplier_id,
            amount_from: q.amount_from,
            amount_to: q.amount_to,
        })
        .await?,
    ))
}

/// POST /api/v1/documents/batch-confirm — require_admin.
pub async fn batch_confirm(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<torgashka_domain::BatchConfirmResultDto>, DocErr> {
    let user_id = require_admin_docs(&state, &claims).await?;
    let document_type = body
        .get("document_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| v422("missing", &["body", "document_type"], "Field required", ""))?
        .to_string();
    let ids: Vec<String> = body
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .ok_or_else(|| v422("missing", &["body", "ids"], "Field required", ""))?;
    let repo = doc_repo(&state)?;
    let svc = DocumentsServiceFacade::new(repo);
    Ok(Json(
        svc.batch_confirm(&BatchConfirmInput { document_type, ids }, user_id)
            .await?,
    ))
}

/// DELETE /api/v1/documents/{id}?document_type= → 204 (require_admin).
pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(tq): Query<TypeQuery>,
) -> Result<StatusCode, DocErr> {
    require_admin_docs(&state, &claims).await?;
    let Some(dt) = tq.document_type else {
        // Python: document_type: str = Query(...) — обов'язковий → 422.
        return Err(DocErr::Validation(json!({"detail": [{
            "type": "missing",
            "loc": ["query", "document_type"],
            "msg": "Field required",
            "input": null,
        }]})));
    };
    let id = path_uuid(id, "document_id")?;
    let repo = doc_repo(&state)?;
    let svc = DocumentsServiceFacade::new(repo);
    svc.delete(id, &dt).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/documents/{id}/copy?document_type= (require_admin).
pub async fn copy(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(tq): Query<TypeQuery>,
) -> Result<Json<Value>, DocErr> {
    let user_id = require_admin_docs(&state, &claims).await?;
    let id = path_uuid(id, "document_id")?;
    let repo = doc_repo(&state)?;
    let svc = DocumentsServiceFacade::new(repo);
    let Some(dt) = tq.document_type else {
        return Err(DocErr::Validation(json!({"detail": [{
            "type": "missing",
            "loc": ["query", "document_type"],
            "msg": "Field required",
            "input": null,
        }]})));
    };
    Ok(Json(svc.copy(id, &dt, user_id).await?))
}

/// GET /api/v1/documents/export — Excel/CSV (Python export_documents).
pub async fn export(
    State(state): State<AppState>,
    Query(q): Query<ExportQueryParams>,
) -> Result<Response, DocErr> {
    let dt_from = parse_iso_date(q.date_from.clone());
    let dt_to = parse_iso_date(q.date_to.clone()).map(|mut d| {
        d = d.date().and_hms_milli_opt(23, 59, 59, 999).unwrap_or(d);
        d
    });
    let supplier_id = match q.supplier_id.as_deref() {
        Some(s) if !s.is_empty() => Some(Uuid::parse_str(s).map_err(|_| {
            v422(
                "uuid_parsing",
                &["query", "supplier_id"],
                "Input should be a valid UUID",
                s,
            )
        })?),
        _ => None,
    };
    let repo = doc_repo(&state)?;
    let svc = DocumentsServiceFacade::new(repo);
    let data = svc
        .export(&ExportQuery {
            ids: parse_ids(q.ids),
            status: q.status,
            document_type: q.document_type,
            search: q.search,
            date_from: dt_from,
            date_to: dt_to,
            supplier_id,
            amount_from: q.amount_from,
            amount_to: q.amount_to,
            detailed: q.detailed.unwrap_or(false),
        })
        .await?;

    let now = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let is_csv = q.format.as_deref() == Some("csv");
    if is_csv {
        let body = csv_bytes(&data.headers, &data.rows);
        let filename = format!("documents_{now}.csv");
        Ok((
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    &format!("attachment; filename={filename}"),
                ),
            ],
            body,
        )
            .into_response())
    } else {
        let body = xlsx_bytes(&data.headers, &data.rows)?;
        let filename = format!("documents_{now}.xlsx");
        Ok((
            StatusCode::OK,
            [
                (
                    header::CONTENT_TYPE,
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                ),
                (
                    header::CONTENT_DISPOSITION,
                    &format!("attachment; filename={filename}"),
                ),
            ],
            body,
        )
            .into_response())
    }
}

/// GET /api/v1/documents/{id}/print?document_type= — optional auth
/// (Bearer або ?token=, 1:1 Python get_current_user_optional).
pub async fn print(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(tq): Query<TypeQuery>,
    Query(token): Query<PrintTokenQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Json<torgashka_domain::DocPrintDto>, DocErr> {
    // Python get_current_user_optional: Bearer header → ?token= → 401.
    let token_str = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| token.token.clone());
    let token_str = token_str.ok_or_else(|| {
        DocErr::Auth(AuthRouteError::Plain(torgashka_domain::AuthError::Unauthorized(
            "Необхідна авторизація. Передайте токен через заголовок Authorization: Bearer <token> або через query-параметр ?token=<token>".to_string(),
        )))
    })?;
    let claims = crate::auth::validate_jwt(&token_str, &state.jwt_secret).map_err(|_| {
        DocErr::Auth(AuthRouteError::Plain(torgashka_domain::AuthError::Unauthorized(
            "Недійсний або прострочений токен".to_string(),
        )))
    })?;
    if claims.sub.is_empty() {
        return Err(DocErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Unauthorized(
                "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
            ),
        )));
    }
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        DocErr::Auth(AuthRouteError::Plain(torgashka_domain::AuthError::Unauthorized(
            "Недійсний токен: відсутній ідентифікатор користувача".to_string(),
        )))
    })?;
    // Python: шукає користувача, 401 якщо не знайдено, 403 якщо деактивований.
    let repo = crate::auth_routes::auth_repo(&state).map_err(DocErr::Auth)?;
    let user = repo
        .get_user_by_id(user_id)
        .await
        .map_err(|e| DocErr::Auth(e.into()))?;
    if !user.is_active {
        return Err(DocErr::Auth(AuthRouteError::Plain(
            torgashka_domain::AuthError::Forbidden("Користувач деактивований".to_string()),
        )));
    }
    let Some(dt) = tq.document_type else {
        // Python: document_type: str = Query(...) — обов'язковий → 422.
        return Err(DocErr::Validation(json!({"detail": [{
            "type": "missing",
            "loc": ["query", "document_type"],
            "msg": "Field required",
            "input": null,
        }]})));
    };
    let id = path_uuid(id, "document_id")?;
    let doc_repo = doc_repo(&state)?;
    let svc = DocumentsServiceFacade::new(doc_repo);
    Ok(Json(svc.print(id, &dt).await?))
}

#[derive(Debug, Deserialize)]
pub struct PrintTokenQuery {
    pub token: Option<String>,
}
