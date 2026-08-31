// ─────────────────────────────────────────────────────────────────────────────
// store_context — StoreContext middleware (Етап 3 мультиточковості)
// ─────────────────────────────────────────────────────────────────────────────
// Валідація `X-Store-Id` на бізнес-ендпоінтах:
//   - запит БЕЗ X-Store-Id         → 400 {"detail": "..."}
//   - не-UUID X-Store-Id           → 400
//   - точка, до якої немає доступу → 403 (перевірка через user_stores)
//   - публічні/auth/stores-шляхи   → без X-Store-Id (управління точками)
//
// Додатково: проставляє task-local [`StoreCtx`] — репозиторії (StorePool)
// підставляють `app.user_id`/`app.store_id` у `set_config` на кожен запит
// (RLS-контур 0004_rls). Працює ПІСЛЯ auth_middleware (Claims у extensions).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::State,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool};

use crate::auth::Claims;
use crate::AppState;

/// Шляхи, що НЕ вимагають X-Store-Id (але потребують JWT-контексту).
fn is_store_management_path(path: &str) -> bool {
    path == "/api/v1/stores" || path.starts_with("/api/v1/stores/") || path == "/api/v1/user-stores"
}

/// Публічні шляхи (JWT не обов'язковий → контекст не потрібен).
fn is_public_path(path: &str) -> bool {
    path == "/api/v1/health"
        || path.starts_with("/api/v1/setup")
        || path.starts_with("/api/v1/auth/")
        || path == "/api/v1/auth/login"
        || path == "/api/v1/auth/login-pin"
        || path == "/api/v1/auth/refresh"
        || path.starts_with("/docs")
        || path.starts_with("/redoc")
        || path.starts_with("/openapi.json")
        || path.starts_with("/uploads/")
}

fn json_error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({"detail": msg.into()}))).into_response()
}

/// Перевірка доступу користувача до точки (user_stores) у межах RLS-контексту.
async fn check_store_access(pool: &StorePool, ctx: &StoreCtx) -> bool {
    let res: Result<Option<bool>, _> = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM user_stores WHERE user_id = $1 AND store_id = $2)",
    )
    .bind(ctx.user_id)
    .bind(ctx.store_id)
    .fetch_optional(pool)
    .await;
    matches!(res, Ok(Some(true)))
}

/// Middleware StoreContext: валідація X-Store-Id + task-local контекст.
pub async fn store_middleware(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if req.method() == Method::OPTIONS || is_public_path(&path) {
        return next.run(req).await;
    }
    // Claims (JWT) — встановлені auth_middleware для непублічних шляхів.
    let claims = match req.extensions().get::<Claims>().cloned() {
        Some(c) => c,
        None => return json_error(StatusCode::UNAUTHORIZED, "Відсутній контекст авторизації"),
    };
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "Недійсний токен: відсутній ідентифікатор користувача",
            )
        }
    };
    // Управління точками: X-Store-Id опційний.
    //   - POST /stores (створення нової точки) виконується З активної точки —
    //     заголовок несе джерело для копіювання налаштувань/шаблонів у нову;
    //   - GET /stores може бути без X-Store-Id — тоді store_id = Nil UUID
    //     (контекст без точки, список точок користувача).
    if is_store_management_path(&path) {
        let store_id = req
            .headers()
            .get("x-store-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::nil);
        let ctx = StoreCtx {
            user_id,
            store_id,
            role: claims.role.clone(),
        };
        return with_store_ctx(ctx, async { next.run(req).await }).await;
    }
    // Бізнес-ендпоінти: X-Store-Id обов'язковий.
    let store_header = req.headers().get("x-store-id");
    let store_id_str = match store_header.and_then(|v| v.to_str().ok()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "Відсутній заголовок X-Store-Id: вкажіть активну торговельну точку",
            )
        }
    };
    let store_id = match Uuid::parse_str(store_id_str) {
        Ok(u) => u,
        Err(_) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                format!("Невірний X-Store-Id: '{store_id_str}' — очікується UUID"),
            )
        }
    };
    let ctx = StoreCtx {
        user_id,
        store_id,
        role: claims.role.clone(),
    };
    let Some(pool) = state.store_pool.clone() else {
        eprintln!("[torgashka-api] store_middleware: store_pool не ініціалізовано");
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Внутрішня помилка сервера",
        );
    };
    // Перевірка доступу + виконання хендлера в ОДНОМУ scope контексту:
    // і перевірка user_stores, і всі запити хендлера бачать app.user_id/store_id.
    with_store_ctx(ctx.clone(), async move {
        let allowed = check_store_access(&pool, &ctx).await;
        if !allowed {
            return json_error(
                StatusCode::FORBIDDEN,
                "Доступ до торговельної точки заборонено",
            );
        }
        next.run(req).await
    })
    .await
}
