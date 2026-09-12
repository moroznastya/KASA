//! R1 (ADR-0008 «Варіант B»): HTTP-рівень ендпоінта знімка БД хаба.
//!
//! Перевіряється РЕАЛЬНИЙ роутер фасаду (`router_v1::build_router`) через
//! `oneshot`, БЕЗ TCP і БЕЗ БД: хендлер читає файл з диска, тож для перевірки
//! авторизації та заголовків жоден пул не потрібен.
//!
//! Окремо перевіряється те, що ендпоінт НЕ вимагає store-скоупу: запити йдуть
//! БЕЗ заголовка `X-Store-Id` (знімок — уся БД, а не дані точки), і все одно
//! доходять до хендлера (200/403), а не отримують 400 «потрібен X-Store-Id».

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use torgashka_api::{auth, router_v1::build_router, AppState};

/// Секрет підпису тестових токенів (лише для цього тесту).
const SECRET: &str = "r1-snapshot-test-secret";

/// Фактичні характеристики РЕАЛЬНОГО артефакта хаба (у репо перевірено
/// `sha256sum`/`stat`): 716167 байт, PG 17.6.
const REAL_FILENAME: &str = "pos_system_fresh_20260912.dump";
const REAL_SHA256: &str = "3d2d17c2f1592dccd39d824da047cbf5c07f9e4ac50a19a7fade75616fabb25a";
const REAL_LENGTH: usize = 716167;

/// Каталог реального артефакта: <корінь репо>/artifacts/hub_snapshot.
fn artifact_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../artifacts/hub_snapshot")
}

/// Один раз на бінар: каталог знімків = реальний артефакт репо.
fn use_artifact_dir() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = artifact_dir();
        assert!(
            dir.join(REAL_FILENAME).is_file(),
            "немає реального артефакта хаба: {}",
            dir.display()
        );
        std::env::set_var(
            torgashka_api::sync_snapshot::SNAPSHOT_DIR_ENV,
            dir.as_os_str(),
        );
    });
}

/// Стан фасаду БЕЗ жодного репозиторія/пула: перевіряємо лише поверхню знімка.
fn state() -> AppState {
    AppState {
        jwt_secret: Arc::new(SECRET.to_string()),
        readdirs: None,
        write: None,
        write_pool: None,
        pos: None,
        ledger: None,
        auth: None,
        prro: None,
        debtors: None,
        documents: None,
        documents_pool: None,
        invoices_v1: None,
        invoices_v2: None,
        invoices_pool: None,
        return_invoices: None,
        return_invoices_pool: None,
        purchase_orders: None,
        purchase_orders_pool: None,
        print_templates: None,
        print_pool: None,
        products_v2: None,
        products_v2_pool: None,
        ocr: None,
        ocr_pool: None,
        uploads_dir: PathBuf::from("uploads"),
        store_pool: None,
        stores: None,
        setup: None,
        node_config: Default::default(),
        local: None,
    }
}

fn token(role: &str) -> String {
    auth::create_access_token("00000000-0000-0000-0000-0000000000ff", role, &[], SECRET)
        .expect("JWT для тесту")
}

/// GET через реальний роутер; `token = None` — запит без авторизації.
async fn get(path: &str, token: Option<&str>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .expect("request");
    if let Some(token) = token {
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("header"),
        );
    }
    let response = build_router(state())
        .oneshot(request)
        .await
        .expect("відповідь роутера");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("тіло")
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| panic!("немає заголовка {name}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn allowed_roles_get_snapshot_with_real_sha256_and_length() {
    use_artifact_dir();
    for role in ["device", "admin", "owner"] {
        let (status, headers, body) = get("/api/v1/sync/snapshot", Some(&token(role))).await;
        assert_eq!(status, StatusCode::OK, "роль {role}");
        assert_eq!(
            header(&headers, "content-type"),
            "application/octet-stream",
            "роль {role}"
        );
        assert_eq!(
            header(&headers, "content-length"),
            REAL_LENGTH.to_string(),
            "роль {role}"
        );
        assert_eq!(
            header(&headers, "x-snapshot-sha256"),
            REAL_SHA256,
            "роль {role}"
        );
        assert_eq!(
            header(&headers, "x-snapshot-filename"),
            REAL_FILENAME,
            "роль {role}"
        );
        assert_eq!(
            header(&headers, "content-disposition"),
            format!("attachment; filename=\"{REAL_FILENAME}\""),
            "роль {role}"
        );
        assert_eq!(body.len(), REAL_LENGTH, "тіло ролі {role}");
        println!(
            "[R1] role={role} → {status} | x-snapshot-sha256={} | content-length={} | \
             body.len={} | sha256(body)={} | x-snapshot-filename={}",
            header(&headers, "x-snapshot-sha256"),
            header(&headers, "content-length"),
            body.len(),
            torgashka_api::sync_snapshot::sha256_hex(&body),
            header(&headers, "x-snapshot-filename"),
        );
        // Заголовок і тіло — ті самі байти (хеш тіла збігається з X-Snapshot-Sha256).
        assert_eq!(
            torgashka_api::sync_snapshot::sha256_hex(&body),
            header(&headers, "x-snapshot-sha256"),
            "хеш тіла не збігається із заголовком (роль {role})"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_needs_no_store_header() {
    use_artifact_dir();
    // Жодного X-Store-Id: store_middleware до цього роутера не підключений —
    // скоуп точки для знімка всієї БД був би хибним.
    let (status, headers, body) = get("/api/v1/sync/snapshot", Some(&token("device"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("x-store-id").is_none());
    assert_eq!(header(&headers, "x-snapshot-filename"), REAL_FILENAME);
    assert_eq!(body.len(), REAL_LENGTH);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_roles_are_403_with_explanation_and_anonymous_is_401() {
    use_artifact_dir();
    for role in ["cashier", "store_manager"] {
        let (status, _headers, body) = get("/api/v1/sync/snapshot", Some(&token(role))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "роль {role}");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("JSON 403");
        let detail = json["detail"].as_str().expect("detail");
        assert!(
            detail.contains(role),
            "detail без фактичної ролі ({role}): {detail}"
        );
        assert!(
            detail.contains("device"),
            "detail без пояснення, якої ролі бракує: {detail}"
        );
    }

    let (status, _headers, _body) = get("/api/v1/sync/snapshot", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "без JWT");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_path_is_not_public_and_rejects_garbage_token() {
    use_artifact_dir();
    let (status, _headers, _body) = get("/api/v1/sync/snapshot", Some("не-токен")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "сміттєвий токен");
}
