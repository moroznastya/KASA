//! Форвардер node→hub (ADR-0008 §7.1-A1/A3, §7.3 п.1, етап E3).
//!
//! Задача: вузол (повний read-write PG точки) приймає документ каси ЛОКАЛЬНО
//! (`POST /api/v1/sync/push` — шлях «каса → вузол») і мусить передати його
//! ВГОРУ, в хаб мережі — той самий прикладний протокол, роль `node`
//! (§7.2 п.1). Без цього документи точки не доїжджають до хаба-арбітра.
//!
//! Склад (дослівно §7.1-A1 + §7.3 п.1):
//!   * ЧЕРГА `hub_outbox` — те, що вузол прийняв, з payload-конвертом як є
//!     (перегравання з `sync_log` неможливе: там лише `payload_hash`);
//!   * СТАН у журналі прийому `sync_log.hub_forwarded_at/hub_forward_status`
//!     (`accepted` | `failed`) — видно по кожному агрегату;
//!   * FIFO за `id`, backoff і `MAX_ATTEMPTS` — ПОЛІТИКА ПЕРЕВИКОРИСТАНА з
//!     черги каси (`offline/sync_push.rs`: `backoff_delay_secs`,
//!     `MAX_ATTEMPTS`, `PUSH_BATCH_MAX`), а HTTP-виклик — спільна функція
//!     `sync_push::post_push_batch` (URL, авторизація, штамп батча, розбір
//!     відповіді — один код на обидва напрямки).
//!
//! Що вирішує роль інстанса (вузол із хабом vs сам хаб): налаштування
//! `sync.hub_url` у ВЛАСНІЙ БД. У хаба його немає → хаб не форвардить нічого
//! і не накопичує чергу; у вузла — є → прийняте локально стає в чергу
//! ([`enqueue_accepted`]) і їде вгору (задача [`spawn_hub_forward_task`]).
//!
//! **Ґейт Фази 3.8 пройдено ОБХОДОМ, не видаленням.** Форвардер НЕ викликає
//! `sync_push::push_pending_batch*` (єдине місце, де живе
//! `NodeConfig::push_blocked_reason`) — він бере з бібліотеки лише політику
//! повторів і спільний HTTP-виклик, а власну чергу веде в PG. Тому
//! promote-нутий вузол, якому ґейт забороняє слати СТАРУ SQLite-чергу каси на
//! колишній сервер, усе одно віддає прийняте вгору в хаб — саме цього вимагає
//! §1.1 плану. Сам ґейт лишається живим до E7 (його ніхто не чіпав).
//!
//! Ідентифікація вузла (A3): нова сутність НЕ створюється — наявна
//! `network_nodes` покриває роль вузла-клієнта (`store_id`, `name`,
//! `node_token_hash`, `status`, `last_seen_at`); токен для хаба береться з
//! налаштувань вузла (`sync.hub_token`), а не вигадується.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use torgashka_infrastructure::offline::sync_push::{
    backoff_delay_secs, post_push_batch, ServerPushResult, ERROR_CLASS_RETRYABLE_FK, MAX_ATTEMPTS,
    PUSH_BATCH_MAX,
};
use torgashka_infrastructure::store_ctx::StorePool;
use uuid::Uuid;

use crate::sync::{PushEnvelope, PushItemResult};

/// Ключі налаштувань вузла (у ВЛАСНІЙ БД вузла; store-scoped або глобальний —
/// як `system_settings` загалом). Оголошені в `infrastructure::sync_settings`
/// (єдине джерело назв): той самий предикат ролі читає репозиторій
/// користувачів для локального маркера `users.sync_state` (ADR-0008 §7.1-D3).
pub use torgashka_infrastructure::sync_settings::{HUB_TOKEN_SETTING, HUB_URL_SETTING};
/// Період циклу форвардера (сек) і нижня межа — як у черги каси.
pub const DEFAULT_INTERVAL_SECS: u64 = 30;
pub const MIN_INTERVAL_SECS: u64 = 5;
/// Скільки батчів максимум за один цикл (щоб цикл не «залипав» назавжди на
/// довгій черзі — решта поїде наступним тиком).
pub const BATCHES_PER_CYCLE: usize = 10;

/// Куди і від чийого імені вузол віддає документи вгору.
#[derive(Debug, Clone)]
pub struct HubForwardConfig {
    /// Базовий URL хаба (напр. `http://hub.example:8000`).
    pub base_url: String,
    /// Bearer-токен вузла для хаба (JWT хаба або device_token вузла).
    pub token: String,
}

/// Чи цей інстанс — вузол із хабом (є налаштування `sync.hub_url`).
///
/// Помилка читання налаштувань трактується як «немає хаба»: без адреси
/// форвардити нікуди, а кожен push каси не мусить падати через це.
pub async fn hub_configured(pool: &PgPool) -> bool {
    matches!(read_setting(pool, HUB_URL_SETTING).await, Ok(Some(_)))
}

impl HubForwardConfig {
    /// Конфігурація з налаштувань вузла: `None` — хаб не налаштований
    /// (інстанс не форвардить; це нормальний стан хаба/одиночної точки).
    pub async fn from_pool(pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        let Some(base_url) = read_setting(pool, HUB_URL_SETTING).await? else {
            return Ok(None);
        };
        // Токен без URL не має сенсу; URL без токена — конфігурація, яку
        // приймач хаба відкине (401). Чесна відмова замість тихої спроби.
        let token = read_setting(pool, HUB_TOKEN_SETTING)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        Ok(Some(Self { base_url, token }))
    }
}

/// Останнє (за `updated_at`) активне значення налаштування — делегація в
/// `infrastructure::sync_settings` (той самий запит; одне місце правди).
async fn read_setting(pool: &PgPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    torgashka_infrastructure::sync_settings::setting(pool, key).await
}

// ─────────────────────────────────────────────────────────────────────────────
// Постановка в чергу (те, що вузол прийняв локально)
// ─────────────────────────────────────────────────────────────────────────────

/// Поставити в чергу форвардингу агрегати, які вузол ЩОЙНО прийняв.
///
/// Бере лише те, що реально лягло в локальну БД (`created`) або вже там було
/// (`already_exists`): `already_exists` — це повторний push каси АБО перший
/// прохід після ввімкнення форвардера (бекфіл історії), і хаб такого агрегата
/// міг ще не бачити. UNIQUE `(store_id, client_uuid, entity)` робить
/// повторну постановку неможливою.
///
/// Повертає кількість реально поставлених у чергу агрегатів.
pub async fn enqueue_accepted(
    pool: &StorePool,
    store_id: Uuid,
    batch_id: Uuid,
    items: &[PushEnvelope],
    results: &[PushItemResult],
) -> usize {
    // Роль інстанса вирішує НАЛАШТУВАННЯ ЙОГО ВЛАСНОЇ БД: немає `sync.hub_url`
    // (це хаб або одиночна точка) — черга не наповнюється взагалі (нуль сміття
    // й нуль «вічного pending», який нікуди не поїде). Одна перевірка на пакет.
    if items.is_empty() || !hub_configured(&pool.0).await {
        return 0;
    }
    let mut queued = 0usize;
    for (item, res) in items.iter().zip(results.iter()) {
        if !matches!(res.status, "created" | "already_exists") {
            continue;
        }
        // Довжина як у `sync_log.entity` (varchar(32)) — щоб журнал і черга
        // називали агрегат однаково.
        let entity: String = item.kind.chars().take(32).collect();
        let envelope = match serde_json::to_string(item) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[hub_forwarder] конверт агрегата не серіалізовано: {e}");
                continue;
            }
        };
        let res = sqlx::query(
            "INSERT INTO hub_outbox (store_id, entity, client_uuid, batch_id, envelope) \
             VALUES ($1, $2, $3, $4, $5::jsonb) \
             ON CONFLICT (store_id, client_uuid, entity) DO NOTHING",
        )
        .bind(store_id)
        .bind(&entity)
        .bind(item.client_uuid)
        .bind(batch_id)
        .bind(&envelope)
        .execute(pool)
        .await;
        match res {
            Ok(r) => queued += r.rows_affected() as usize,
            Err(e) => eprintln!("[hub_forwarder] постановка в чергу ({entity}): {e}"),
        }
    }
    queued
}

// ─────────────────────────────────────────────────────────────────────────────
// Цикл форвардингу
// ─────────────────────────────────────────────────────────────────────────────

/// Рядок черги, який бере участь у передачі.
type HubRow = (i64, Uuid, String, Uuid, String, i32);

/// Підсумок одного циклу форвардингу (той самий зміст, що `PushSummary` каси).
#[derive(Debug, Clone, Default)]
pub struct ForwardSummary {
    /// Скільки батчів віддано в хаб (запитів).
    pub batches: usize,
    /// Скільки агрегатів у них поїхало.
    pub sent: usize,
    /// Хаб підтвердив (`created` | `already_exists`).
    pub accepted: usize,
    /// Відкладено (мережа/5xx/`RETRYABLE_FK`) — лишається в черзі.
    pub deferred: usize,
    /// Незворотний `failed` (валідація/конфлікт/вичерпані спроби).
    pub failed: usize,
}

/// Один цикл: наступний батч із черги → POST у хаб → обробка per-item.
///
/// Порядок як у каси: FIFO, ≤`PUSH_BATCH_MAX` агрегатів, один цикл = один
/// батч (штамп `batch_id` — той самий, що вузол зберіг у `sync_log`, тож хаб
/// бачить ТОЙ САМИЙ ідентифікатор пакета).
///
/// Мережева помилка (`Err`) — стан черги НЕ змінюється (дизайн 4.3):
/// наступний цикл спробує той самий батч (хаб може бути просто недоступний).
pub async fn forward_pending(
    pool: &StorePool,
    client: &reqwest::Client,
    cfg: &HubForwardConfig,
    max_batches: usize,
) -> Result<ForwardSummary, String> {
    let mut summary = ForwardSummary::default();
    // E9: major-версія схеми ВУЗЛА — з його власної `schema_revision` (фолбек —
    // константа бінарника, якщо рядка/колонки немає). Читаємо один раз на
    // цикл: значення не змінюється під час проходу черги.
    let own_schema_major = torgashka_infrastructure::sync_schema::schema_major(pool).await;
    for _ in 0..max_batches.max(1) {
        // 1. Голова черги: найстаріший pending, який уже дозволено за backoff.
        let head: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
            "SELECT store_id, batch_id FROM hub_outbox \
             WHERE status = 'pending' AND next_attempt_at <= now() \
             ORDER BY id LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("вибір голови черги hub_outbox: {e}"))?;
        let Some((store_id, batch_id)) = head else {
            break;
        };

        // 2. Увесь батч цієї точки — ОДИН запит (як приймає протокол).
        let rows: Vec<HubRow> = sqlx::query_as(
            "SELECT id, store_id, entity, client_uuid, envelope::text, attempts \
             FROM hub_outbox \
             WHERE status = 'pending' AND next_attempt_at <= now() \
               AND store_id = $1 AND batch_id IS NOT DISTINCT FROM $2 \
             ORDER BY id LIMIT $3",
        )
        .bind(store_id)
        .bind(batch_id)
        .bind(PUSH_BATCH_MAX as i64)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("вибір батча hub_outbox: {e}"))?;
        if rows.is_empty() {
            break;
        }

        // Батч із `batch_id IS NULL` — історія до E2a (або drain поза батчем):
        // штампуємо новий, щоб у хаба теж був ідентифікатор пакета.
        let batch_id = batch_id.unwrap_or_else(Uuid::new_v4);
        let body: Vec<Value> = rows
            .iter()
            .map(|r| serde_json::from_str::<Value>(&r.4).unwrap_or(Value::Null))
            .collect();

        summary.batches += 1;
        summary.sent += rows.len();
        let (status, text) = match post_push_batch(
            client,
            &cfg.base_url,
            &cfg.token,
            Some(&store_id.to_string()),
            &batch_id.to_string(),
            own_schema_major,
            &body,
        )
        .await
        {
            Ok(v) => v,
            Err(msg) => return Err(msg),
        };

        // 5xx/429: backoff на ВЕСЬ батч (та сама політика, що в каси).
        if status.is_server_error() || status.as_u16() == 429 {
            for r in &rows {
                defer_or_fail(pool, r, "5xx/429: пакет").await;
            }
            summary.deferred += rows.len();
            continue;
        }
        // 4xx: валідація всього батча — незворотний failed (як у каси).
        if status.is_client_error() {
            let msg = format!("HTTP {status}: {text}");
            for r in &rows {
                mark_failed(pool, r, &msg).await;
            }
            summary.failed += rows.len();
            continue;
        }
        // 2xx: per-item результати хаба.
        let results: Vec<ServerPushResult> = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("невалідна відповідь хаба (HTTP {status}, тіло не JSON): {e}");
                for r in &rows {
                    defer_or_fail(pool, r, "невалідна відповідь хаба").await;
                }
                return Err(msg);
            }
        };
        for r in &rows {
            match results.iter().find(|x| x.client_uuid == r.3.to_string()) {
                // Хаб підтвердив (створено або вже було — ідемпотентність
                // client_uuid): агрегат передано вгору.
                Some(x) if x.status == "created" || x.status == "already_exists" => {
                    mark_accepted(pool, r).await;
                    summary.accepted += 1;
                }
                // Батько ще не прийнятий хабом — та сама політика, що на
                // каса→вузол (E2b): повертаємо в чергу, не ховаємо.
                Some(x) if x.error_class.as_deref() == Some(ERROR_CLASS_RETRYABLE_FK) => {
                    defer_or_fail(pool, r, "RETRYABLE_FK: батько ще не прийнятий хабом").await;
                    summary.deferred += 1;
                }
                // Валідація/конфлікт: повтор нічого не змінить.
                Some(x) => {
                    mark_failed(
                        pool,
                        r,
                        x.error.as_deref().unwrap_or("хаб відкинув агрегат"),
                    )
                    .await;
                    summary.failed += 1;
                }
                // Хаб не відповів за цей агрегат — наступний цикл спробує.
                None => {
                    defer_or_fail(pool, r, "хаб не повернув результат для агрегата").await;
                    summary.deferred += 1;
                }
            }
        }
    }
    Ok(summary)
}

/// Фонова задача форвардера: цикл із періодом `interval_secs` (≥
/// [`MIN_INTERVAL_SECS`]), той самий патерн, що `sync_push::spawn_push_task`.
pub fn spawn_hub_forward_task(
    pool: StorePool,
    cfg: HubForwardConfig,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    let interval = interval_secs.max(MIN_INTERVAL_SECS);
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            let started = std::time::Instant::now();
            match forward_pending(&pool, &client, &cfg, BATCHES_PER_CYCLE).await {
                Ok(s) if s.sent > 0 => eprintln!(
                    "[hub_forwarder] цикл: батчів {}, агрегатів {} за {:.1}с \
                     (accepted {}, deferred {}, failed {})",
                    s.batches,
                    s.sent,
                    started.elapsed().as_secs_f64(),
                    s.accepted,
                    s.deferred,
                    s.failed
                ),
                Ok(_) => {}
                Err(e) => eprintln!("[hub_forwarder] цикл: помилка: {e}"),
            }
        }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Зміни стану черги + журналу прийому
// ─────────────────────────────────────────────────────────────────────────────

/// Хаб підтвердив агрегат: черга → `done`, `sync_log` → `accepted`.
async fn mark_accepted(pool: &StorePool, r: &HubRow) {
    let res = sqlx::query(
        "UPDATE hub_outbox SET status = 'done', forward_status = 'accepted', \
         forwarded_at = now(), attempts = attempts + 1, error = NULL WHERE id = $1",
    )
    .bind(r.0)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[hub_forwarder] черга → accepted (id={}): {e}", r.0);
    }
    mark_sync_log(pool, r, "accepted").await;
}

/// Незворотна відмова: черга → `failed`, `sync_log` → `failed`.
///
/// `hub_forwarded_at` виставляється і тут: спроба передачі ЗАВЕРШЕНА (хаб
/// відповів остаточно), тож агрегат не мусить висіти у черзі моніторингу
/// «ще не передано» — проблема видима як `hub_forward_status='failed'`.
async fn mark_failed(pool: &StorePool, r: &HubRow, error: &str) {
    let res = sqlx::query(
        "UPDATE hub_outbox SET status = 'failed', forward_status = 'failed', \
         forwarded_at = now(), attempts = attempts + 1, error = $2 WHERE id = $1",
    )
    .bind(r.0)
    .bind(error)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[hub_forwarder] черга → failed (id={}): {e}", r.0);
    }
    mark_sync_log(pool, r, "failed").await;
}

/// Відкласти: `attempts += 1`, `next_attempt_at = now + backoff`; після
/// `MAX_ATTEMPTS` невдач — видимий `failed` (та сама політика, що в каси).
/// Рядок лишається в черзі (`pending`), `sync_log` НЕ чіпаємо (він і далі
/// «ще не передано» — саме це показує моніторинг E6).
async fn defer_or_fail(pool: &StorePool, r: &HubRow, reason: &str) {
    let next_attempts = i64::from(r.5) + 1;
    if next_attempts >= MAX_ATTEMPTS {
        let msg = format!("{next_attempts} невдалих спроб ({reason})");
        mark_failed(pool, r, &msg).await;
        return;
    }
    let delay = backoff_delay_secs(next_attempts);
    let next_at: DateTime<Utc> = Utc::now() + chrono::Duration::seconds(delay);
    let res = sqlx::query(
        "UPDATE hub_outbox SET attempts = $2, next_attempt_at = $3, error = $4 WHERE id = $1",
    )
    .bind(r.0)
    .bind(next_attempts as i32)
    .bind(next_at)
    .bind(reason)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[hub_forwarder] черга → backoff (id={}): {e}", r.0);
    }
}

/// Відмітка в журналі прийому: агрегат передано вгору (`accepted` | `failed`).
async fn mark_sync_log(pool: &StorePool, r: &HubRow, status: &str) {
    let res = sqlx::query(
        "UPDATE sync_log SET hub_forwarded_at = now(), hub_forward_status = $4 \
         WHERE store_id = $1 AND client_uuid = $2 AND entity = $3 \
           AND direction = 'push' AND hub_forwarded_at IS NULL",
    )
    .bind(r.1)
    .bind(r.3)
    .bind(&r.2)
    .bind(status)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[hub_forwarder] стан форвардингу в sync_log: {e}");
    }
}
