// ─────────────────────────────────────────────────────────────────────────────
// auth_routes — Rust-гілка auth/users/settings/RBAC (етап 6)
// ─────────────────────────────────────────────────────────────────────────────
// 1:1 з Python v1 (backend/app/api/v1/users.py + settings.py + AuthService):
//   POST /api/v1/auth/login|login-pin|refresh|logout
//   GET  /api/v1/auth/verify|users-list
//   GET  /api/v1/auth/users/me
//   GET/POST /api/v1/users, GET/PUT/DELETE /api/v1/users/{id},
//   PUT /api/v1/users/{id}/permissions|hourly-rate,
//   GET /api/v1/users/permissions/list
//   GET/PUT /api/v1/settings, GET /api/v1/settings/{module},
//   PUT /api/v1/settings/{key}
//
// JWT: access/refresh генеруються тут (той самий секрет і формат, що Python);
// refresh-ендпойнт декодує refresh-токен (без permissions у claims).
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

use kasa_application::services::auth::AuthServiceFacade;
use kasa_domain::{
    permission_label, validate_and_normalize_setting_value, AuthError, AuthService,
    LoginPinRequest, LoginRequest, PermissionsUpdateInput, SettingDto, SettingUpdateInput,
    SettingsBatchInput, UserCreateInput, UserDto, UserRole, UserUpdateInput, ALL_PERMISSIONS,
    PERMISSION_GROUPS,
};
use kasa_infrastructure::repositories::auth::SqlxAuth;

use crate::{
    auth::{create_access_token, create_refresh_token, decode_token, Claims},
    AppState,
};

/// Спеціалізована помилка з IntoResponse (деталі 1:1 Python).
#[derive(Debug)]
pub enum AuthRouteError {
    /// 401/403/404/409/400 — {"detail": "..."}
    Plain(AuthError),
    /// 422 FastAPI-формат {"detail": [ {type, loc, msg, input, ctx} ]}
    Validation(Value),
}

impl From<AuthError> for AuthRouteError {
    fn from(e: AuthError) -> Self {
        AuthRouteError::Plain(e)
    }
}

impl IntoResponse for AuthRouteError {
    fn into_response(self) -> Response {
        match self {
            AuthRouteError::Validation(detail) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(detail)).into_response()
            }
            AuthRouteError::Plain(e) => {
                let (status, msg) = match e {
                    AuthError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
                    AuthError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
                    AuthError::NotFound(m) => (StatusCode::NOT_FOUND, m),
                    AuthError::Conflict(m) => (StatusCode::CONFLICT, m),
                    AuthError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
                    AuthError::Validation(_) => unreachable!("validation handled above"),
                    AuthError::Infrastructure(m) => {
                        eprintln!("[kasa-api] auth infrastructure error: {m}");
                        (StatusCode::INTERNAL_SERVER_ERROR, "Помилка БД".to_string())
                    }
                };
                (status, Json(json!({"detail": msg}))).into_response()
            }
        }
    }
}

// ─── 422-хелпери (FastAPI Pydantic v2 формат) ───────────────────────────────
// Pydantic збирає ВСІ помилки валідації тіла → акумулюємо в Vec, віддаємо разом.

fn v422_err(vtype: &str, loc: &[&str], msg: &str, input: Value, ctx: Option<Value>) -> Value {
    let mut detail = json!({"type": vtype, "loc": loc, "msg": msg, "input": input});
    if let Some(c) = ctx {
        detail["ctx"] = c;
    }
    detail
}

fn finish422(errs: Vec<Value>) -> Result<(), AuthRouteError> {
    if errs.is_empty() {
        Ok(())
    } else {
        Err(AuthRouteError::Validation(json!({"detail": errs})))
    }
}

fn path_uuid(raw: String, field: &'static str) -> Result<Uuid, AuthRouteError> {
    Uuid::parse_str(&raw).map_err(|_| {
        AuthRouteError::Validation(json!({"detail": [v422_err(
            "uuid_parsing",
            &["path", field],
            "Input should be a valid UUID",
            Value::String(raw),
            None,
        )]}))
    })
}

// ─── Допоміжні функції ──────────────────────────────────────────────────────

/// user_id з claims (Python `payload.get("sub")` → 401).
fn sub_uuid(claims: &Claims) -> Result<Uuid, AuthRouteError> {
    Uuid::parse_str(&claims.sub).map_err(|_| {
        AuthError::Unauthorized("Недійсний токен: відсутній ідентифікатор користувача".to_string())
            .into()
    })
}

pub(crate) fn auth_repo(
    state: &AppState,
) -> Result<Arc<dyn AuthService + Send + Sync>, AuthRouteError> {
    state.auth.clone().ok_or_else(|| {
        AuthError::Forbidden("Rust-гілка auth вимкнена (KASA_RUST_AUTH=0)".to_string()).into()
    })
}

/// require_admin (1:1 Python `Depends(AuthService.require_admin)`):
/// get_current_user (401 "Користувача не знайдено" / 403 "Користувач деактивований")
/// → role != admin → 403 "Доступ заборонено: потрібна роль адміністратора".
pub(crate) async fn require_admin(
    state: &AppState,
    claims: &Claims,
) -> Result<Uuid, AuthRouteError> {
    // 1:1 Python require_admin_role: роль береться з JWT (scope["user_role"]),
    // НЕ з БД. Це дозволяє токену з role=admin працювати незалежно від БД.
    let _ = state;
    let user_id = sub_uuid(claims)?;
    if claims.role != "admin" {
        return Err(AuthError::Forbidden(
            "Доступ заборонено: потрібна роль адміністратора".to_string(),
        )
        .into());
    }
    Ok(user_id)
}

/// Перевірка ролі admin для settings (Python: `current_user.role != "admin"` → 403).
async fn ensure_settings_admin(state: &AppState, claims: &Claims) -> Result<Uuid, AuthRouteError> {
    let repo = auth_repo(state)?;
    let user_id = sub_uuid(claims)?;
    let user = repo.get_user_by_id(user_id).await?;
    if user.role != "admin" {
        return Err(AuthError::Forbidden(
            "Тільки адміністратор може змінювати налаштування".to_string(),
        )
        .into());
    }
    Ok(user_id)
}

/// Генерує access+refresh токени (1:1 Python login: access з правами, refresh без прав).
async fn issue_tokens(
    state: &AppState,
    user: &UserDto,
) -> Result<(String, String), AuthRouteError> {
    let access = create_access_token(
        &user.id.to_string(),
        &user.role,
        &user.effective_permissions(),
        &state.jwt_secret,
    )
    .map_err(|e| AuthError::Infrastructure(format!("JWT: {e}")))?;
    let refresh = create_refresh_token(&user.id.to_string(), &user.role, &state.jwt_secret)
        .map_err(|e| AuthError::Infrastructure(format!("JWT: {e}")))?;
    Ok((access, refresh))
}

// ─── Парсинг тіл (422 1:1 Pydantic v2, всі помилки разом) ───────────────────

fn body_obj(body: &Value) -> Result<&serde_json::Map<String, Value>, AuthRouteError> {
    body.as_object().ok_or_else(|| {
        AuthRouteError::Validation(json!({"detail": [v422_err(
            "model_attributes_type",
            &["body"],
            "Input should be a valid dictionary or instance of BaseModel",
            body.clone(),
            None,
        )]}))
    })
}

fn missing_err(field: &str, body: &Value) -> Value {
    v422_err(
        "missing",
        &["body", field],
        "Field required",
        body.clone(),
        None,
    )
}

fn string_type_err(field: &str, v: &Value) -> Value {
    v422_err(
        "string_type",
        &["body", field],
        "Input should be a valid string",
        v.clone(),
        None,
    )
}

fn list_type_err(field: &str, v: &Value) -> Value {
    v422_err(
        "list_type",
        &["body", field],
        "Input should be a valid list",
        v.clone(),
        None,
    )
}

fn too_short_err(field: &str, v: &str, m: usize) -> Value {
    v422_err(
        "string_too_short",
        &["body", field],
        &format!("String should have at least {m} characters"),
        Value::String(v.to_string()),
        Some(json!({"min_length": m})),
    )
}

fn too_long_err(field: &str, v: &str, m: usize) -> Value {
    v422_err(
        "string_too_long",
        &["body", field],
        &format!("String should have at most {m} characters"),
        Value::String(v.to_string()),
        Some(json!({"max_length": m})),
    )
}

/// Валідує рядкове поле (порядок Pydantic: missing → type → min → max).
fn str_field_validated(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    body: &Value,
    min: Option<usize>,
    max: Option<usize>,
) -> (Option<String>, Vec<Value>) {
    let Some(v) = obj.get(field) else {
        return (None, vec![missing_err(field, body)]);
    };
    let Some(s) = v.as_str() else {
        return (None, vec![string_type_err(field, v)]);
    };
    let mut errs = Vec::new();
    if let Some(m) = min {
        if s.chars().count() < m {
            errs.push(too_short_err(field, s, m));
        }
    }
    if let Some(m) = max {
        if s.chars().count() > m {
            errs.push(too_long_err(field, s, m));
        }
    }
    (Some(s.to_string()), errs)
}

/// Опційне рядкове поле (тип/довжина при наявності).
fn opt_str_validated(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    min: Option<usize>,
    max: Option<usize>,
) -> (Option<String>, Vec<Value>) {
    let Some(v) = obj.get(field) else {
        return (None, vec![]);
    };
    if v.is_null() {
        return (None, vec![]);
    }
    let Some(s) = v.as_str() else {
        return (None, vec![string_type_err(field, v)]);
    };
    let mut errs = Vec::new();
    if let Some(m) = min {
        if s.chars().count() < m {
            errs.push(too_short_err(field, s, m));
        }
    }
    if let Some(m) = max {
        if s.chars().count() > m {
            errs.push(too_long_err(field, s, m));
        }
    }
    (Some(s.to_string()), errs)
}

fn parse_login(body: &Value) -> Result<LoginRequest, AuthRouteError> {
    let obj = body_obj(body)?;
    let mut errs = Vec::new();
    let (login, mut e1) = str_field_validated(obj, "login", body, None, None);
    errs.append(&mut e1);
    let (password, mut e2) = str_field_validated(obj, "password", body, None, None);
    errs.append(&mut e2);
    finish422(errs)?;
    Ok(LoginRequest {
        login: login.unwrap(),
        password: password.unwrap(),
    })
}

fn parse_login_pin(body: &Value) -> Result<LoginPinRequest, AuthRouteError> {
    let obj = body_obj(body)?;
    let mut errs = Vec::new();
    let (login, mut e1) = str_field_validated(obj, "login", body, None, None);
    errs.append(&mut e1);
    let (pin_code, mut e2) = str_field_validated(obj, "pin_code", body, None, None);
    errs.append(&mut e2);
    finish422(errs)?;
    Ok(LoginPinRequest {
        login: login.unwrap(),
        pin_code: pin_code.unwrap(),
    })
}

fn parse_role(value: &Value) -> Result<UserRole, AuthRouteError> {
    match value.as_str() {
        Some("admin") => Ok(UserRole::Admin),
        Some("cashier") => Ok(UserRole::Cashier),
        _ => Err(AuthRouteError::Validation(json!({"detail": [v422_err(
            "enum",
            &["body", "role"],
            "Input should be 'admin' or 'cashier'",
            value.clone(),
            Some(json!({"expected": "'admin' or 'cashier'"})),
        )]}))),
    }
}

fn parse_bool_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    default: bool,
) -> (bool, Vec<Value>) {
    match obj.get(field) {
        None => (default, vec![]),
        Some(v) => match v.as_bool() {
            Some(b) => (b, vec![]),
            None => (
                default,
                vec![v422_err(
                    "bool_parsing",
                    &["body", field],
                    "Input should be a valid boolean, unable to interpret input",
                    v.clone(),
                    None,
                )],
            ),
        },
    }
}

fn parse_permissions_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
) -> (Option<Vec<String>>, Vec<Value>) {
    let Some(v) = obj.get(field) else {
        return (None, vec![]);
    };
    if v.is_null() {
        return (None, vec![]);
    }
    let Value::Array(arr) = v else {
        return (None, vec![list_type_err(field, v)]);
    };
    let mut out = Vec::with_capacity(arr.len());
    let mut errs = Vec::new();
    for item in arr {
        match item.as_str() {
            Some(s) => out.push(s.to_string()),
            None => errs.push(string_type_err(field, item)),
        }
    }
    (Some(out), errs)
}

fn parse_create_user(body: &Value) -> Result<UserCreateInput, AuthRouteError> {
    let obj = body_obj(body)?;
    let mut errs = Vec::new();
    let (name, mut e) = str_field_validated(obj, "name", body, None, Some(255));
    errs.append(&mut e);
    let (login, mut e) = opt_str_validated(obj, "login", None, Some(100));
    errs.append(&mut e);
    let (password, mut e) = str_field_validated(obj, "password", body, Some(4), Some(100));
    errs.append(&mut e);
    let (pin_code, mut e) = opt_str_validated(obj, "pin_code", Some(4), Some(10));
    errs.append(&mut e);
    let role = match obj.get("role") {
        None | Some(Value::Null) => UserRole::Cashier,
        Some(v) => match parse_role(v) {
            Ok(r) => r,
            Err(AuthRouteError::Validation(d)) => {
                if let Some(arr) = d.get("detail").and_then(|x| x.as_array()) {
                    errs.extend(arr.iter().cloned());
                }
                UserRole::Cashier
            }
            Err(e) => return Err(e),
        },
    };
    let (is_active, mut e) = parse_bool_field(obj, "is_active", true);
    errs.append(&mut e);
    let (permissions, mut e) = parse_permissions_field(obj, "permissions");
    errs.append(&mut e);
    finish422(errs)?;
    Ok(UserCreateInput {
        name: name.unwrap(),
        login,
        password: password.unwrap(),
        pin_code,
        role,
        is_active,
        permissions,
    })
}

fn parse_update_user(body: &Value) -> Result<UserUpdateInput, AuthRouteError> {
    let obj = body_obj(body)?;
    let mut errs = Vec::new();
    let (name, mut e) = opt_str_validated(obj, "name", None, Some(255));
    errs.append(&mut e);
    let (login, mut e) = opt_str_validated(obj, "login", None, Some(100));
    errs.append(&mut e);
    let (password, mut e) = opt_str_validated(obj, "password", Some(4), Some(100));
    errs.append(&mut e);
    let (pin_code, mut e) = opt_str_validated(obj, "pin_code", Some(4), Some(10));
    errs.append(&mut e);
    let role = match obj.get("role") {
        None | Some(Value::Null) => None,
        Some(v) => match parse_role(v) {
            Ok(r) => Some(r),
            Err(AuthRouteError::Validation(d)) => {
                if let Some(arr) = d.get("detail").and_then(|x| x.as_array()) {
                    errs.extend(arr.iter().cloned());
                }
                None
            }
            Err(e) => return Err(e),
        },
    };
    let (is_active, mut e) = match obj.get("is_active") {
        None | Some(Value::Null) => (None, vec![]),
        Some(v) => match v.as_bool() {
            Some(b) => (Some(b), vec![]),
            None => (
                None,
                vec![v422_err(
                    "bool_parsing",
                    &["body", "is_active"],
                    "Input should be a valid boolean, unable to interpret input",
                    v.clone(),
                    None,
                )],
            ),
        },
    };
    errs.append(&mut e);
    let (permissions, mut e) = parse_permissions_field(obj, "permissions");
    errs.append(&mut e);
    finish422(errs)?;
    Ok(UserUpdateInput {
        name,
        login,
        password,
        pin_code,
        role,
        is_active,
        permissions,
    })
}

fn parse_permissions_body(body: &Value) -> Result<PermissionsUpdateInput, AuthRouteError> {
    let obj = body_obj(body)?;
    let Some(v) = obj.get("permissions") else {
        return Err(AuthRouteError::Validation(
            json!({"detail": [missing_err("permissions", body)]}),
        ));
    };
    let Value::Array(arr) = v else {
        return Err(AuthRouteError::Validation(
            json!({"detail": [list_type_err("permissions", v)]}),
        ));
    };
    let mut list = Vec::with_capacity(arr.len());
    let mut errs = Vec::new();
    for item in arr {
        match item.as_str() {
            Some(s) => list.push(s.to_string()),
            None => errs.push(string_type_err("permissions", item)),
        }
    }
    finish422(errs)?;
    Ok(PermissionsUpdateInput { permissions: list })
}

fn parse_hourly_rate(body: &Value) -> Result<f64, AuthRouteError> {
    let obj = body_obj(body)?;
    let Some(v) = obj.get("hourly_rate") else {
        return Err(AuthRouteError::Validation(
            json!({"detail": [missing_err("hourly_rate", body)]}),
        ));
    };
    let num = match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| {
            AuthRouteError::Validation(json!({"detail": [v422_err(
                "float_parsing",
                &["body", "hourly_rate"],
                "Input should be a valid number, unable to parse string as a number",
                v.clone(),
                None,
            )]}))
        })?,
        Value::String(s) => s.parse::<f64>().map_err(|_| {
            AuthRouteError::Validation(json!({"detail": [v422_err(
                "float_parsing",
                &["body", "hourly_rate"],
                "Input should be a valid number, unable to parse string as a number",
                v.clone(),
                None,
            )]}))
        })?,
        _ => {
            return Err(AuthRouteError::Validation(json!({"detail": [v422_err(
                "float_parsing",
                &["body", "hourly_rate"],
                "Input should be a valid number, unable to parse string as a number",
                v.clone(),
                None,
            )]})))
        }
    };
    if num <= 0.0 {
        return Err(AuthRouteError::Validation(json!({"detail": [v422_err(
            "greater_than",
            &["body", "hourly_rate"],
            "Input should be greater than 0",
            json!(num),
            Some(json!({"gt": 0.0})),
        )]})));
    }
    Ok(num)
}

fn parse_setting_update(body: &Value) -> Result<SettingUpdateInput, AuthRouteError> {
    let obj = body_obj(body)?;
    let value = match obj.get("value") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .ok_or_else(|| {
                    AuthRouteError::Validation(json!({"detail": [v422_err(
                        "string_type",
                        &["body", "value"],
                        "Input should be a valid string",
                        v.clone(),
                        None,
                    )]}))
                })?
                .to_string(),
        ),
    };
    Ok(SettingUpdateInput { value })
}

fn parse_settings_batch(body: &Value) -> Result<SettingsBatchInput, AuthRouteError> {
    let obj = body_obj(body)?;
    let Some(v) = obj.get("settings") else {
        return Err(AuthRouteError::Validation(
            json!({"detail": [missing_err("settings", body)]}),
        ));
    };
    let Value::Object(map) = v else {
        return Err(AuthRouteError::Validation(json!({"detail": [v422_err(
            "dict_type",
            &["body", "settings"],
            "Input should be a valid dictionary",
            v.clone(),
            None,
        )]})));
    };
    let mut pairs = Vec::with_capacity(map.len());
    let mut errs = Vec::new();
    for (k, val) in map {
        match val {
            Value::Null => pairs.push((k.clone(), None)),
            Value::String(sv) => pairs.push((k.clone(), Some(sv.clone()))),
            _ => errs.push(v422_err(
                "string_type",
                &["body", "settings"],
                "Input should be a valid string",
                val.clone(),
                None,
            )),
        }
    }
    finish422(errs)?;
    Ok(SettingsBatchInput { settings: pairs })
}

// ─── Auth хендлери ──────────────────────────────────────────────────────────

/// POST /api/v1/auth/login (публічний).
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::LoginResult>, AuthRouteError> {
    let input = parse_login(&body)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    let mut result = svc.login(&input).await?;
    let (access, refresh) = issue_tokens(&state, &result.user).await?;
    result.access_token = access;
    result.refresh_token = refresh;
    Ok(Json(result))
}

/// POST /api/v1/auth/login-pin (публічний).
pub async fn login_pin(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::LoginResult>, AuthRouteError> {
    let input = parse_login_pin(&body)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    let mut result = svc.login_pin(&input).await?;
    let (access, refresh) = issue_tokens(&state, &result.user).await?;
    result.access_token = access;
    result.refresh_token = refresh;
    Ok(Json(result))
}

/// POST /api/v1/auth/refresh (публічний).
pub async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::LoginResult>, AuthRouteError> {
    let refresh_token = body
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AuthRouteError::Plain(AuthError::BadRequest("Відсутній refresh_token".to_string()))
        })?;
    let claims = decode_token(refresh_token, &state.jwt_secret)
        .map_err(|_| AuthError::Unauthorized("Недійсний або прострочений токен".to_string()))?;
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AuthError::Unauthorized("Недійсний refresh_token".to_string()))?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    let mut result = svc.refresh(user_id).await?;
    let (access, refresh) = issue_tokens(&state, &result.user).await?;
    result.access_token = access;
    result.refresh_token = refresh;
    Ok(Json(result))
}

/// POST /api/v1/auth/logout (JWT).
pub async fn logout(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, AuthRouteError> {
    let user_id = sub_uuid(&claims)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    svc.logout(user_id).await?;
    Ok(Json(json!({"message": "Успішний вихід із системи"})))
}

/// GET /api/v1/auth/verify (публічний; токен опційний — як Python optional).
pub async fn verify(State(state): State<AppState>, request: axum::extract::Request) -> Json<Value> {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(token) = token else {
        return Json(json!({"valid": false}));
    };
    let claims = match decode_token(token, &state.jwt_secret) {
        Ok(c) => c,
        Err(_) => return Json(json!({"valid": false})),
    };
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => return Json(json!({"valid": false})),
    };
    let repo = match auth_repo(&state) {
        Ok(r) => r,
        Err(_) => return Json(json!({"valid": false})),
    };
    match repo.get_user_by_id(user_id).await {
        Ok(user) => Json(json!({"valid": true, "user_id": user.id.to_string(), "role": user.role})),
        Err(_) => Json(json!({"valid": false})),
    }
}

/// GET /api/v1/auth/users-list (публічний) — список активних для логіну.
pub async fn users_list(
    State(state): State<AppState>,
) -> Result<Json<Vec<kasa_domain::PublicUserDto>>, AuthRouteError> {
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.users_list_public().await?))
}

/// GET /api/v1/auth/users/me (JWT) — поточний користувач.
pub async fn me(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<UserDto>, AuthRouteError> {
    let user_id = sub_uuid(&claims)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.get_user_by_id(user_id).await?))
}

// ─── Users хендлери (всі require_admin) ─────────────────────────────────────

/// GET /api/v1/users?page=&size= (тільки admin).
pub async fn list_users(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<serde_json::Map<String, Value>>,
) -> Result<Json<kasa_domain::UserListDto>, AuthRouteError> {
    require_admin(&state, &claims).await?;
    // page: Query(1, ge=1); size: Query(50, ge=1, le=1000) — 422 як FastAPI.
    let page = parse_page_param(&params, "page", 1)?;
    let size = parse_page_param(&params, "size", 50)?;
    if size > 1000 {
        return Err(AuthRouteError::Validation(json!({"detail": [v422_err(
            "less_than_equal",
            &["query", "size"],
            "Input should be less than or equal to 1000",
            Value::String(params.get("size").and_then(|v| v.as_str()).unwrap_or("").to_string()),
            Some(json!({"le": 1000})),
        )]})));
    }
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.list_users(page, size).await?))
}

fn parse_page_param(
    params: &serde_json::Map<String, Value>,
    name: &str,
    default: i64,
) -> Result<i64, AuthRouteError> {
    let Some(v) = params.get(name) else {
        return Ok(default);
    };
    let raw = v.as_str().unwrap_or("");
    let num: i64 = raw.parse().map_err(|_| {
        AuthRouteError::Validation(json!({"detail": [v422_err(
            "int_parsing",
            &["query", name],
            "Input should be a valid integer, unable to parse string as an integer",
            Value::String(raw.to_string()),
            None,
        )]}))
    })?;
    if num < 1 {
        return Err(AuthRouteError::Validation(json!({"detail": [v422_err(
            "greater_than_equal",
            &["query", name],
            "Input should be greater than or equal to 1",
            Value::String(raw.to_string()),
            Some(json!({"ge": 1})),
        )]})));
    }
    Ok(num)
}

/// GET /api/v1/users/{user_id} (тільки admin).
pub async fn get_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Result<Json<UserDto>, AuthRouteError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(user_id, "user_id")?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.get_user_by_id(id).await?))
}

/// POST /api/v1/users (тільки admin) → 201.
pub async fn create_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<UserDto>), AuthRouteError> {
    require_admin(&state, &claims).await?;
    let input = parse_create_user(&body)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok((StatusCode::CREATED, Json(svc.create_user(&input).await?)))
}

/// PUT /api/v1/users/{user_id} (тільки admin).
pub async fn update_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<UserDto>, AuthRouteError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(user_id, "user_id")?;
    let input = parse_update_user(&body)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.update_user(id, &input).await?))
}

/// PUT /api/v1/users/{user_id}/permissions (тільки admin).
pub async fn update_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<UserDto>, AuthRouteError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(user_id, "user_id")?;
    let input = parse_permissions_body(&body)?;
    // Валідуємо, що всі права існують (Python: 400 "Невідоме право доступу...").
    let valid: std::collections::HashSet<&str> = ALL_PERMISSIONS.iter().copied().collect();
    for perm in &input.permissions {
        if !valid.contains(perm.as_str()) {
            let available = ALL_PERMISSIONS.join(", ");
            return Err(AuthError::BadRequest(format!(
                "Невідоме право доступу: '{perm}'. Доступні права: {available}"
            ))
            .into());
        }
    }
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.update_permissions(id, &input.permissions).await?))
}

/// PUT /api/v1/users/{user_id}/hourly-rate (тільки admin).
pub async fn update_hourly_rate(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AuthRouteError> {
    require_admin(&state, &claims).await?;
    let id = path_uuid(user_id, "user_id")?;
    let hourly_rate = parse_hourly_rate(&body)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.update_hourly_rate(id, hourly_rate).await?))
}

/// DELETE /api/v1/users/{user_id} (тільки admin) → 204.
pub async fn delete_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Result<StatusCode, AuthRouteError> {
    let current_user_id = require_admin(&state, &claims).await?;
    let id = path_uuid(user_id, "user_id")?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    svc.delete_user(id, current_user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/users/permissions/list (тільки admin) — групи + всі права.
pub async fn permissions_list(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, AuthRouteError> {
    require_admin(&state, &claims).await?;
    let mut groups = Vec::with_capacity(PERMISSION_GROUPS.len());
    for (group_name, icon, perms) in PERMISSION_GROUPS {
        let permissions: Vec<Value> = perms
            .iter()
            .map(|key| {
                let label = permission_label(key);
                let label = if label == "unknown" {
                    key.split(':').nth(1).unwrap_or(key).to_string()
                } else {
                    label.to_string()
                };
                json!({"key": key, "label": label, "description": label})
            })
            .collect();
        groups.push(json!({"name": group_name, "icon": icon, "permissions": permissions}));
    }
    let all: Vec<&str> = ALL_PERMISSIONS.to_vec();
    let mut all_sorted = all.clone();
    all_sorted.sort_unstable();
    Ok(Json(
        json!({"groups": groups, "all_permissions": all_sorted}),
    ))
}

// ─── Settings хендлери ───────────────────────────────────────────────────────

/// GET /api/v1/settings (JWT) — всі налаштування за модулями.
pub async fn settings_all(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<kasa_domain::SettingsModulesDto>, AuthRouteError> {
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    // Python: Depends(get_current_user) — перевірка існування/активності.
    svc.get_user_by_id(sub_uuid(&claims)?).await?;
    Ok(Json(svc.settings_all().await?))
}

/// GET /api/v1/settings/{module} (JWT) — налаштування модуля.
pub async fn settings_by_module(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(module): Path<String>,
) -> Result<Json<Vec<SettingDto>>, AuthRouteError> {
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    svc.get_user_by_id(sub_uuid(&claims)?).await?;
    Ok(Json(svc.settings_by_module(&module).await?))
}

/// PUT /api/v1/settings (admin) — масове оновлення.
pub async fn settings_batch_update(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<kasa_domain::SettingsModulesDto>, AuthRouteError> {
    ensure_settings_admin(&state, &claims).await?;
    let input = parse_settings_batch(&body)?;
    // Валідація + нормалізація кожного значення (Python: 422 при помилці,
    // нормалізоване значення зберігається).
    let mut normalized: Vec<(String, Option<String>)> = Vec::with_capacity(input.settings.len());
    for (key, value) in &input.settings {
        let v = validate_and_normalize_setting_value(key, value.as_deref())
            .map_err(AuthError::BadRequest)?;
        normalized.push((key.clone(), v));
    }
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.settings_batch_update(&normalized).await?))
}

/// PUT /api/v1/settings/{key} (admin) — upsert з валідацією.
pub async fn settings_update_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<SettingDto>, AuthRouteError> {
    ensure_settings_admin(&state, &claims).await?;
    let input = parse_setting_update(&body)?;
    let normalized = validate_and_normalize_setting_value(&key, input.value.as_deref())
        .map_err(AuthError::BadRequest)?;
    let repo = auth_repo(&state)?;
    let svc = AuthServiceFacade::new(repo);
    Ok(Json(svc.settings_update_key(&key, normalized).await?))
}

/// Пул для бенчмарк/тестів: пряме створення репозиторію (не використовується в роутах).
#[allow(dead_code)]
fn _sqlx_auth(pool: sqlx::PgPool) -> SqlxAuth {
    SqlxAuth::new(pool)
}
