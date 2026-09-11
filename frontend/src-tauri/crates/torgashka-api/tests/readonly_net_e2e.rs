//! E2E: `readonly_net` — «останній рубіж» запису в read-only репліку.
//!
//! ## Що доводять тести
//! 1. **Контроль (без PG)**: middleware переписує ЛИШЕ ті відповіді, у тілі
//!    яких є ОДНА З ДВОХ стабільних сигнатур:
//!    * маркер `[READ_ONLY_REPLICA]` (наш код) → 503 §4 з обома заголовками
//!      і людським текстом — гілка з пріоритетом;
//!    * префікс `Display` помилки БД sqlx `error returned from database: `
//!      (бібліотека; sqlx-core-0.8.6/src/error.rs:44) → тіло замінюється на
//!      людський текст без фрагментів PG, СТАТУС хендлера зберігається,
//!      додається `X-Torgashka-Sanitized: db-error` і росте
//!      `readonly_guard::sanitized_hits()`.
//!
//!    Усе інше повертається байт-в-байт (включно з власним 503 гейта, де
//!    сигнатур немає, з читаннями і з тілами > 64 КіБ, які не читаються).
//! 2. **Інтеграційно на реальному PostgreSQL**: вузол, чиї пули фізично
//!    read-only (`-c default_transaction_read_only=on`), але `mode=primary`
//!    (тобто гейт НЕ знає про проблему й пропускає запит) — `PUT` на
//!    write-маршрут ПРРО через `StorePool` дає 503 §4 (а не сирий 400 зі
//!    стеком PostgreSQL), і `readonly_guard::hits() > 0`.
//!
//! ## Чому `PUT /api/v2/prro/settings`
//! Це єдиний write-маршрут ПРРО, який доходить до DML через `StorePool` БЕЗ
//! зовнішніх залежностей (gRPC ДПС + КЕП-ключ). `POST`-гілки
//! (`/fiscal/shift/open`, `/fiscal/sync`, `/fiscalize`) спершу будують
//! контекст із КЕП-файлом, тож у тесті без ключів падають раніше за DML.
//! Другий write-метод (`POST`) покритий у контролі (п.1).

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

use tokio::sync::Mutex;
use torgashka_api::auth::create_access_token;
use torgashka_api::readonly_net::{
    readonly_net_middleware, SANITIZED_DB_ERROR, SANITIZED_DETAIL, SANITIZED_HEADER,
};
use torgashka_api::write_gate::{STANDBY_DETAIL, UPSTREAM_DOWN, UPSTREAM_HEADER};
use torgashka_api::{router_v1, AppState};
use torgashka_infrastructure::node_config::{NodeConfig, NodeMode};
use torgashka_infrastructure::readonly_guard as guard;
use torgashka_infrastructure::store_ctx::StorePool;

const SECRET: &str = "readonly-net-e2e-secret";
const MARKER: &str = "[READ_ONLY_REPLICA]";
const NODE_MODE_HEADER: &str = "x-torgashka-node-mode";
const PG_TEXT: &str = "cannot execute INSERT in a read-only transaction";
/// Сигнатура БІБЛІОТЕКИ: префікс `Display` `sqlx::Error::Database`
/// (sqlx-core-0.8.6/src/error.rs:44). Тут — той самий літерал, щоб тест
/// доводив саме те, що бачить клієнт.
const SQLX_PREFIX: &str = "error returned from database: ";

/// Тести бінаря ділять одну тестову БД і глобальні метрики → послідовно.
static SEQ: Mutex<()> = Mutex::const_new(());

// ─────────────────────────────────────────────────────────────────────────────
// (1) КОНТРОЛЬ: middleware без PG
// ─────────────────────────────────────────────────────────────────────────────

fn probe_state(mode: NodeMode) -> AppState {
    AppState {
        jwt_secret: Arc::new("test-secret-readonly-net".to_string()),
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

/// Внутрішній «сервіс» віддає 400 із тілом, що містить маркер (як це робить
/// хендлер, який дістав помилку репліки з репозиторію).
fn probe_app(mode: NodeMode) -> Router {
    Router::new()
        .route(
            "/probe/marked",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "detail": format!("error returned from database: {PG_TEXT} {MARKER} вузол у режимі standby") })),
                )
            }),
        )
        .route(
            "/probe/marked-read",
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "detail": format!("{MARKER} (читання не інспектується)") })),
                )
            }),
        )
        .route(
            "/probe/clean",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "detail": "помилка валідації: prro_fn" })),
                )
            }),
        )
        // Сира помилка БД (префікс sqlx) зі статусом хендлера 400/500 і читанням.
        .route(
            "/probe/db-error-400",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "detail": format!("{SQLX_PREFIX}{PG_TEXT}") })),
                )
            }),
        )
        .route(
            "/probe/db-error-500",
            post(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "detail": format!("{SQLX_PREFIX}{PG_TEXT}") })),
                )
            }),
        )
        .route(
            "/probe/db-error-read",
            get(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "detail": format!("{SQLX_PREFIX}{PG_TEXT}") })),
                )
            }),
        )
        // Обидві сигнатури в одному тілі: маркер репліки мусить перемогти.
        .route(
            "/probe/prefix-and-marker",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "detail": format!("{SQLX_PREFIX}{PG_TEXT} {MARKER} вузол у режимі standby")
                    })),
                )
            }),
        )
        // Сигнатура, але тіло понад поріг інспекції (не читаємо).
        .route(
            "/probe/big-db-error",
            post(|| async {
                let big = "x".repeat(70 * 1024);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "detail": format!("{SQLX_PREFIX}{PG_TEXT} {big}") })),
                )
            }),
        )
        .route(
            "/probe/gate-503",
            post(|| async { torgashka_api::write_gate::standby_503("/probe/gate-503") }),
        )
        .route(
            "/probe/big-marked",
            post(|| async {
                let big = "x".repeat(70 * 1024);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "detail": format!("{MARKER} {big}") })),
                )
            }),
        )
        .layer(from_fn_with_state(
            probe_state(mode),
            readonly_net_middleware,
        ))
}

async fn probe_call(
    app: &Router,
    method: &str,
    path: &str,
) -> (StatusCode, String, axum::http::HeaderMap) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("запит");
    let resp = app.clone().oneshot(req).await.expect("відповідь");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024 * 1024)
        .await
        .expect("тіло");
    (status, String::from_utf8_lossy(&bytes).to_string(), headers)
}

#[tokio::test]
async fn marker_body_is_rewritten_to_503_contract_without_pg_text() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);
    let before = guard::fallback_hits();
    let (status, raw, headers) = probe_call(&app, "POST", "/probe/marked").await;

    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "має бути 503 §4: {raw}"
    );
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "500 заборонено");
    assert_eq!(
        headers.get(NODE_MODE_HEADER).and_then(|v| v.to_str().ok()),
        Some("standby"),
        "заголовок режиму вузла"
    );
    assert_eq!(
        headers.get(UPSTREAM_HEADER).and_then(|v| v.to_str().ok()),
        Some(UPSTREAM_DOWN),
        "заголовок стану апстріму"
    );
    let body: Value = serde_json::from_str(&raw).expect("JSON-тіло");
    assert_eq!(
        body,
        json!({ "detail": STANDBY_DETAIL }),
        "тіло — контракт §4 (людський текст)"
    );
    assert!(!raw.contains(MARKER), "маркер прибрано з тіла: {raw}");
    assert!(
        !raw.contains("cannot execute"),
        "тексту PostgreSQL у тілі немає: {raw}"
    );
    assert!(
        guard::fallback_hits() > before,
        "метрика «фолбек відпрацював» зросла"
    );
    println!("[readonly_net] 503 §4: {status} | {raw} | {headers:?}");
}

#[tokio::test]
async fn bodies_without_marker_are_byte_identical() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);

    let (s1, clean, _) = probe_call(&app, "POST", "/probe/clean").await;
    assert_eq!(s1, StatusCode::BAD_REQUEST);
    assert_eq!(
        clean, r#"{"detail":"помилка валідації: prro_fn"}"#,
        "тіло без маркера НЕ змінене"
    );

    // Кейс із `/probe/pg-text-no-marker` (сирий текст PG БЕЗ жодної сигнатури)
    // ЗНЯТО у Фазі 2c: він закріплював ВИТІК тексту PostgreSQL у тіло відповіді
    // як очікувану поведінку. З Фази 2b така відповідь САНАЦЮЄТЬСЯ — позитивне
    // покриття:
    //   * `sqlx_db_error_prefix_status_400_is_sanitized_without_pg_text` (400),
    //   * `sqlx_db_error_prefix_status_500_keeps_handler_status` (500).
    // Пробу-маршрут прибрано разом із кейсом (більше ніде не задіяна).

    // Власний 503 гейта (STANDBY_DETAIL, без маркера) — не чіпаємо.
    let before = guard::fallback_hits();
    let (s3, gate, headers) = probe_call(&app, "POST", "/probe/gate-503").await;
    assert_eq!(s3, StatusCode::SERVICE_UNAVAILABLE);
    let gate_body: Value = serde_json::from_str(&gate).expect("JSON");
    assert_eq!(gate_body, json!({ "detail": STANDBY_DETAIL }));
    assert_eq!(
        headers.get(UPSTREAM_HEADER).and_then(|v| v.to_str().ok()),
        Some(UPSTREAM_DOWN)
    );
    assert_eq!(
        guard::fallback_hits(),
        before,
        "власний 503 гейта не зараховується як фолбек"
    );
    println!("[readonly_net] без сигнатур (чисте тіло + власний 503 гейта): {clean} | {gate}");
}

#[tokio::test]
async fn reads_and_oversized_bodies_are_never_inspected() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);

    // GET із маркером — читання не інспектуємо взагалі.
    let (s, body, _) = probe_call(&app, "GET", "/probe/marked-read").await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "читання не переписується: {body}"
    );
    assert!(body.contains(MARKER));

    // Тіло > 64 КіБ: Content-Length відомий → не читаємо, відповідь як є.
    let (s, big, _) = probe_call(&app, "POST", "/probe/big-marked").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "велике тіло не інспектується");
    assert!(big.len() > 64 * 1024, "тіло справді понад поріг");
    println!("[readonly_net] читання й тіло {} Б — не змінені", big.len());
}

// ─────────────────────────────────────────────────────────────────────────────
// (2) ІНТЕГРАЦІЯ: primary-вузол із read-only пулами → 503 §4, не сирий 400
// ─────────────────────────────────────────────────────────────────────────────

/// seed: точка + користувач-адмін + доступ (user_stores) — RW-пулом.
async fn seed_store_user(pool: &sqlx::PgPool) -> (uuid::Uuid, uuid::Uuid) {
    let store = uuid::Uuid::new_v4();
    let user = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'readonly-net точка')")
        .bind(store)
        .execute(pool)
        .await
        .expect("INSERT stores");
    sqlx::query(
        "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed) \
         VALUES ($1, 'readonly-net admin', $2, 'x', 'admin', true, now(), now(), true)",
    )
    .bind(user)
    .bind(format!("ro-net-{}", user.simple()))
    .execute(pool)
    .await
    .expect("INSERT users");
    sqlx::query(
        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at) \
         VALUES ($1, $2, 'admin', '{}', true, now())",
    )
    .bind(user)
    .bind(store)
    .execute(pool)
    .await
    .expect("INSERT user_stores");
    (store, user)
}

#[tokio::test]
async fn primary_node_with_read_only_pool_answers_503_not_raw_400() {
    let _seq = SEQ.lock().await;
    common::force_test_db();
    // `PUT /api/v2/prro/settings` монтується під прапорцем Rust-гілки ПРРО v2
    // (той самий шлях, що обслуговує UI налаштувань ПРРО у продакшні).
    std::env::set_var(torgashka_api::RUST_PRRO_V2_ENV, "1");

    // RW-пул — ЛИШЕ для підготовки схеми та seed-даних.
    let rw = torgashka_infrastructure::db::connect_test_pool(5)
        .await
        .expect("тестова БД (TEST_DATABASE_URL або <db>_test)");
    torgashka_infrastructure::db::ensure_schema(&rw)
        .await
        .expect("ensure_schema");
    let _ = torgashka_infrastructure::prro::SqlxPrroRepository::connect(StorePool::new(rw.clone()))
        .await
        .expect("схема ПРРО (DDL ідемпотентно)");
    let (store, user) = seed_store_user(&rw).await;

    // «Репліка»: ТА САМА БД, але сесія read-only (усі DML фізично неможливі).
    let opts = (*rw.connect_options())
        .clone()
        .options([("default_transaction_read_only", "on")]);
    let ro = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect_with(opts)
        .await
        .expect("read-only сесія");

    let hits_before = guard::hits();
    let mut state = probe_state(NodeMode::Primary); // гейт НЕ знає про проблему
    state.jwt_secret = Arc::new(SECRET.to_string());
    state.store_pool = Some(StorePool::new(ro.clone()));
    state.write_pool = Some(ro.clone());
    state.prro = Some(Arc::new(torgashka_api::prro::PrroFacade::new(
        torgashka_infrastructure::prro::SqlxPrroRepository::new(StorePool::new(ro.clone())),
        false,
    )));

    let app = router_v1::build_router(state);
    let token = create_access_token(&user.to_string(), "admin", &[], SECRET).expect("JWT");

    // Multipart: лише `prro_fn` → перша ж БД-операція у `save_settings` — DML
    // (`set_setting`), тож у репліку впираємось одразу, без КЕП/gRPC.
    let boundary = "----torgashkaReadonlyNetE2E";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"prro_fn\"\r\n\r\n1234567890\r\n--{boundary}--\r\n"
    );
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v2/prro/settings")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("authorization", format!("Bearer {token}"))
        .header("x-store-id", store.to_string())
        .body(Body::from(body))
        .expect("запит");
    let resp = app.clone().oneshot(req).await.expect("відповідь");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("тіло");
    let raw = String::from_utf8_lossy(&bytes).to_string();

    println!("[readonly_net:e2e] PUT /api/v2/prro/settings → {status} | {raw}");
    println!(
        "[readonly_net:e2e] hits={} (було {hits_before}) | fallback={}",
        guard::hits(),
        guard::fallback_hits()
    );
    println!("[readonly_net:e2e] last_hit={:?}", guard::last_hit());

    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "очікуємо 503 §4, а не сирий 400 зі стеком PG: {raw}"
    );
    assert!(
        !raw.contains("cannot execute") && !raw.contains("read-only transaction"),
        "тексту PostgreSQL у тілі немає: {raw}"
    );
    let parsed: Value = serde_json::from_str(&raw).expect("JSON-тіло");
    assert_eq!(parsed, json!({ "detail": STANDBY_DETAIL }), "тіло §4");
    assert_eq!(
        headers.get(UPSTREAM_HEADER).and_then(|v| v.to_str().ok()),
        Some(UPSTREAM_DOWN)
    );
    assert!(
        guard::hits() > hits_before,
        "фунел StorePool мусив спіймати відмову репліки (SQLSTATE 25006)"
    );
    assert!(
        guard::last_hit().is_some(),
        "є останній перехоплений запит (fingerprint + час)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (1b) ДРУГА СИГНАТУРА: префікс `Display` помилки БД sqlx (не 25006)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn sqlx_db_error_prefix_status_400_is_sanitized_without_pg_text() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);
    let before = guard::sanitized_hits();
    let (status, raw, headers) = probe_call(&app, "POST", "/probe/db-error-400").await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "статус хендлера НЕ змінюється (не маскуємо 400 під 503): {raw}"
    );
    assert_eq!(
        headers.get(SANITIZED_HEADER).and_then(|v| v.to_str().ok()),
        Some(SANITIZED_DB_ERROR),
        "заголовок санації"
    );
    let body: Value = serde_json::from_str(&raw).expect("JSON-тіло");
    assert_eq!(
        body,
        json!({ "detail": SANITIZED_DETAIL }),
        "людський текст"
    );
    assert!(
        !raw.contains(SQLX_PREFIX.trim_end())
            && !raw.contains("cannot execute")
            && !raw.contains("INSERT"),
        "жодного фрагмента PG/SQL у тілі: {raw}"
    );
    assert!(
        guard::sanitized_hits() > before,
        "метрика санації зросла ({before} → {})",
        guard::sanitized_hits()
    );
    let last = guard::last_sanitized().expect("остання санація зафіксована");
    assert!(
        last.0.contains("cannot execute"),
        "сирий текст лишився у метриці/журналі, а не у відповіді: {last:?}"
    );
    println!(
        "[readonly_net] sqlx-префікс + 400 → {status} | {raw} | sanitized={:?}",
        guard::last_sanitized()
    );
}

#[tokio::test]
async fn sqlx_db_error_prefix_status_500_keeps_handler_status() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);
    let before = guard::sanitized_hits();
    let (status, raw, headers) = probe_call(&app, "POST", "/probe/db-error-500").await;

    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 лишається 500 (статуси хендлерів не переписуємо): {raw}"
    );
    assert_eq!(
        headers.get(SANITIZED_HEADER).and_then(|v| v.to_str().ok()),
        Some(SANITIZED_DB_ERROR)
    );
    let body: Value = serde_json::from_str(&raw).expect("JSON-тіло");
    assert_eq!(body, json!({ "detail": SANITIZED_DETAIL }));
    assert!(!raw.contains("cannot execute"), "тексту PG немає: {raw}");
    assert!(guard::sanitized_hits() > before);
    println!("[readonly_net] sqlx-префікс + 500 → {status} | {raw}");
}

#[tokio::test]
async fn read_only_marker_takes_priority_over_db_error_prefix() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);
    let fallback_before = guard::fallback_hits();
    let (status, raw, headers) = probe_call(&app, "POST", "/probe/prefix-and-marker").await;

    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "гілка 25006 має пріоритет → 503 §4: {raw}"
    );
    let body: Value = serde_json::from_str(&raw).expect("JSON-тіло");
    assert_eq!(
        body,
        json!({ "detail": STANDBY_DETAIL }),
        "тіло — контракт §4, а не санація помилки БД: {raw}"
    );
    assert_eq!(
        headers.get(UPSTREAM_HEADER).and_then(|v| v.to_str().ok()),
        Some(UPSTREAM_DOWN),
        "заголовок §4 присутній (санація його не ставить)"
    );
    assert_eq!(
        headers.get(NODE_MODE_HEADER).and_then(|v| v.to_str().ok()),
        Some("standby")
    );
    assert!(
        headers.get(SANITIZED_HEADER).is_none(),
        "заголовка санації немає — спрацювала перша гілка"
    );
    assert!(
        guard::fallback_hits() > fallback_before,
        "зараховано фолбек"
    );
    println!("[readonly_net] обидві сигнатури → пріоритет 25006: {status} | {raw}");
}

#[tokio::test]
async fn bodies_without_either_signature_stay_byte_identical() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);
    let sanitized_before = guard::sanitized_hits();
    let (status, raw, headers) = probe_call(&app, "POST", "/probe/clean").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        raw, r#"{"detail":"помилка валідації: prro_fn"}"#,
        "тіло без обох сигнатур — байт-у-байт"
    );
    assert!(
        headers.get(SANITIZED_HEADER).is_none(),
        "заголовка санації немає"
    );
    assert_eq!(
        guard::sanitized_hits(),
        sanitized_before,
        "лічильник санацій не зрушив"
    );
    println!("[readonly_net] без сигнатур: {raw} (байт-у-байт)");
}

#[tokio::test]
async fn reads_and_oversized_db_error_bodies_are_never_sanitized() {
    // Метрики `guard::*` ГЛОБАЛЬНІ на процес: тести бінаря серіалізуємо тим
    // самим мʼютексом, що й інтеграційний (інакше негативні перевірки
    // «лічильник не зрушив» ганяються з паралельними тестами).
    let _seq = SEQ.lock().await;
    let app = probe_app(NodeMode::Standby);
    let sanitized_before = guard::sanitized_hits();

    // GET із сирою помилкою БД — читання не інспектуємо взагалі.
    let (s, body, headers) = probe_call(&app, "GET", "/probe/db-error-read").await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "статус як є: {body}");
    assert!(
        body.contains("cannot execute"),
        "читання не санаціюється: {body}"
    );
    assert!(headers.get(SANITIZED_HEADER).is_none());

    // Тіло > 64 КіБ із сигнатурою: поріг не читаємо — відповідь як є.
    let (s, big, headers) = probe_call(&app, "POST", "/probe/big-db-error").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(big.len() > 64 * 1024, "тіло справді понад поріг");
    assert!(
        big.contains("cannot execute"),
        "велике тіло не санаціюється"
    );
    assert!(headers.get(SANITIZED_HEADER).is_none());

    assert_eq!(
        guard::sanitized_hits(),
        sanitized_before,
        "жодного зарахування санації (ні читання, ні понад поріг)"
    );
    println!(
        "[readonly_net] читання й тіло {} Б із сигнатурою — не санаційовані",
        big.len()
    );
}
