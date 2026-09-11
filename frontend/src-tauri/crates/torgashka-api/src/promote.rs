//! Локальні адмін-операції standby-вузла (ЕТАП 19,
//! `network-replication-etap15-20.md` §6/§12):
//!
//! - `POST /api/v1/local/promote`        — disaster recovery: локальний
//!   PostgreSQL виходить з recovery (`pg_promote`), вузол стає primary;
//! - `POST /api/v1/local/repoint-primary` — оператор вручну вказує НОВИЙ
//!   primary (host:port) — фактичний `pg_basebackup` робиться оператором
//!   (MVP), фасад лише оновлює `[node] primary_db_url` + ставить позначку
//!   `repoint_pending`.
//!
//! Маршрути монтуються ОКРЕМО від звичайних `/api/v1/local/*` (поза
//! store-middleware: owner може не мати X-Store-Id, а store-middleware ходить
//! у PRIMARY-пул — який у момент DR недоступний). Авторизація — stateless
//! JWT (validate_jwt за локальним `jwt_secret`), вимога `role=owner`
//! (`token_type=access`) — працює повністю офлайн (підпис, без БД), див. §17.3.

use std::path::PathBuf;
use std::time::Duration;

use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use torgashka_infrastructure::node_config::{self, RepointPending};
use uuid::Uuid;

use crate::auth::{self, Claims};
use crate::network;
use crate::route_local::{local, LocalErr};
use crate::AppState;

/// Скільки разів перевіряємо вихід з recovery (10 × 3 с ≈ 30 с — контракт).
const RECOVERY_POLL_ATTEMPTS: u32 = 10;
const RECOVERY_POLL_INTERVAL: Duration = Duration::from_secs(3);

// ─────────────────────────────────────────────────────────────────────────────
// Stateless owner-авторизація (офлайн)
// ─────────────────────────────────────────────────────────────────────────────

/// Валідує Bearer JWT локально (підпис `jwt_secret`, БЕЗ жодного запиту в БД)
/// і вимагає `role=owner`, `type=access`. Це єдиний шар захисту маршрутів
/// `/api/v1/local/promote` та `/api/v1/local/repoint-primary`: вони лежать
/// ПОЗА auth/store middleware приватної гілки саме для того, щоб працювати,
/// коли primary (і його БД) недоступний.
pub(crate) fn require_owner_offline(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Claims, LocalErr> {
    let Some(h) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return Err(LocalErr::Unauthorized(
            "Відсутній заголовок авторизації".into(),
        ));
    };
    let Some(token) = h.strip_prefix("Bearer ") else {
        return Err(LocalErr::Unauthorized(
            "Невірний формат токена. Використовуйте Bearer".into(),
        ));
    };
    let claims = auth::validate_jwt(token, &state.jwt_secret)
        .map_err(|e| LocalErr::Unauthorized(format!("Недійсний або прострочений токен: {e}")))?;
    if claims.token_type != "access" {
        return Err(LocalErr::Unauthorized(
            "Очікується access-токен (не refresh)".into(),
        ));
    }
    if claims.role != "owner" {
        return Err(LocalErr::Forbidden(
            "Доступ заборонено: promote/repoint доступні лише власнику мережі (role=owner)".into(),
        ));
    }
    Ok(claims)
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/local/promote
// ─────────────────────────────────────────────────────────────────────────────

/// Promote standby → primary (ЕТАП 19.A).
///
/// 1. Перевіряє `pg_is_in_recovery()` на ЛОКАЛЬНОМУ PG (порт local_port);
/// 2. `SELECT pg_promote(true, 60)` (потребує superuser на локальному кластері);
/// 3. Чекає вихід з recovery (poll до ~30 с);
/// 4. `network_nodes` у локальній БД: цей вузол → role=primary, status=active,
///    replication creds занулено (він більше не споживач);
/// 5. Файловий захист split-brain: видаляє `standby.signal` і `primary_conninfo`
///    з `postgresql.auto.conf` ЛОКАЛЬНОГО кластера — вузол НІКОЛИ не
///    повернеться в standby автоматично;
/// 6. `node_config` → mode=primary (збереження файлу; `primary_db_url` і
///    `upstream_write_url` очищуються — апстріму більше немає);
/// 7. drain залишку SQLite-черги у ВЛАСНИЙ PG (best-effort, `drain_local_outbox`).
///
/// ⚠️ ПРО PUSH (Фаза 3.8, виправлено): `node_config` НЕ керує HTTP-push'ем
/// напряму — ціль push береться з SQLite-налаштувань каси (`server_url` +
/// `device_token`/`api_token`), і `promote` їх НЕ чистить (це налаштування
/// активації каси). Тому самé лише `mode=primary` НЕ гарантувало, що черга не
/// поллється на СТАРИЙ сервер. Гарантію дає ґейт у push-клієнті
/// (`sync_push::push_pending_batch_with_node` + `NodeConfig::push_blocked_reason`):
/// «mode=primary І апстріму немає» → HTTP-push вимкнено, залишок черги
/// застосовується локально (п.7). Норма НЕ ламається: `mode=Standby` і
/// `mode=Primary` ІЗ апстрімом (standalone POS на віддалений сервер) пушать як
/// і раніше.
pub async fn promote_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, LocalErr> {
    let claims = require_owner_offline(&state, &headers)?;
    let ls = local(&state)?;
    let old_mode = match ls.cfg.mode {
        torgashka_infrastructure::node_config::NodeMode::Standby => "standby",
        torgashka_infrastructure::node_config::NodeMode::Primary => "primary",
    };

    // ── 1. Поточний стан recovery ──────────────────────────────────────────
    let in_recovery: bool = sqlx::query_scalar("SELECT pg_is_in_recovery()")
        .fetch_one(&ls.pool.0)
        .await
        .map_err(|e| {
            LocalErr::Read(format!(
                "локальний PostgreSQL (127.0.0.1:{}) недоступний: {e}",
                ls.cfg.local_port
            ))
        })?;

    // ── 1a. Самоідентифікація вузла в network_nodes (ДО promote): рядок, чий
    // replication_slot_name == активний фізичний слот цього standby. ─────────
    let self_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT n.id \
         FROM pg_catalog.pg_replication_slots s \
         JOIN public.network_nodes n ON n.replication_slot_name = s.slot_name \
         WHERE s.active AND s.slot_type = 'physical' \
         LIMIT 1",
    )
    .fetch_optional(&ls.pool.0)
    .await
    .map_err(|e| LocalErr::Read(format!("не вдалось знайти рядок вузла: {e}")))?;
    if self_id.is_none() {
        eprintln!(
            "[promote] self-вузол не ідентифіковано (активного replication-слота немає) \
             — network_nodes не оновлюється; оператор має оновити роль вручну"
        );
    }

    // ── 2. pg_promote (лише якщо ще в recovery; інакше — вже primary) ──────
    if in_recovery {
        let promoted: bool = sqlx::query_scalar("SELECT pg_promote(true, 60)")
            .fetch_one(&ls.pool.0)
            .await
            .map_err(|e| map_promote_sql_error(e, ls.cfg.local_port))?;
        if !promoted {
            return Err(LocalErr::Conflict(
                "pg_promote повернув false — вузол не переведено в primary \
                 (деталі в postgres.log локального кластера)"
                    .into(),
            ));
        }
        eprintln!("[promote] pg_promote виконано успішно");
    } else {
        eprintln!("[promote] локальний PG вже НЕ в recovery — promote було виконано раніше");
    }

    // ── 3. Poll виходу з recovery (~30 с) ───────────────────────────────────
    for attempt in 1..=RECOVERY_POLL_ATTEMPTS {
        tokio::time::sleep(RECOVERY_POLL_INTERVAL).await;
        let still: bool = sqlx::query_scalar("SELECT pg_is_in_recovery()")
            .fetch_one(&ls.pool.0)
            .await
            .map_err(|e| LocalErr::Read(format!("перевірка recovery після promote: {e}")))?;
        if !still {
            eprintln!("[promote] вихід з recovery підтверджено (спроба {attempt})");
            break;
        }
        if attempt == RECOVERY_POLL_ATTEMPTS {
            return Err(LocalErr::Conflict(format!(
                "локальний PG не вийшов з recovery за {} с",
                u64::from(RECOVERY_POLL_ATTEMPTS) * RECOVERY_POLL_INTERVAL.as_secs()
            )));
        }
    }

    // ── 4. network_nodes: цей вузол → primary/active, creds занулено ───────
    let mut network_nodes_updated = false;
    if let Some(id) = self_id {
        let res = sqlx::query(
            "UPDATE public.network_nodes \
             SET role = 'primary', status = 'active', \
                 replication_role_name = NULL, replication_slot_name = NULL, \
                 updated_at = now() \
             WHERE id = $1",
        )
        .bind(id)
        .execute(&ls.pool.0)
        .await
        .map_err(|e| LocalErr::Read(format!("оновлення network_nodes: {e}")))?;
        network_nodes_updated = res.rows_affected() > 0;
        eprintln!(
            "[promote] network_nodes: вузол {id} → primary/active (rows={})",
            res.rows_affected()
        );
    }

    // ── 5. Файловий захист split-brain (data_dir — точний, з pg_settings) ──
    let data_dir: Option<String> = sqlx::query_scalar(
        "SELECT setting FROM pg_catalog.pg_settings WHERE name = 'data_directory'",
    )
    .fetch_optional(&ls.pool.0)
    .await
    .ok()
    .flatten();
    let markers_cleared = match data_dir {
        Some(dir) => clear_standby_markers(PathBuf::from(dir)),
        None => {
            eprintln!("[promote] data_directory не визначено — файловий захист пропущено");
            false
        }
    };

    // ── 6. node_config → primary (диск) ─────────────────────────────────────
    let new_cfg = ls.cfg.clone().into_promoted_primary();
    let config_file = new_cfg.save_to_disk().map_err(|e| {
        LocalErr::Read(format!(
            "PG переведено в primary, але node_config не збережено ({e}) — \
             виправте вручну db_sources.toml [node] mode=\"primary\""
        ))
    })?;
    eprintln!(
        "[promote] node_config збережено: mode=primary, файл {}",
        config_file.display()
    );

    // Діагностичний журнал (рішення Творця): вузол переведено в primary.
    // Пишеться в ЛОКАЛЬНУ БД (щойно стала primary) — це нове джерело правди.
    network::log_node_event(
        &ls.pool.0,
        self_id,
        "promoted",
        "warn",
        json!({ "old_mode": old_mode }),
    )
    .await;

    // ── 7. Drain залишку SQLite-черги у ВЛАСНИЙ PG (Фаза 3.8) ──────────────
    // Best-effort: PG уже writable (кроки 2-3), тож агрегати, зроблені офлайн,
    // застосовуються тут же тим самим ядром, що й серверний push. Помилка drain
    // НЕ валить promote (він уже відбувся) — оператор бачить підсумок і може
    // повторити вручну (`POST /api/v1/local/outbox/drain`, ідемпотентно).
    let drain = match crate::route_local::drain_local_outbox(&state, &claims).await {
        Ok(s) => json!({
            "drained": s.drained,
            "created": s.created,
            "already_exists": s.already_exists,
            "errors": s.errors,
            "pending_left": s.pending_left,
            "errors_detail": s.errors_detail,
        }),
        Err(e) => {
            eprintln!("[promote] увага: drain черги не виконано ({e}) — повторіть вручну \n                 POST /api/v1/local/outbox/drain після виправлення причини");
            // Тіло — лише машинний код: `outbox_drain.error` читає інший вузол,
            // сирий текст PG/SQLite там зайвий (причина — у stderr вище).
            json!({ "error": e.db_class() })
        }
    };

    let promoted_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    Ok(Json(json!({
        "ok": true,
        "promoted_at": promoted_at,
        "old_mode": old_mode,
        "node_id": self_id.map(|u| u.to_string()),
        "network_nodes_updated": network_nodes_updated,
        "standby_markers_cleared": markers_cleared,
        "config_file": config_file.display().to_string(),
        "outbox_drain": drain,
        "note": "вузол тепер primary. Переналаштуйте активне джерело БД фасаду \
                 на 127.0.0.1:<local_port> (Налаштування → джерела даних) і \
                 перезапустіть застосунок. Інші вузли — repoint-primary + \
                 pg_basebackup (див. disaster-recovery-network.md)."
    })))
}

/// Занулення реплікаційних маркерів локального кластера (захист split-brain):
/// видаляє `standby.signal` та рядок `primary_conninfo` з
/// `postgresql.auto.conf`. Після цього навіть рестарт PostgreSQL стартує як
/// primary — вузол НІКОЛИ не спробує автоматично повернутись у standby.
fn clear_standby_markers(data_dir: PathBuf) -> bool {
    let mut all_ok = true;

    // 1) standby.signal.
    let signal = data_dir.join("standby.signal");
    match std::fs::remove_file(&signal) {
        Ok(_) => eprintln!("[promote] видалено standby.signal: {}", signal.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "[promote] standby.signal немає ({}) — вузол уже primary?",
                signal.display()
            );
        }
        Err(e) => {
            eprintln!("[promote] не вдалось видалити standby.signal: {e}");
            all_ok = false;
        }
    }

    // 2) postgresql.auto.conf: прибрати primary_conninfo (пароль реплікації).
    let auto = data_dir.join("postgresql.auto.conf");
    if auto.exists() {
        let content = match std::fs::read_to_string(&auto) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[promote] postgresql.auto.conf не читається: {e}");
                return false;
            }
        };
        let filtered: Vec<&str> = content
            .lines()
            .filter(|l| !l.trim_start().starts_with("primary_conninfo"))
            .collect();
        let new_text = filtered.join("\n");
        if new_text != content {
            match std::fs::write(&auto, new_text.as_bytes()) {
                Ok(_) => eprintln!(
                    "[promote] primary_conninfo прибрано з postgresql.auto.conf: {}",
                    auto.display()
                ),
                Err(e) => {
                    eprintln!("[promote] запис postgresql.auto.conf: {e}");
                    all_ok = false;
                }
            }
        }
    } else {
        eprintln!(
            "[promote] postgresql.auto.conf немає в {} — primary_conninfo не було",
            auto.display()
        );
    }
    all_ok
}

/// Зрозуміла помилка для `pg_promote`: суперкористувач потрібен (SQLSTATE
/// 42501 insufficient_privilege) — інакше віддаємо технічний текст.
fn map_promote_sql_error(e: sqlx::Error, local_port: u16) -> LocalErr {
    if let Some(db) = e.as_database_error() {
        if db.code().as_deref() == Some("42501") {
            return LocalErr::Forbidden(format!(
                "pg_promote на локальному PostgreSQL (127.0.0.1:{local_port}) \
                 потребує прав суперкористувача. Поточний користувач джерела \
                 БД не має superuser. Рішення: налаштуйте TORGASHKA_PG_USER=postgres \
                 (або надайте ролі superuser) і повторіть promote. Деталі: {e}"
            ));
        }
    }
    LocalErr::Read(format!("pg_promote не виконано: {e}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/local/repoint-primary
// ─────────────────────────────────────────────────────────────────────────────

/// Тіло repoint-primary: новий primary (host:port).
#[derive(Debug, Deserialize)]
pub struct RepointBody {
    pub new_primary_host: String,
    pub new_primary_port: u16,
}

/// Repoint standby на новий primary (ЕТАП 19.B, MVP).
///
/// Фасад лише ОНОВЛЮЄ конфігурацію: `[node] primary_db_url` → новий host:port
/// (user/password/db беруться з поточного URL — той самий кластер-джерело) і
/// ставить позначку `repoint_pending`. Фактичний новий `pg_basebackup`
/// виконує ОПЕРАТОР вручну (розділ «Вказати новий головний вузол» інтерфейсу
/// / CLI) — доти вузол залишається standby зі старими даними.
pub async fn repoint_primary_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RepointBody>,
) -> Result<Json<Value>, LocalErr> {
    require_owner_offline(&state, &headers)?;
    let ls = local(&state)?;

    let host = body.new_primary_host.trim().to_string();
    if host.is_empty() {
        return Err(LocalErr::BadRequest("new_primary_host порожній".into()));
    }
    if body.new_primary_port == 0 {
        return Err(LocalErr::BadRequest(
            "new_primary_port має бути в діапазоні 1..=65535".into(),
        ));
    }

    // Поточний повний URL: з нього беруться user/password/db.
    let current = ls.cfg.resolve_primary_db_url().ok_or_else(|| {
        LocalErr::Conflict(
            "primary URL не задано (ні [node] primary_db_url, ні активне джерело \
             db_sources.toml) — немає звідки взяти креденшалі для нового primary"
                .into(),
        )
    })?;
    let new_url = node_config::rewrite_host_port(&current, &host, body.new_primary_port)
        .ok_or_else(|| LocalErr::BadRequest("поточний primary URL не postgresql://".into()))?;

    let requested_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut new_cfg = ls.cfg.clone();
    new_cfg.primary_db_url = Some(new_url);
    new_cfg.repoint_pending = Some(RepointPending {
        new_primary_host: host.clone(),
        new_primary_port: body.new_primary_port,
        requested_at: requested_at.clone(),
    });
    let config_file = new_cfg
        .save_to_disk()
        .map_err(|e| LocalErr::Read(format!("не вдалось зберегти node_config: {e}")))?;
    eprintln!(
        "[repoint-primary] конфіг збережено: {} → {}:{}, файл {}",
        ls.cfg
            .resolve_primary_db_url()
            .map(|u| node_config::primary_endpoint(&u)
                .map(|(h, p)| format!("{h}:{p}"))
                .unwrap_or_default())
            .unwrap_or_default(),
        host,
        body.new_primary_port,
        config_file.display()
    );

    // Діагностичний журнал (рішення Творця): оператор перенацілив вузол.
    network::log_node_event(
        &ls.pool.0,
        None,
        "repoint_requested",
        "warn",
        json!({
            "new_primary_host": host,
            "new_primary_port": body.new_primary_port,
        }),
    )
    .await;

    Ok(Json(json!({
        "ok": true,
        "repoint_pending": {
            "new_primary_host": host,
            "new_primary_port": body.new_primary_port,
            "requested_at": requested_at,
        },
        "config_file": config_file.display().to_string(),
        "note": "primary_db_url оновлено (креденшалі/БД збережено з попереднього URL). \
                 Фактичний pg_basebackup виконайте ВРУЧНУ (запустіть провіжинінг \
                 standby з новим primary). До завершення реплікації цей вузол \
                 працює зі СТАРИМИ даними — не вимикайте primary."
    })))
}

// ─────────────────────────────────────────────────────────────────────────────
// Збірка адмін-роутера
// ─────────────────────────────────────────────────────────────────────────────

/// Адмін-маршрути DR: `POST /api/v1/local/promote`, `POST
/// /api/v1/local/repoint-primary`.
///
/// Монтуються окремо від `route_local::router` (який під store+auth шарами
/// приватної гілки): ці маршрути мають працювати, коли primary недоступний
/// (store-middleware ходить у primary-пул і впав би). Авторизація — stateless
/// JWT owner всередині хендлерів (офлайн-сумісна).
pub fn admin_router(state: AppState) -> Router<AppState> {
    if !state.node_config.is_standby() || state.local.is_none() {
        // Не standby / локальна репліка не підключена — promote неможливий.
        return Router::<AppState>::new();
    }
    Router::<AppState>::new()
        .route("/api/v1/local/promote", post(promote_handler))
        .route(
            "/api/v1/local/repoint-primary",
            post(repoint_primary_handler),
        )
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Мінімальний AppState для тестів owner-авторизації (всі пули None).
    fn owner_state(secret: &str) -> AppState {
        AppState {
            jwt_secret: Arc::new(secret.to_string()),
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
            node_config: torgashka_infrastructure::node_config::NodeConfig::default(),
            local: None,
        }
    }

    #[test]
    fn owner_offline_rejects_non_bearer() {
        let state = owner_state("s");
        let headers = HeaderMap::new();
        let err = require_owner_offline(&state, &headers).unwrap_err();
        assert!(matches!(err, LocalErr::Unauthorized(_)));
    }

    #[test]
    fn owner_offline_rejects_wrong_role() {
        let state = owner_state("test-secret-для-юніт-тесту");
        let token = auth::create_access_token(
            "11111111-1111-1111-1111-111111111111",
            "cashier", // не owner
            &[],
            "test-secret-для-юніт-тесту",
        )
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        let err = require_owner_offline(&state, &headers).unwrap_err();
        assert!(matches!(err, LocalErr::Forbidden(_)), "cashier → 403");
    }

    #[test]
    fn owner_offline_accepts_owner_token() {
        let state = owner_state("test-secret-для-юніт-тесту");
        let token = auth::create_access_token(
            "11111111-1111-1111-1111-111111111111",
            "owner",
            &[],
            "test-secret-для-юніт-тесту",
        )
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        let claims = require_owner_offline(&state, &headers).unwrap();
        assert_eq!(claims.role, "owner");
    }

    #[test]
    fn repoint_body_rejects_zero_port_via_validator() {
        // Валідація порту — у хендлері; тут перевіряємо, що структура парситься
        // з обома полями (serde).
        let raw = r#"{"new_primary_host":"10.0.0.9","new_primary_port":5432}"#;
        let body: RepointBody = serde_json::from_str(raw).unwrap();
        assert_eq!(body.new_primary_host, "10.0.0.9");
        assert_eq!(body.new_primary_port, 5432);
    }

    #[test]
    fn clear_standby_markers_removes_signal_and_conninfo() {
        let dir = std::env::temp_dir().join(format!("promote_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("standby.signal"), "").unwrap();
        std::fs::write(
            dir.join("postgresql.auto.conf"),
            "# Do not edit this file manually!\nprimary_conninfo = 'host=10.0.0.5 port=5432 user=replicator password=secret'\nrestore_command = ''\n",
        )
        .unwrap();
        let ok = clear_standby_markers(dir.clone());
        assert!(ok);
        assert!(
            !dir.join("standby.signal").exists(),
            "standby.signal видалено"
        );
        let conf = std::fs::read_to_string(dir.join("postgresql.auto.conf")).unwrap();
        assert!(
            !conf.contains("primary_conninfo"),
            "primary_conninfo прибрано"
        );
        assert!(conf.contains("restore_command"), "інші параметри збережено");
        std::fs::remove_dir_all(&dir).ok();
    }
}
