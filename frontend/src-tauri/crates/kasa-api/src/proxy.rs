// ─────────────────────────────────────────────────────────────────────────────
// proxy — reverse proxy на Python sidecar (127.0.0.1:8001)
// ─────────────────────────────────────────────────────────────────────────────
// Проксіює запит із тим самим path/method/body/headers. Таймаут 30s.
// Python недоступний → 503 {"detail":"python_sidecar_unavailable"} (НЕ тихий фейл).
// ─────────────────────────────────────────────────────────────────────────────

use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, StatusCode},
    response::Response,
};

use crate::{AppState, PYTHON_SIDECAR_PORT};

/// Таймаут запиту до Python sidecar.
pub const PROXY_TIMEOUT: Duration = Duration::from_secs(30);

/// Заголовки, які не копіюються при проксіюванні (їх обчислює reqwest/axum).
const HOP_BY_HOP: [&str; 4] = ["host", "content-length", "transfer-encoding", "connection"];

/// Fallback-хендлер: усе, що не health, — у Python sidecar.
pub async fn proxy_handler(
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let target = format!("http://127.0.0.1:{PYTHON_SIDECAR_PORT}{path}{query}");

    let method = req.method().clone();
    let headers = req_headers(&req);
    let body = to_bytes(req.into_body(), usize::MAX).await.map_err(|e| {
        eprintln!("[kasa-api] помилка читання тіла запиту: {e}");
        StatusCode::BAD_REQUEST
    })?;

    let mut builder = state.http_client.request(method, &target);
    for (name, value) in headers {
        builder = builder.header(name, value);
    }

    match builder.body(body).send().await {
        Ok(resp) => {
            let status = resp.status();
            let resp_headers = resp.headers().clone();
            let resp_body = resp.bytes().await.map_err(|e| {
                eprintln!("[kasa-api] помилка читання відповіді sidecar: {e}");
                StatusCode::BAD_GATEWAY
            })?;
            let mut out = Response::builder().status(status);
            for (name, value) in resp_headers.iter() {
                if !HOP_BY_HOP.contains(&name.as_str()) {
                    out = out.header(name, value);
                }
            }
            out.body(Body::from(resp_body)).map_err(|e| {
                eprintln!("[kasa-api] помилка збірки відповіді: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
        }
        Err(e) => {
            // Python sidecar недоступний — явний 503, не тихий фейл.
            eprintln!("[kasa-api] Python sidecar недоступний: {e}");
            Ok(Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"detail":"python_sidecar_unavailable"}"#))
                .expect("статичне тіло 503 завжди валідне"))
        }
    }
}

/// Заголовки запиту, які треба проксіювати (без hop-by-hop).
fn req_headers(req: &Request) -> Vec<(String, String)> {
    req.headers()
        .iter()
        .filter(|(name, _)| !HOP_BY_HOP.contains(&name.as_str()))
        .map(|(name, value)| {
            let v = value.to_str().unwrap_or_default().to_string();
            (name.as_str().to_string(), v)
        })
        .collect()
}

// ── Тести ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hop_by_hop_headers_filtered() {
        let req = Request::builder()
            .uri("http://127.0.0.1:8000/api/v1/x")
            .header("host", "127.0.0.1:8000")
            .header("content-length", "10")
            .header("x-custom", "keep-me")
            .body(Body::empty())
            .unwrap();
        let headers = req_headers(&req);
        assert!(headers
            .iter()
            .all(|(n, _)| n != "host" && n != "content-length"));
        assert!(headers
            .iter()
            .any(|(n, v)| n == "x-custom" && v == "keep-me"));
    }

    #[test]
    fn invalid_header_value_skipped_not_panicked() {
        let req = Request::builder()
            .uri("http://127.0.0.1:8000/api/v1/x")
            .header("x-bad", b"\xff\xfe".as_slice())
            .body(Body::empty())
            .unwrap();
        let headers = req_headers(&req);
        // Невалідний UTF-8 → порожній рядок, без паніки.
        assert!(headers.iter().any(|(n, v)| n == "x-bad" && v.is_empty()));
    }

    #[test]
    fn target_url_builds_with_query() {
        // Перевіряємо формат target: порт і query підставляються коректно.
        let path = "/api/v1/products".to_string();
        let query = Some("limit=10".to_string());
        let target = format!(
            "http://127.0.0.1:{PYTHON_SIDECAR_PORT}{path}{}",
            query.map(|q| format!("?{q}")).unwrap_or_default()
        );
        assert_eq!(target, "http://127.0.0.1:8001/api/v1/products?limit=10");
    }
}
