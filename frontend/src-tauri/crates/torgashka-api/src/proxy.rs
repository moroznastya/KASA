// ─────────────────────────────────────────────────────────────────────────────
// legacy — LEGACY-роути → 410 Gone (дезактивація Python sidecar, етап 8)
// ─────────────────────────────────────────────────────────────────────────────
// Після повної дезактивації Python sidecar (0 CRIT, 0 ALIAS) fallback
// більше НЕ проксіює на 127.0.0.1:8001. Усе, що не змонтовано в Rust-роутері
// (LEGACY-роути, які фронтенд не кличе), повертає 410 Gone — сумісність
// старих клієнтів збережена через явну відповідь, а не тихий 502/503.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// Fallback-хендлер: усе невідоме → 410 Gone (Python sidecar дезактивовано).
pub async fn proxy_handler(req: Request) -> Response {
    eprintln!(
        "[torgashka-api] 410 Gone: {} {} (LEGACY — Python sidecar дезактивовано)",
        req.method(),
        req.uri().path()
    );
    (
        StatusCode::GONE,
        Json(serde_json::json!({
            "detail": "endpoint_deprecated",
            "message": "Цей ендпоінт більше не підтримується (Python sidecar дезактивовано)."
        })),
    )
        .into_response()
}

// ── Тести ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;

    #[tokio::test]
    async fn unknown_route_returns_410() {
        let req = HttpRequest::builder()
            .uri("http://127.0.0.1:8000/api/v1/ledger/entries")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = proxy_handler(req).await;
        assert_eq!(resp.status(), StatusCode::GONE);
    }

    #[tokio::test]
    async fn legacy_route_returns_json_detail() {
        let req = HttpRequest::builder()
            .uri("http://127.0.0.1:8000/health")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = proxy_handler(req).await;
        assert_eq!(resp.status(), StatusCode::GONE);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["detail"], "endpoint_deprecated");
    }
}
