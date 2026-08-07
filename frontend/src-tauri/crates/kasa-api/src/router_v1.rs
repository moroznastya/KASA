// ─────────────────────────────────────────────────────────────────────────────
// router_v1 — маршрути /api/v1 (Strangler Fig, етап 0)
// ─────────────────────────────────────────────────────────────────────────────
// Зараз:
//   GET /api/v1/health  → нативний Rust-хендлер (200 {"status":"ok"})
//   все інше           → reverse proxy на Python sidecar :8001
// JWT-валідація — на весь роутер (middleware), /health пропускається всередині.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{middleware, routing::get, Router};

use crate::{auth, proxy, AppState};

/// Збирає роутер v1 зі станом.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        // Усе, що не health, — у Python sidecar (метод/шлях/тіло/заголовки).
        .fallback(proxy::proxy_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state)
}

/// GET /api/v1/health → 200 {"status":"ok"} (без JWT — відкритий).
pub async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(crate::health_payload())
}
