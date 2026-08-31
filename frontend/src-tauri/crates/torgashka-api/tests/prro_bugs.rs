// ─────────────────────────────────────────────────────────────────────────────
// Регресійні тести двох багів ПРРО (Torgashka 4.0.0):
//
//   БАГ 1: POST /api/v2/prro/test-connection вбивав ВЕСЬ процес (release):
//          IIT SDK EUSignCP (cspb.so) — general protection fault при
//          EUReadPrivateKeyBinary (DSTU 4145 ключ). Відтворено: ядро
//          `traps: tokio-rt-worker general protection fault in cspb.so`
//          offset 0x7a925. Фікс: (а) naked-asm обгортка з примусовим
//          вирівнюванням стека; (б) ІЗОЛЯЦІЯ всіх FFI-викликів SDK у
//          субпроцесі (SDK_HELPER_ENV) — крах SDK вбиває лише хелпер.
//
//   БАГ 2: POST /api/v2/prro/shift/open без тіла (axios data=undefined,
//          без Content-Type) → 415 Unsupported Media Type (axum-екстрактор
//          Json<Value>). Фікс: ручне читання тіла (axum::body::to_bytes),
//          порожнє тіло → comment=None, НЕ 415.
//
// Запуск: cargo test -p torgashka-api --test prro_bugs
// Потрібна PostgreSQL (backend/.env: pos_system_fresh) + ключі ПРРО у
// certs/prro-test/ (nastya_key.jks / pb_3791505547 (2).jks).
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;

use torgashka_api::AppState;
use torgashka_infrastructure::store_ctx::StorePool;

/// Реальний користувач-власник з БД (прив'язаний до обох точок).
const OWNER_UUID: &str = "e30d480c-ef3b-4d0e-8808-0c745196d3d8";
/// Реальна точка з БД.
const STORE_UUID: &str = "65d5db51-672f-4a38-9c1e-f36c5feb5374";

/// Абсолютний шлях до кореня репозиторію Torgashka.
fn repo_root() -> std::path::PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // crates/torgashka-api → frontend/src-tauri/crates → ... → torgashka/
    manifest
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

/// Реальний AppState: PostgreSQL + pos + prro (як serve_listener).
async fn real_state() -> AppState {
    let pool = torgashka_infrastructure::db::connect_readonly_pool(5)
        .await
        .expect("PostgreSQL доступна (backend/.env)");
    let store_pool = StorePool::new(pool.clone());

    let pos: Option<Arc<dyn torgashka_domain::PosService + Send + Sync>> = Some(Arc::new(
        torgashka_infrastructure::repositories::pos::SqlxPos::new(store_pool.clone()),
    ));

    let prro = match torgashka_infrastructure::prro::SqlxPrroRepository::connect(store_pool.clone())
        .await
    {
        Ok(repo) => Some(Arc::new(torgashka_api::prro::PrroFacade::new(repo, false))),
        Err(e) => {
            eprintln!("[test] PrroRepository недоступний ({e}) — prro-тести пропущено");
            None
        }
    };

    let jwt_secret =
        torgashka_api::auth::resolve_jwt_secret().expect("JWT секрет (backend/.env SECRET_KEY)");

    AppState {
        jwt_secret: Arc::new(jwt_secret),
        readdirs: None,
        write: None,
        write_pool: Some(pool.clone()),
        pos,
        ledger: None,
        auth: None,
        prro,
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
        store_pool: Some(store_pool),
        stores: None,
        setup: None,
    }
}

fn valid_token(secret: &str) -> String {
    torgashka_api::auth::create_access_token(OWNER_UUID, "owner", &["*".to_string()], secret)
        .expect("токен має створитись")
}

fn post(
    path: &str,
    token: &str,
    store: &str,
    body: Option<&str>,
    ct: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("X-Store-Id", store);
    if let Some(ct) = ct {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    match body {
        Some(b) => builder.body(Body::from(b.to_string())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

// ─── БАГ 1: test-connection з битим ключем → 4xx з текстом, процес НЕ падає ──

#[tokio::test]
async fn bug1_test_connection_bad_key_returns_4xx_no_crash() {
    // Ключ із НЕПРАВИЛЬНИМ паролем через env-fallback (keystore-файл у CWD
    // тесту відсутній → PrroKeyStore::decrypt_password() помилка → контекст
    // бере PRRO_KEY_FILE/PRRO_KEY_PASSWORD).
    let root = repo_root();
    let bad_key = root.join("certs/prro-test/pb_3791505547 (2).jks");
    if !bad_key.is_file() {
        eprintln!("SKIP: ключ {bad_key:?} не знайдено");
        return;
    }
    std::env::set_var("PRRO_KEY_FILE", &bad_key);
    std::env::set_var("PRRO_KEY_PASSWORD", "test123"); // НЕПРАВИЛЬНИЙ пароль

    let state = real_state().await;
    if state.prro.is_none() {
        eprintln!("SKIP: Rust-гілка ПРРО недоступна");
        return;
    }
    std::env::set_var("TORGASHKA_RUST_PRRO_V2", "1");
    let app = torgashka_api::router_v1::build_router(state);

    let token = valid_token(&torgashka_api::auth::resolve_jwt_secret().expect("секрет"));
    let req = post(
        "/api/v2/prro/test-connection",
        &token,
        STORE_UUID,
        None,
        None,
    );
    let resp = app.oneshot(req).await.expect("відповідь фасаду");

    // Якщо SDK вбивав процес (#GP, до фіксу) — тест-процес помер би ТУТ.
    let status = resp.status();
    assert!(
        status != StatusCode::OK,
        "test-connection з битим ключем не може бути 200"
    );
    assert!(
        status.is_client_error(),
        "очікувався 4xx, отримано {status}"
    );
    assert_ne!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "помилка має бути клієнтською (4xx), не 500"
    );
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .expect("тіло відповіді");
    let text = String::from_utf8_lossy(&bytes).to_string();
    assert!(
        !text.trim().is_empty(),
        "відповідь має містити текст помилки"
    );
    eprintln!("[bug1] status={status} body={text:?} — процес вцілів ✓");
}

// ─── БАГ 2: POST /shift/open без тіла → НЕ 415 ──────────────────────────────

#[tokio::test]
async fn bug2_shift_open_no_body_not_415() {
    let state = real_state().await;
    if state.pos.is_none() {
        eprintln!("SKIP: POS недоступний");
        return;
    }
    let app = torgashka_api::router_v1::build_router(state);
    let token = valid_token(&torgashka_api::auth::resolve_jwt_secret().expect("секрет"));

    // Той самий запит, що шле фронтенд: без тіла, без Content-Type.
    let req = post("/api/v2/prro/shift/open", &token, STORE_UUID, None, None);
    let resp = app.oneshot(req).await.expect("відповідь фасаду");
    let status = resp.status();
    assert_ne!(
        status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "open_shift без тіла не має давати 415"
    );
    // Дозволено 200 (зміну відкрито) або 4xx з текстом (сервісна помилка),
    // але НЕ 415 і не 500.
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .expect("тіло відповіді");
    let text = String::from_utf8_lossy(&bytes).to_string();
    eprintln!("[bug2] open_shift без тіла → {status} body={text:?} ✓");
}

#[tokio::test]
async fn bug2_shift_close_no_body_not_415() {
    let state = real_state().await;
    if state.pos.is_none() {
        eprintln!("SKIP: POS недоступний");
        return;
    }
    let app = torgashka_api::router_v1::build_router(state);
    let token = valid_token(&torgashka_api::auth::resolve_jwt_secret().expect("секрет"));
    let req = post("/api/v2/prro/shift/close", &token, STORE_UUID, None, None);
    let resp = app.oneshot(req).await.expect("відповідь фасаду");
    let status = resp.status();
    assert_ne!(
        status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "close_shift без тіла не має давати 415"
    );
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    eprintln!("[bug2] close_shift без тіла → {status} ✓");
}
