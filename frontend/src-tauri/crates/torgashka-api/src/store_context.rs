// ─────────────────────────────────────────────────────────────────────────────
// store_context — StoreContext middleware (Етап 3 мультиточковості)
// ─────────────────────────────────────────────────────────────────────────────
// Валідація `X-Store-Id` на бізнес-ендпоінтах:
//   - запит БЕЗ X-Store-Id         → 400 {"detail": "..."}
//   - не-UUID X-Store-Id           → 400
//   - точка, до якої немає доступу → 403 (перевірка через user_stores)
//   - публічні/auth/stores-шляхи   → без X-Store-Id (управління точками)
//
// Додатково: проставляє task-local [`StoreCtx`] і ВІДКРИВАЄ КОНТЕКСТ ЗАПИТУ
// ([`StoreRequest`]) — одне з'єднання з `app.user_id`/`app.store_id` (RLS-контур
// 0004_rls) на весь HTTP-запит. Раніше кожен одиночний запит репозиторію коштував
// 3 мережеві круги (`set_config` → запит → `reset`) — тепер 1 круг на запит
// плюс один круг на скидання. Працює ПІСЛЯ auth_middleware (Claims у extensions).
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::State,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use torgashka_infrastructure::store_ctx::{with_store_ctx, StoreCtx, StorePool, StoreRequest};

use crate::auth::{Claims, DeviceCtx};
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

/// Відкриває КОНТЕКСТ ЗАПИТУ (А): ОДИН мережевий круг на все — `set_config`
/// RLS-параметрів (`app.user_id`/`app.store_id`) + перевірка доступу
/// користувача до точки (`user_stores`).
///
/// Повертає `(контекст, доступ дозволено)`. Пул не сконфігуровано (режим
/// проксі) → `Ok(None)`: працюємо як раніше (кожен statement сам собі контекст).
async fn open_request_ctx(
    pool: &Option<StorePool>,
    ctx: &StoreCtx,
) -> Result<Option<(StoreRequest, bool)>, Response> {
    let Some(pool) = pool else {
        return Ok(None);
    };
    match StoreRequest::open(&pool.0, ctx).await {
        Ok((req_tx, allowed)) => Ok(Some((req_tx, allowed))),
        Err(e) => {
            eprintln!("[torgashka-api] store_middleware: відкриття контексту запиту: {e}");
            Err(json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "База даних недоступна",
            ))
        }
    }
}

/// Те саме без перевірки доступу (шляхи управління точками: X-Store-Id опційний).
async fn open_request_ctx_unchecked(
    pool: &Option<StorePool>,
    ctx: &StoreCtx,
) -> Result<Option<StoreRequest>, Response> {
    let Some(pool) = pool else {
        return Ok(None);
    };
    match StoreRequest::open_unchecked(&pool.0, ctx).await {
        Ok(req_tx) => Ok(Some(req_tx)),
        Err(e) => {
            eprintln!("[torgashka-api] store_middleware: відкриття контексту запиту: {e}");
            Err(json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "База даних недоступна",
            ))
        }
    }
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
    // Device-каса (Частина 4): Claims(role="device") поставлені
    // auth_middleware за Bearer device_token на sync-шляхах. X-Store-Id
    // ІГНОРУЄТЬСЯ — точка береться з DeviceCtx (захист від підміни точки
    // касою). user_stores не перевіряємо: пристрій — не користувач, його
    // немає в user_stores (перевірка давала б 403).
    if claims.role == "device" {
        let dctx = match req.extensions().get::<DeviceCtx>().cloned() {
            Some(c) => c,
            None => return json_error(StatusCode::UNAUTHORIZED, "Відсутній контекст пристрою"),
        };
        let ctx = StoreCtx {
            user_id, // == dctx.device_id (claims.sub = device_id)
            store_id: dctx.store_id,
            role: "device".to_string(),
        };
        let pool = state.store_pool.clone();
        let req_tx = match open_request_ctx_unchecked(&state.store_pool, &ctx).await {
            Ok(v) => v,
            Err(e) => return e,
        };
        let inner = async move {
            // Живість каси: last_seen_at=now() на кожен запит. devices БЕЗ RLS
            // (політик немає) — оновлення безпечне в будь-якому контексті.
            //
            // ADR-0008 (E7): окремої «standby-гілки з локальним журналом»
            // більше немає — вузол завжди пише у ВЛАСНУ read-write БД.
            if let Some(sp) = pool {
                if let Err(e) = sqlx::query("UPDATE devices SET last_seen_at = now() WHERE id = $1")
                    .bind(user_id)
                    .execute(&sp)
                    .await
                {
                    eprintln!("[torgashka-api] store_middleware: оновлення last_seen_at: {e}");
                }
            }
            next.run(req).await
        };
        let resp = match &req_tx {
            Some(req_tx) => with_store_ctx(ctx.clone(), req_tx.scope(inner)).await,
            None => with_store_ctx(ctx.clone(), inner).await,
        };
        if let Some(req_tx) = req_tx {
            if let Err(e) = req_tx.finish().await {
                eprintln!("[torgashka-api] store_middleware: скидання контексту запиту: {e}");
            }
        }
        return resp;
    }
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
        let req_tx = match open_request_ctx_unchecked(&state.store_pool, &ctx).await {
            Ok(v) => v,
            Err(e) => return e,
        };
        let inner = async { next.run(req).await };
        let resp = match &req_tx {
            Some(req_tx) => with_store_ctx(ctx.clone(), req_tx.scope(inner)).await,
            None => with_store_ctx(ctx.clone(), inner).await,
        };
        if let Some(req_tx) = req_tx {
            if let Err(e) = req_tx.finish().await {
                eprintln!("[torgashka-api] store_middleware: скидання контексту запиту: {e}");
            }
        }
        return resp;
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
    if state.store_pool.is_none() {
        eprintln!("[torgashka-api] store_middleware: store_pool не ініціалізовано");
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Внутрішня помилка сервера",
        );
    }
    // RLS-контекст + перевірка доступу — ОДИН круг (контекст запиту).
    let req_tx = match open_request_ctx(&state.store_pool, &ctx).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let allowed = req_tx.as_ref().map(|(_, allowed)| *allowed).unwrap_or(true);
    if !allowed {
        if let Some((req_tx, _)) = req_tx {
            let _ = req_tx.finish().await;
        }
        return json_error(
            StatusCode::FORBIDDEN,
            "Доступ до торговельної точки заборонено",
        );
    }
    // Виконання хендлера — у тому самому контексті запиту (той самий таск).
    let inner = async move { next.run(req).await };
    let resp = match &req_tx {
        Some((req_tx, _)) => with_store_ctx(ctx.clone(), req_tx.scope(inner)).await,
        None => with_store_ctx(ctx.clone(), inner).await,
    };
    if let Some((req_tx, _)) = req_tx {
        if let Err(e) = req_tx.finish().await {
            eprintln!("[torgashka-api] store_middleware: скидання контексту запиту: {e}");
        }
    }
    resp
}
