//! Поведінковий тест WriteGate (ADR-0007 §11.4 п.3) — БЕЗ PostgreSQL.
//!
//! Стан фасаду: `node_config.mode = Standby`, `write_pool = None`, `local = None`,
//! `server_url` у SQLite відсутній (XDG_DATA_HOME → temp) ⇒ primary недосяжний.
//! Отже будь-який `ProxyToPrimary`-маршрут мусить дати **503 за §4** і жодного
//! дотику до пулу (пул = `None`: якщо б хендлер виконував SQL, відповідь була б
//! не 503 §4, а «write_pool не ініціалізовано»/500).
//!
//! «Нуль рядків у postgres.log репліки» доводиться СТРУКТУРНО: (1) у стані тесту
//! немає ЖОДНОГО пулу PG (`write_pool/store_pool = None`), (2) гейт відповідає
//! до маршрутизації (найзовнішній шар), тож хендлер не викликається взагалі.
//! Сильніший доказ (реальний postgres.log репліки) потребував би запущеної
//! standby-репліки — це e2e-обсяг AT-8/AT-9 (§5 ADR), не unit-гейт.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use serde_json::Value;
use torgashka_api::write_gate::{self, GateDecision, STANDBY_DETAIL};
use torgashka_api::{router_v1, AppState};
use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use tower::ServiceExt;

/// Стан фасаду без жодного пулу PG (доказ «PG не торкали»).
fn gate_state(mode: NodeMode) -> AppState {
    AppState {
        jwt_secret: Arc::new("test-secret-write-gate".to_string()),
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
        uploads_dir: std::path::PathBuf::from("uploads"),
        store_pool: None,
        stores: None,
        setup: None,
        node_config: NodeConfig {
            mode,
            ..NodeConfig::default()
        },
        local: None,
    }
}

/// Ізоляція SQLite-налаштувань: `server_url` відсутній ⇒ proxy → 503.
fn isolate_sqlite_settings() {
    let dir = tempfile::tempdir().expect("tempdir");
    // TempDir живе до кінця процесу тесту (leak через Box) — інакше каталог
    // зникне раніше за читання settings усередині middleware.
    let leaked: &'static std::path::Path = Box::leak(dir.keep().into_boxed_path());
    std::env::set_var("XDG_DATA_HOME", leaked);
}

async fn call(app: &Router, method: &str, path: &str) -> (StatusCode, Value, HeaderMap) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::empty())
        .expect("запит");
    let resp = app.clone().oneshot(req).await.expect("відповідь");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("тіло");
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, headers)
}

fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

fn is_gate_503(status: StatusCode, body: &Value, headers: &HeaderMap) -> bool {
    status == StatusCode::SERVICE_UNAVAILABLE
        && body.get("detail").and_then(Value::as_str) == Some(STANDBY_DETAIL)
        && header(headers, write_gate::UPSTREAM_HEADER) == Some(write_gate::UPSTREAM_DOWN)
}

/// Маршрути класу `ProxyToPrimary` (§11.1 «Адмін/мережа») — усі мусять дати 503 §4.
const PROXY_ROUTES: &[(&str, &str, &str)] = &[
    ("POST", "/api/v1/admin/stores", "stores"),
    (
        "PUT",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111",
        "stores",
    ),
    (
        "DELETE",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111",
        "stores",
    ),
    (
        "POST",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111/delete",
        "stores",
    ),
    ("POST", "/api/v1/admin/network-config/import", "stores"),
    ("POST", "/api/v1/stores", "stores"),
    (
        "POST",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111/workers",
        "user_stores",
    ),
    (
        "POST",
        "/api/v1/admin/users/11111111-1111-1111-1111-111111111111/deactivate",
        "user_stores",
    ),
    (
        "POST",
        "/api/v1/admin/users/11111111-1111-1111-1111-111111111111/reset-password",
        "user_stores",
    ),
    (
        "PUT",
        "/api/v1/users/11111111-1111-1111-1111-111111111111/permissions",
        "user_stores",
    ),
    ("POST", "/api/v1/user-stores", "user_stores"),
    ("POST", "/api/v1/admin/devices", "devices"),
    (
        "POST",
        "/api/v1/admin/devices/11111111-1111-1111-1111-111111111111/unblock",
        "devices",
    ),
    (
        "DELETE",
        "/api/v1/admin/devices/11111111-1111-1111-1111-111111111111",
        "devices",
    ),
    ("POST", "/api/v1/devices/activate", "devices"),
    (
        "POST",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111/activation-code",
        "store_activation_codes",
    ),
    ("POST", "/api/v1/admin/network-nodes", "network_nodes"),
    (
        "POST",
        "/api/v1/admin/network-nodes/11111111-1111-1111-1111-111111111111/archive",
        "network_nodes",
    ),
    (
        "POST",
        "/api/v1/admin/network-nodes/11111111-1111-1111-1111-111111111111/force-resync",
        "network_nodes",
    ),
    ("POST", "/api/v1/network-nodes/join", "network_nodes"),
    (
        "PUT",
        "/api/v1/network-nodes/11111111-1111-1111-1111-111111111111/heartbeat",
        "network_nodes",
    ),
    (
        "PUT",
        "/api/v1/admin/stores/11111111-1111-1111-1111-111111111111/prro-settings",
        "prro_settings",
    ),
    ("POST", "/api/v1/admin/migrate/legacy", "migrate_legacy"),
    ("PUT", "/api/v2/prro/settings", "prro_settings"),
];

#[tokio::test]
async fn standby_proxy_routes_return_503_contract() {
    isolate_sqlite_settings();
    let app = router_v1::build_router(gate_state(NodeMode::Standby));

    for (method, path, entity) in PROXY_ROUTES {
        // Клас поверхні справді ProxyToPrimary (єдине джерело рішення — §11.1).
        let classified = write_gate::classify_request(&method.parse().expect("метод"), path);
        assert_eq!(
            classified,
            Some(*entity),
            "{method} {path}: класифікація поверхні"
        );
        assert_eq!(
            write_gate::policy_for(entity),
            Some(write_gate::WritePolicy::ProxyToPrimary),
            "{entity}: політика §11.1"
        );

        let (status, body, headers) = call(&app, method, path).await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{method} {path}: очікували 503 §4, маємо {status}"
        );
        assert_eq!(
            body.get("detail").and_then(Value::as_str),
            Some(STANDBY_DETAIL),
            "{method} {path}: тіло мусить бути рівно §4"
        );
        assert_eq!(
            header(&headers, write_gate::UPSTREAM_HEADER),
            Some("down"),
            "{method} {path}: X-Torgashka-Upstream"
        );
        assert_eq!(
            header(&headers, write_gate::RETRY_AFTER_HEADER),
            Some("30"),
            "{method} {path}: Retry-After"
        );
        assert_eq!(
            header(&headers, write_gate::NODE_MODE_HEADER),
            Some("standby"),
            "{method} {path}: маркер режиму"
        );
        assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "жодного 500");
        eprintln!("[write_gate] standby {method} {path} → 503 §4 ({entity}) ✓");
    }
}

#[tokio::test]
async fn standby_disabled_route_sync_push_returns_503() {
    isolate_sqlite_settings();
    let app = router_v1::build_router(gate_state(NodeMode::Standby));
    assert_eq!(
        write_gate::decide(NodeMode::Standby, Some("sync_push")),
        GateDecision::Disabled
    );
    let (status, body, headers) = call(&app, "POST", "/api/v1/sync/push").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body.get("detail").and_then(Value::as_str),
        Some(STANDBY_DETAIL)
    );
    assert_eq!(header(&headers, write_gate::UPSTREAM_HEADER), Some("down"));
    assert_eq!(
        header(&headers, write_gate::NODE_MODE_HEADER),
        Some("standby")
    );
    eprintln!("[write_gate] standby POST /api/v1/sync/push → 503 §4 (sync_push) ✓");
}

#[tokio::test]
async fn standby_local_outbox_routes_are_not_gate_blocked() {
    isolate_sqlite_settings();
    let app = router_v1::build_router(gate_state(NodeMode::Standby));

    // POS-документи каси + локальний канал + auth: політика LocalOutbox/немає
    // рядка → гейт НЕ блокує (F4 стосується лише адмін-поверхні).
    let routes = [
        ("POST", "/api/v1/invoices", "invoice"),
        ("POST", "/api/v2/receipts", "receipt"),
        ("POST", "/api/v1/local/receipts", "receipt"),
        ("POST", "/api/v1/purchase-orders", "purchase_order"),
    ];
    for (method, path, entity) in routes {
        assert_eq!(
            write_gate::classify_request(&method.parse().expect("метод"), path),
            Some(entity)
        );
        let (status, body, headers) = call(&app, method, path).await;
        assert!(
            !is_gate_503(status, &body, &headers),
            "{method} {path}: гейт не має блокувати LocalOutbox, маємо {status} {body}"
        );
        assert_eq!(
            header(&headers, write_gate::UPSTREAM_HEADER),
            None,
            "{method} {path}: маркера upstream-down бути не може"
        );
        assert_eq!(
            header(&headers, write_gate::NODE_MODE_HEADER),
            Some("standby"),
            "{method} {path}: маркер режиму присутній завжди"
        );
        eprintln!("[write_gate] standby {method} {path} → {status} (не гейт-503) ✓");
    }

    // Сесії вузла (§11.1 рядок 2, F6): логін/логаут — `work_session` →
    // LocalOutbox → гейт не блокує (на standby пише SQLite + outbox).
    assert_eq!(
        write_gate::classify_request(&"POST".parse().expect("метод"), "/api/v1/auth/login"),
        Some("work_session")
    );
    assert_eq!(
        write_gate::policy_for("work_session"),
        Some(write_gate::WritePolicy::LocalOutbox)
    );
    let (status, body, headers) = call(&app, "POST", "/api/v1/auth/login").await;
    assert!(
        !is_gate_503(status, &body, &headers),
        "auth/login не мусить блокуватися гейтом, маємо {status}"
    );
    // Читання з репліки гейт теж не чіпає.
    let (status, _, headers) = call(&app, "GET", "/api/v1/admin/stores").await;
    assert_ne!(status, StatusCode::SERVICE_UNAVAILABLE, "GET не гейтується");
    assert_eq!(
        header(&headers, write_gate::NODE_MODE_HEADER),
        Some("standby")
    );
    eprintln!("[write_gate] standby GET /api/v1/admin/stores → {status} (читання репліки) ✓");
}

#[tokio::test]
async fn primary_mode_behavior_unchanged_f2() {
    isolate_sqlite_settings();
    let app = router_v1::build_router(gate_state(NodeMode::Primary));

    for (method, path, _) in PROXY_ROUTES {
        let (status, body, headers) = call(&app, method, path).await;
        assert!(
            !is_gate_503(status, &body, &headers),
            "F2: primary не мусить отримувати гейт-503 ({method} {path}, маємо {status})"
        );
        assert_eq!(
            header(&headers, write_gate::UPSTREAM_HEADER),
            None,
            "F2: на primary upstream-маркера немає"
        );
        assert_eq!(
            header(&headers, write_gate::NODE_MODE_HEADER),
            Some("primary"),
            "F2: маркер режиму primary у кожній відповіді (§4)"
        );
    }
    let (status, body, headers) = call(&app, "POST", "/api/v1/sync/push").await;
    assert!(
        !is_gate_503(status, &body, &headers),
        "F2: sync/push на primary не блокується гейтом"
    );
    assert_eq!(
        header(&headers, write_gate::NODE_MODE_HEADER),
        Some("primary")
    );
    eprintln!(
        "[write_gate] F2-регресія: {} маршрутів на primary без гейт-503 ✓",
        PROXY_ROUTES.len()
    );
}

#[test]
fn app_state_has_no_pg_pools_in_this_test() {
    // Структурний доказ «жодного SQL у репліку»: у стані тесту пулів немає.
    let st = gate_state(NodeMode::Standby);
    assert!(st.write_pool.is_none());
    assert!(st.store_pool.is_none());
    assert!(st.local.is_none());
    assert!(st.node_config.is_standby());
    eprintln!("[write_gate] write_pool=None, store_pool=None, local=None, mode=standby ✓");
}
