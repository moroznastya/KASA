//! WriteGate — ЄДИНЕ місце рішення «куди писати» на HTTP-поверхні фасаду
//! (ADR-0007 §11, F2–F5).
//!
//! Проблема, яку закриває модуль: на standby-вузлі `state.write_pool` вказує на
//! **локальну read-only репліку** (`lib.rs::init_readdirs` →
//! `db::connect_readonly_pool` → активне джерело db_sources.toml = репліка).
//! Будь-який безумовний `INSERT/UPDATE/DELETE` через цей пул дає
//! `ERROR: cannot execute INSERT in a read-only transaction` (факт 3 ADR-0007).
//!
//! Рішення (варіант B, §2.2):
//!   * `mode == Primary`   → **нічого не змінюється** (F2): пул як був;
//!   * `mode == Standby`:
//!       * `LocalOutbox` (POS-документи, сесії, heartbeat) → `Pass`
//!         (далі працює локальний обробник / SQLite-черга);
//!       * `ProxyToPrimary` (адмін/мережа) → HTTP pass-through на `server_url`
//!         (§11.2: без нової БД-ролі, з JWT користувача, verbatim-відповідь);
//!       * `DisabledOnStandby` (приймачі агрегатора, DDL провіжну) → `503` (§4).
//!
//! Заборонено і тому відсутнє у коді: сирий `500` від `sqlx::Error`, текст
//! помилки PostgreSQL у тілі відповіді, запис у локальну репліку (F5),
//! власні DB-креденшли гейта, retry/черга для адмін-операцій.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sqlx::PgPool;
use torgashka_infrastructure::node_config::NodeMode;

use crate::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Таблиця політик (§11.1 ADR-0007)
// ─────────────────────────────────────────────────────────────────────────────

/// Політика запису для сутності (§11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePolicy {
    /// Операційні дані вузла: локальний обробник + SQLite-черга.
    LocalOutbox,
    /// Глобальні дані: HTTP pass-through на primary (§11.2).
    ProxyToPrimary,
    /// На репліці не має сенсу: `503` на HTTP-поверхні (§4).
    DisabledOnStandby,
}

/// Таблиця політик «сутність → політика» — дослівно §11.1 ADR-0007.
///
/// Це ЄДИНЕ джерело рішення. Нова write-точка без рядка тут валить
/// `tests/write_gate_guard.rs` (CI-guard §11.4).
pub const POLICY_TABLE: &[(&str, WritePolicy)] = &[
    // POS-документи каси + операційні дані вузла (§11.1, рядки 1–3).
    ("receipt", WritePolicy::LocalOutbox),
    ("return_receipt", WritePolicy::LocalOutbox),
    ("purchase_order", WritePolicy::LocalOutbox),
    ("inventory", WritePolicy::LocalOutbox),
    ("transfer", WritePolicy::LocalOutbox),
    ("write_off", WritePolicy::LocalOutbox),
    ("invoice", WritePolicy::LocalOutbox),
    ("work_session", WritePolicy::LocalOutbox),
    ("device_heartbeat", WritePolicy::LocalOutbox),
    // Адмін/мережа — єдине джерело істини primary (§11.1, рядок 4).
    ("stores", WritePolicy::ProxyToPrimary),
    ("user_stores", WritePolicy::ProxyToPrimary),
    ("devices", WritePolicy::ProxyToPrimary),
    ("store_activation_codes", WritePolicy::ProxyToPrimary),
    ("network_nodes", WritePolicy::ProxyToPrimary),
    ("prro_settings", WritePolicy::ProxyToPrimary),
    ("migrate_legacy", WritePolicy::ProxyToPrimary),
    ("audit_log", WritePolicy::ProxyToPrimary),
    ("network_events", WritePolicy::ProxyToPrimary),
    // Агрегатор-only приймачі + DDL провіжну + фоновий job (§11.1, рядки 5–7).
    ("sync_push", WritePolicy::DisabledOnStandby),
    ("store_sync_state", WritePolicy::DisabledOnStandby),
    ("sync_log", WritePolicy::DisabledOnStandby),
    ("replication_ddl_role", WritePolicy::DisabledOnStandby),
    ("network_nodes_offline_job", WritePolicy::DisabledOnStandby),
];

/// Політика для сутності. `None` — сутність не класифікована (§11.1).
pub fn policy_for(entity: &str) -> Option<WritePolicy> {
    POLICY_TABLE
        .iter()
        .find(|(name, _)| *name == entity)
        .map(|(_, policy)| *policy)
}

// ─────────────────────────────────────────────────────────────────────────────
// Контракт §4
// ─────────────────────────────────────────────────────────────────────────────

/// Заголовок-маркер режиму вузла — у КОЖНІЙ відповіді фасаду (§4).
pub const NODE_MODE_HEADER: &str = "x-torgashka-node-mode";
/// Заголовок стану апстріму (додається на 503, §4).
pub const UPSTREAM_HEADER: &str = "x-torgashka-upstream";
/// Заголовок Retry-After (додається на 503, §4).
pub const RETRY_AFTER_HEADER: &str = "retry-after";
/// Скільки секунд радить зачекати 503 (§4).
pub const RETRY_AFTER_SECS: u64 = 30;

/// Тіло 503 за §4 — рівно цей текст, без тексту помилки PostgreSQL.
pub const STANDBY_DETAIL: &str = "primary недоступний (standby-вузол): адміністративна операція не може бути виконана локально — повторіть, коли мережа відновиться";

/// Текст помилки гейта на standby (те саме повідомлення §4).
pub const UPSTREAM_DOWN: &str = "down";

/// Повідомлення «пулу немає» — дослівно як у хендлерах до гейта (F2:
/// на primary текст і статус не змінюються).
pub const NO_POOL_MSG: &str = "write_pool не ініціалізовано";

/// Таймаут HTTP pass-through на primary (контракт: ≤10 с).
const PROXY_TIMEOUT: Duration = Duration::from_secs(10);
/// Максимальний розмір тіла запиту, що проксіюється.
const PROXY_MAX_BODY: usize = 32 * 1024 * 1024;
/// Анти-спам логу: не частіше 1/сек на маршрут (§4).
const LOG_THROTTLE: Duration = Duration::from_secs(1);

// ─────────────────────────────────────────────────────────────────────────────
// Рішення
// ─────────────────────────────────────────────────────────────────────────────

/// Що робити з запитом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Пропустити до наявного обробника (F2 на primary; LocalOutbox на standby).
    Pass,
    /// HTTP pass-through на primary (§11.2).
    Proxy,
    /// `503` за §4, вниз запит НЕ пускаємо.
    Disabled,
}

/// Рішення гейта: `(режим вузла, сутність) → дія`. Єдине місце правила §2.2.
pub fn decide(mode: NodeMode, entity: Option<&str>) -> GateDecision {
    match mode {
        // F2: primary — поведінка НЕ змінюється НІКОЛИ.
        NodeMode::Primary => GateDecision::Pass,
        NodeMode::Standby => match entity.and_then(policy_for) {
            Some(WritePolicy::LocalOutbox) => GateDecision::Pass,
            Some(WritePolicy::ProxyToPrimary) => GateDecision::Proxy,
            Some(WritePolicy::DisabledOnStandby) => GateDecision::Disabled,
            // Некласифікована поверхня → pass-through (ADR-0007 §11.1 без рядка).
            None => GateDecision::Pass,
        },
    }
}

/// Маркер режиму вузла для заголовка `X-Torgashka-Node-Mode`.
pub fn mode_label(mode: NodeMode) -> &'static str {
    match mode {
        NodeMode::Primary => "primary",
        NodeMode::Standby => "standby",
    }
}

/// Чи метод є write-методом HTTP-поверхні.
pub fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Класифікація HTTP-поверхні → сутність
// ─────────────────────────────────────────────────────────────────────────────

/// Мапа «HTTP-поверхня → сутність» §11.1.
///
/// Читання (`GET/HEAD/OPTIONS`) НЕ гейтуються: локальна репліка є джерелом
/// читання (§10 ADR-0007), F5 забороняє лише ЗАПИС у неї.
///
/// `None` = поверхня не має рядка в §11.1 → pass-through. Такі поверхні
/// перелічені в АНОМАЛІЯХ звіту (напр. довідники `products/categories/...` —
/// пишуться через сервіси, не через літерали DML, тому їх не бачить скан §3).
pub fn classify_request(method: &Method, path: &str) -> Option<&'static str> {
    if !is_write_method(method) {
        return None;
    }
    let p = path.trim_end_matches('/');

    // ── Агрегатор-only приймачі (DisabledOnStandby) ──────────────────────────
    if p == "/api/v1/sync/push" {
        return Some("sync_push");
    }

    // ── Локальний канал каси (LocalOutbox) ───────────────────────────────────
    if let Some(rest) = p.strip_prefix("/api/v1/local/") {
        return match rest {
            "receipts" | "ops" | "sync/now" => Some("receipt"),
            _ => None,
        };
    }

    // ── Адмін/мережа (ProxyToPrimary) ────────────────────────────────────────
    if p.starts_with("/api/v1/admin/db-sources") {
        // Конфіг ДЖЕРЕЛ БД цього вузла (db_sources.toml) — локальний, не primary.
        return None;
    }
    if p == "/api/v1/admin/network-config/import" {
        // Мережевий конфіг (реєстр точок мережі) — глобальні дані → primary.
        return Some("stores");
    }
    if p == "/api/v1/admin/migrate/legacy" {
        return Some("migrate_legacy");
    }
    if let Some(tail) = p.strip_prefix("/api/v1/admin/stores") {
        if tail.ends_with("/workers") {
            return Some("user_stores");
        }
        if tail.ends_with("/activation-code") {
            return Some("store_activation_codes");
        }
        if tail.ends_with("/prro-settings") {
            return Some("prro_settings");
        }
        // /admin/stores, /admin/stores/:id (PUT/DELETE), /admin/stores/:id/delete
        return Some("stores");
    }
    if p.starts_with("/api/v1/admin/users/") {
        // Працівники точки: створення/деактивація/скидання — та сама
        // глобальна поверхня `users`/`user_stores` (§3.1 #7).
        return Some("user_stores");
    }
    if p == "/api/v1/admin/devices" || p.starts_with("/api/v1/admin/devices/") {
        return Some("devices");
    }
    if p == "/api/v1/devices/activate" {
        return Some("devices");
    }
    if p.starts_with("/api/v1/admin/network-nodes") || p.starts_with("/api/v1/network-nodes/") {
        return Some("network_nodes");
    }
    if p == "/api/v2/prro/settings" {
        // Та сама сутність `prro_settings` (§3.1 #10), але через ПРРО-сервіс
        // (не літерал DML в admin_prro.rs) → та сама політика §11.1.
        return Some("prro_settings");
    }

    // ── Точки мережі (ProxyToPrimary) ────────────────────────────────────────
    if p == "/api/v1/stores" || p == "/api/v1/user-stores" {
        return Some(if p == "/api/v1/stores" {
            "stores"
        } else {
            "user_stores"
        });
    }
    if p == "/api/v1/users" || p.starts_with("/api/v1/users/") {
        // Працівники/права/ставки — глобальна поверхня (див. АНОМАЛІЮ №1:
        // окремого рядка `users` у §11.1 немає → беремо `user_stores`).
        return Some("user_stores");
    }

    // ── POS-документи каси (LocalOutbox) ─────────────────────────────────────
    if p.starts_with("/api/v1/receipts") || p.starts_with("/api/v2/receipts") {
        return Some("receipt");
    }
    if p.starts_with("/api/v1/return-invoices") || p.starts_with("/api/v2/return-invoices") {
        return Some("return_receipt");
    }
    if p.starts_with("/api/v1/purchase-orders") {
        return Some("purchase_order");
    }
    if p.starts_with("/api/v1/inventory") {
        return Some("inventory");
    }
    if p.starts_with("/api/v1/transfers") {
        return Some("transfer");
    }
    if p.starts_with("/api/v1/write-offs") || p == "/api/v1/write-off-reasons" {
        return Some("write_off");
    }
    if p.starts_with("/api/v1/invoices") || p.starts_with("/api/v2/invoices") {
        return Some("invoice");
    }

    // ── Сесії вузла (LocalOutbox) ─────────────────────────────────────────────
    // §11.1 рядок 2: `work_session` — логін/логаут пишуться ЛОКАЛЬНО (F6:
    // SQLite-міграція 0009 + outbox-опа `work_session`; фасад будує
    // `SqlxAuth::with_standby(_, node_cfg.is_standby())` — lib.rs:254, 1216),
    // тому на standby ця поверхня НЕ блокується і PG не торкається.
    if p.starts_with("/api/v1/auth/") {
        return Some("work_session");
    }

    None
}

// ─────────────────────────────────────────────────────────────────────────────
// 503 за §4 + анти-спам лог
// ─────────────────────────────────────────────────────────────────────────────

/// Один рядок логу на маршрут, не частіше 1/сек (§4, анти-спам).
fn log_rejected(route: &str) {
    static LAST: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let seen = LAST.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    let mut guard = match seen.lock() {
        Ok(g) => g,
        // Отруєний мʼютекс (паніка в іншому потоці) не має валити обробку.
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(prev) = guard.get(route) {
        if now.duration_since(*prev) < LOG_THROTTLE {
            return;
        }
    }
    guard.insert(route.to_string(), now);
    drop(guard);
    eprintln!("[torgashka-api] upstream-write rejected (standby): {route}");
}

/// Відповідь 503 за контрактом §4 (тіло + `X-Torgashka-Upstream` + `Retry-After`).
pub fn standby_503(route: &str) -> Response {
    log_rejected(route);
    let mut resp = (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "detail": STANDBY_DETAIL })),
    )
        .into_response();
    stamp(&mut resp, UPSTREAM_HEADER, UPSTREAM_DOWN);
    let retry = RETRY_AFTER_SECS.to_string();
    if let Ok(v) = HeaderValue::from_str(&retry) {
        resp.headers_mut()
            .insert(HeaderName::from_static(RETRY_AFTER_HEADER), v);
    }
    resp
}

fn stamp(resp: &mut Response, name: &'static str, value: &'static str) {
    resp.headers_mut().insert(
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Хелпери доступу до пулу (друга лінія захисту, §B.10 контракту)
// ─────────────────────────────────────────────────────────────────────────────

/// Пул для ЗАПИСУ через гейт: на primary — рівно `state.write_pool` (F2,
/// байт-в-байт стара поведінка), на standby для `ProxyToPrimary`/
/// `DisabledOnStandby` — помилка з текстом §4 (пул НЕ повертається, у репліку
/// нічого не пишеться — F5).
pub fn admin_pool(state: &AppState, entity: &str) -> Result<PgPool, String> {
    match decide(state.node_config.mode, Some(entity)) {
        GateDecision::Pass => state
            .write_pool
            .clone()
            .ok_or_else(|| NO_POOL_MSG.to_string()),
        GateDecision::Proxy | GateDecision::Disabled => Err(STANDBY_DETAIL.to_string()),
    }
}

/// Пул для ЧИТАННЯ (GET-хендлери): локальна репліка — дозволене джерело
/// читання (§10 ADR-0007), F5 забороняє лише запис. Поведінка ідентична
/// колишньому `state.write_pool.clone().ok_or(...)`.
pub fn read_pool(state: &AppState) -> Result<PgPool, String> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| NO_POOL_MSG.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// ProxyToPrimary — HTTP pass-through (§11.2)
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP-клієнт гейта: без retry, без redirect-follow, таймаут ≤10 с.
fn proxy_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(PROXY_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// `server_url` з SQLite settings (той самий ключ, що використовує outbox —
/// нового конфіг-ключа НЕ вводимо, §11.2).
async fn upstream_base() -> Option<String> {
    let read = tokio::task::spawn_blocking(|| {
        torgashka_infrastructure::offline::commands::get_setting("server_url".to_string())
    })
    .await
    .ok()?;
    let raw = read.ok().flatten()?;
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Заголовки, які НЕ переносяться через проксі (hop-by-hop + довжина тіла).
fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "upgrade"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
    )
}

/// HTTP pass-through: `method + path + query + body + headers` як є на primary.
///
/// Жодного звернення до PostgreSQL звідси (у гейта немає власних
/// DB-креденшлів, §11.2). Будь-яка помилка (немає `server_url`, таймаут,
/// зʼєднання) → `503` за §4, не `500`.
pub async fn proxy_to_primary(req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();
    let full_path = match parts.uri.query() {
        Some(q) => format!("{path}?{q}"),
        None => path.clone(),
    };
    let Some(base) = upstream_base().await else {
        // `server_url` не задано (каса ще не активована / порожній) → 503.
        return standby_503(&path);
    };
    let url = format!("{base}{full_path}");
    let bytes: Bytes = match axum::body::to_bytes(body, PROXY_MAX_BODY).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "[torgashka-api] upstream-write proxy: тіло запиту не прочитано ({path}): {e}"
            );
            return standby_503(&path);
        }
    };
    let mut builder = proxy_client().request(parts.method.clone(), url);
    for (k, v) in parts.headers.iter() {
        if is_hop_by_hop(k) {
            continue;
        }
        builder = builder.header(k.clone(), v.clone());
    }
    let resp = match builder.body(bytes).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[torgashka-api] upstream-write proxy → primary недосяжний ({path}): {e}");
            return standby_503(&path);
        }
    };
    let status = resp.status();
    let headers: HeaderMap = resp.headers().clone();
    let payload = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[torgashka-api] upstream-write proxy: тіло відповіді не прочитано: {e}");
            return standby_503(&path);
        }
    };
    let mut out = match Response::builder().status(status).body(Body::from(payload)) {
        Ok(r) => r,
        Err(_) => return standby_503(&path),
    };
    for (k, v) in headers.iter() {
        if is_hop_by_hop(k) {
            continue;
        }
        out.headers_mut().insert(k.clone(), v.clone());
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Middleware — ОДНА точка на весь роутер фасаду
// ─────────────────────────────────────────────────────────────────────────────

/// Гейт запису фасаду (ADR-0007 §11):
///   * `X-Torgashka-Node-Mode` — у КОЖНУ відповідь (§4);
///   * primary → `next.run(req)` без будь-яких інших змін (F2);
///   * standby + `Disabled` → `503` (§4), запит вниз НЕ пускаємо;
///   * standby + `Proxy` → HTTP pass-through (§11.2);
///   * standby + `Pass`/некласифіковане → `next.run(req)`.
pub async fn gate_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let mode = state.node_config.mode;
    if mode == NodeMode::Primary {
        let mut resp = next.run(req).await;
        stamp(&mut resp, NODE_MODE_HEADER, mode_label(mode));
        return resp;
    }
    let entity = classify_request(req.method(), req.uri().path());
    let route = req.uri().path().to_string();
    let mut resp = match decide(mode, entity) {
        GateDecision::Pass => next.run(req).await,
        GateDecision::Disabled => standby_503(&route),
        GateDecision::Proxy => proxy_to_primary(req).await,
    };
    stamp(&mut resp, NODE_MODE_HEADER, mode_label(mode));
    resp
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести чистої логіки (без PG, без HTTP)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_always_passes_f2() {
        for (entity, _) in POLICY_TABLE {
            assert_eq!(
                decide(NodeMode::Primary, Some(entity)),
                GateDecision::Pass,
                "primary мусить НЕ змінювати поведінку: {entity}"
            );
        }
        assert_eq!(decide(NodeMode::Primary, None), GateDecision::Pass);
    }

    #[test]
    fn standby_decisions_match_policy_table() {
        for (entity, policy) in POLICY_TABLE {
            let expected = match policy {
                WritePolicy::LocalOutbox => GateDecision::Pass,
                WritePolicy::ProxyToPrimary => GateDecision::Proxy,
                WritePolicy::DisabledOnStandby => GateDecision::Disabled,
            };
            assert_eq!(
                decide(NodeMode::Standby, Some(entity)),
                expected,
                "{entity}"
            );
        }
        assert_eq!(decide(NodeMode::Standby, None), GateDecision::Pass);
        assert_eq!(
            decide(NodeMode::Standby, Some("невідома")),
            GateDecision::Pass
        );
    }

    #[test]
    fn standalone_503_contract() {
        let resp = standby_503("/api/v1/admin/stores");
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            resp.headers()
                .get(UPSTREAM_HEADER)
                .map(|v| v.to_str().unwrap()),
            Some("down")
        );
        assert_eq!(
            resp.headers()
                .get(RETRY_AFTER_HEADER)
                .map(|v| v.to_str().unwrap()),
            Some("30")
        );
    }

    #[test]
    fn classify_covers_policy_surfaces() {
        let m = &Method::POST;
        assert_eq!(classify_request(m, "/api/v1/sync/push"), Some("sync_push"));
        assert_eq!(classify_request(m, "/api/v1/admin/stores"), Some("stores"));
        assert_eq!(
            classify_request(m, "/api/v1/admin/stores/abc/workers"),
            Some("user_stores")
        );
        assert_eq!(
            classify_request(m, "/api/v1/admin/stores/abc/activation-code"),
            Some("store_activation_codes")
        );
        assert_eq!(
            classify_request(&Method::PUT, "/api/v1/admin/stores/abc/prro-settings"),
            Some("prro_settings")
        );
        assert_eq!(
            classify_request(m, "/api/v1/admin/migrate/legacy"),
            Some("migrate_legacy")
        );
        assert_eq!(
            classify_request(m, "/api/v1/devices/activate"),
            Some("devices")
        );
        assert_eq!(
            classify_request(&Method::PUT, "/api/v1/network-nodes/x/heartbeat"),
            Some("network_nodes")
        );
        assert_eq!(
            classify_request(&Method::PUT, "/api/v2/prro/settings"),
            Some("prro_settings")
        );
        assert_eq!(classify_request(m, "/api/v2/receipts"), Some("receipt"));
        assert_eq!(classify_request(m, "/api/v1/invoices"), Some("invoice"));
        // Читання не гейтується.
        assert_eq!(classify_request(&Method::GET, "/api/v1/admin/stores"), None);
    }
}
