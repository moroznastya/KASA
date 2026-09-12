//! E6-СТАН (ADR-0008 §7.2 п.4): `GET /api/v1/sync/status` — стан синку
//! інстанса: лаг недоставленого, розмір черги форвардингу, відкриті конфлікти
//! спільних довідників, версії довідників.
//!
//! ЧОМУ ЕНДПОІНТ ЛОКАЛЬНИЙ (жодного звернення до хаба): його задача —
//! діагностика, у тому числі КОЛИ ХАБ НЕДОСТУПНИЙ (E4). Статус, який висне на
//! недосяжному апстрімі, для чергового інженера марний; тому «лаг» тут —
//! власна, локально вимірювана величина: **вік найстарішого НЕдоставленого
//! агрегата в `hub_outbox`** (0 = черга порожня, дані поїхали).
//! Версії довідників (`sync_meta`) показують, докуди дійшли дані по кожній
//! сутності — за ними оператор бачить рух, не роблячи мережевих викликів.
//!
//! РЕТЕНШН (`sync_log` / `catalog_change_requests`) — НЕ реалізовано:
//! блокер Б6 (план §6, «термін, партиціювання»). Місце й константа — нижче,
//! щоб рішення Творця вмикалось одним правленням, а не пошуком по коду.
//! Процедура бекапу хаба — також Б6/Б7, не тут.

use axum::{
    extract::{Extension, State},
    Json,
};
use serde_json::{json, Value};

use crate::{sync::SyncError, AppState};

/// Місце рішення Творця (блокер Б6): термін зберігання журналів синку.
/// `None` = політики немає → жодного видалення не робиться (дані не втрачаємо).
/// Коли Творець вирішить — тут з'явиться політика, а фоновий job E6 — чистка.
pub const RETENTION_POLICY: Option<RetentionPolicy> = None;

/// Опис політики ретеншну (не реалізовано — блокер Б6).
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    /// Термін зберігання `sync_log`/`hub_outbox` (днів).
    pub sync_log_days: u32,
    /// Термін зберігання вирішених `catalog_change_requests` (днів;
    /// конфлікти, ймовірно, зберігаються довше — це рішення Творця).
    pub decided_requests_days: u32,
}

/// Причина, чому ретеншн не ввімкнено (видима в API — жодного «німого» TODO).
pub const RETENTION_BLOCKER: &str =
    "ретеншн sync_log/catalog_change_requests потребує рішення Творця (блокер Б6: \
     термін зберігання + партиціювання, план §6)";

/// `GET /api/v1/sync/status` — стан синку (StoreCtx-скоуп: чужі точки не видно).
pub async fn status(
    State(state): State<AppState>,
    Extension(_claims): Extension<crate::auth::Claims>,
) -> Result<Json<Value>, SyncError> {
    let pool = state.store_pool.clone().ok_or(SyncError::Unavailable)?;
    // Явний скоуп точки: без `X-Store-Id` не віддаємо агрегати всієї мережі
    // (RLS тут не рятує — dev-роль postgres має BYPASSRLS, як у `sync::push`).
    let store_id = torgashka_infrastructure::store_ctx::current_store_ctx()
        .map(|c| c.store_id)
        .filter(|s| !s.is_nil())
        .ok_or_else(|| {
            SyncError::BadRequest("потрібен заголовок X-Store-Id (скоуп точки)".to_string())
        })?;

    // Роль інстанса — за налаштуванням У ВЛАСНІЙ БД (те саме джерело, що
    // форвардер E3): `sync.hub_url` є → вузол, немає → хаб/одиночна точка.
    let hub = crate::hub_forwarder::HubForwardConfig::from_pool(&pool).await?;
    let role = if hub.is_some() { "node" } else { "hub" };

    // Черга форвардингу вгору (E3): pending = ще не доїхало до хаба.
    let (pending, failed): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE status = 'pending'), \
                COUNT(*) FILTER (WHERE status = 'failed') \
         FROM hub_outbox WHERE store_id = $1",
    )
    .bind(store_id)
    .fetch_one(&pool)
    .await?;
    let oldest_pending: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT MIN(created_at) FROM hub_outbox WHERE store_id = $1 AND status = 'pending'",
    )
    .bind(store_id)
    .fetch_one(&pool)
    .await?;
    let now = chrono::Utc::now();
    let lag_seconds = oldest_pending
        .map(|t| (now - t).num_seconds().max(0))
        .unwrap_or(0);

    // Відкриті конфлікти спільних довідників (E5): власні пропозиції точки.
    let conflicts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM catalog_change_requests WHERE store_id = $1 AND status = 'conflict'",
    )
    .bind(store_id)
    .fetch_one(&pool)
    .await?;

    // Версії довідників: докуди дійшли дані по кожній сутності.
    let versions: Vec<(String, i64)> =
        sqlx::query_as("SELECT entity, version FROM sync_meta ORDER BY entity")
            .fetch_all(&pool)
            .await?;

    Ok(Json(json!({
        "role": role,
        "hub_url": hub.map(|c| c.base_url),
        "store_id": store_id,
        "lag_seconds": lag_seconds,
        "queue": {
            "pending": pending,
            "failed": failed,
            "oldest_pending_at": oldest_pending,
        },
        "conflicts": { "open": conflicts },
        "versions": versions
            .into_iter()
            .map(|(e, v)| json!({"entity": e, "version": v}))
            .collect::<Vec<_>>(),
        // Ретеншн — свідомо не ввімкнено (блокер Б6): поле видиме, щоб
        // оператор знав, що журнали ростуть без чистки, а не «так задумано».
        "retention": {
            "configured": RETENTION_POLICY.is_some(),
            "sync_log_days": RETENTION_POLICY.map(|p| p.sync_log_days),
            "decided_requests_days": RETENTION_POLICY.map(|p| p.decided_requests_days),
            "blocker": RETENTION_BLOCKER,
        },
    })))
}
