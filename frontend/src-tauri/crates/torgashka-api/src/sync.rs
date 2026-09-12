// ─────────────────────────────────────────────────────────────────────────────
// sync — pull майстер-даних (ЕТАП 3 offline-first).
// ─────────────────────────────────────────────────────────────────────────────
// GET /api/v1/sync/master?entity={categories|products|stock_norms|suppliers|
//                                employees|settings}&since_version={int}
//
// Дизайн: docs/design/sync-schema-design.md, розділи 1.4 (дельти), 2.1 (формат).
//
// Механізм версій (a): кожен рядок довідника має `server_version` = значення
// sync_meta.version НА МОМЕНТ зміни (Alembic 0012, BEFORE-тригер). Ендпоінт
// повертає рядки з server_version > since_version — стабільні append-only
// дельти: версії не перепризначаються, повторної видачі версії з іншими
// даними не існує (розділ 1.4).
//
// RLS: категорії та system_settings покриті RLS (0004_rls) — StorePool
// проставляє current_setting('app.store_id') на кожен запит, тому чужі
// точки автоматично відфільтровуються. products/suppliers/users глобальні
// для власника (без store_id) — RLS до них не застосовується.
//
// Пагінація: page_size = 500 rows; запит бере 501 рядок → has_more=true,
// клієнт повторює pull з since_version = `to`.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use torgashka_infrastructure::store_ctx::StorePool;

use crate::AppState;

/// Максимум рядків на одну сторінку дельти (дизайн 1.5).
pub const PAGE_SIZE: i64 = 500;
/// PAGE_SIZE + 1: зайвий рядок сигналізує has_more.
const FETCH_LIMIT: i64 = PAGE_SIZE + 1;

/// Дозволені сутності pull (дизайн 1.2, порядок каси задає клієнт).
pub const ALLOWED_ENTITIES: [&str; 6] = [
    "categories",
    "products",
    "stock_norms",
    "suppliers",
    "employees",
    "settings",
];

// ─── DTO дельти (розділ 2.1) ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MasterDelta {
    pub entity: String,
    pub since: i64,
    pub to: i64,
    pub has_more: bool,
    pub changes: Vec<Change>,
}

#[derive(Debug, Serialize)]
pub struct Change {
    pub op: &'static str,
    pub id: String,
    pub version: i64,
    pub data: Option<Value>,
}

/// Query-параметри ендпоінта.
#[derive(Debug, Deserialize)]
pub struct MasterQuery {
    pub entity: String,
    pub since_version: Option<String>,
}

// ─── Помилки → HTTP ─────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("невалідний запит: {0}")]
    BadRequest(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("серверна база недоступна")]
    Unavailable,
}

impl IntoResponse for SyncError {
    fn into_response(self) -> Response {
        match self {
            SyncError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, Json(json!({"detail": msg}))).into_response()
            }
            SyncError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"detail": "серверна база недоступна"})),
            )
                .into_response(),
            SyncError::Db(e) => {
                eprintln!("[sync] DB помилка: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"detail": "помилка бази даних"})),
                )
                    .into_response()
            }
        }
    }
}

// ─── Хендлер ────────────────────────────────────────────────────────────────

/// GET /api/v1/sync/master
pub async fn master(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Query(q): Query<MasterQuery>,
) -> Result<Json<MasterDelta>, SyncError> {
    // Валідація entity.
    if !ALLOWED_ENTITIES.contains(&q.entity.as_str()) {
        return Err(SyncError::BadRequest(format!(
            "невідома сутність '{}': дозволені {}",
            q.entity,
            ALLOWED_ENTITIES.join(", ")
        )));
    }
    // Валідація since_version (int ≥ 0; відсутній → 0).
    let since = match &q.since_version {
        None => 0i64,
        Some(raw) => raw.trim().parse::<i64>().map_err(|_| {
            SyncError::BadRequest(format!(
                "since_version має бути цілим числом, отримано '{raw}'"
            ))
        })?,
    };
    if since < 0 {
        return Err(SyncError::BadRequest(format!(
            "since_version не може бути від'ємним: {since}"
        )));
    }

    let pool = state.store_pool.clone().ok_or(SyncError::Unavailable)?;

    let (changes, to, has_more) = fetch_delta(&pool, &q.entity, since).await?;

    // Частина 4: device-каса — після УСПІШНОГО pull фіксуємо стан точки в
    // store_sync_state (last_local_seq НЕ чіпаємо). JWT-каси/admin — без змін.
    if claims.role == "device" {
        let store_id = torgashka_infrastructure::store_ctx::current_store_ctx().map(|c| c.store_id);
        if let (Some(store_id), Ok(device_id)) = (store_id, uuid::Uuid::parse_str(&claims.sub)) {
            if let Err(e) = upsert_store_sync_state(&pool, store_id, device_id).await {
                eprintln!("[torgashka-api] sync/master: store_sync_state: {e}");
            }
        }
    }

    Ok(Json(MasterDelta {
        entity: q.entity.clone(),
        since,
        to,
        has_more,
        changes,
    }))
}

/// Частина 4: фіксація стану sync точки після УСПІШНОЇ операції device-каси.
/// Оновлюємо лише last_synced_at/status/device_id; last_local_seq НЕ чіпаємо
/// (його просувають інші механізми). Таблиця БЕЗ RLS — звичайний UPSERT.
async fn upsert_store_sync_state(
    pool: &StorePool,
    store_id: Uuid,
    device_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO store_sync_state (store_id, device_id, last_synced_at, status) \
         VALUES ($1, $2, now(), 'ok') \
         ON CONFLICT (store_id) DO UPDATE \
         SET device_id = EXCLUDED.device_id, last_synced_at = now(), status = 'ok'",
    )
    .bind(store_id)
    .bind(device_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ─── Запити дельти по сутностях ─────────────────────────────────────────────

/// Виконує запит дельти для сутності. Повертає (changes, to, has_more).
async fn fetch_delta(
    pool: &StorePool,
    entity: &str,
    since: i64,
) -> Result<(Vec<Change>, i64, bool), SyncError> {
    // store_id з task-local контексту (StoreContext middleware). Явний фільтр
    // НЕ покладається лише на RLS: dev-роль PostgreSQL (postgres) — superuser
    // з BYPASSRLS, тож політики RLS для неї не діють. Подвійний контур:
    // SQL-фільтр store_id + RLS (0004) для обмежених ролей.
    let store_id = torgashka_infrastructure::store_ctx::current_store_ctx()
        .map(|c| c.store_id)
        .unwrap_or_else(uuid::Uuid::nil);
    let (mut changes, mut has_more) = match entity {
        "categories" => query_categories(pool, since, store_id).await?,
        "products" => query_products(pool, since).await?,
        "suppliers" => query_suppliers(pool, since).await?,
        "employees" => query_employees(pool, since).await?,
        "settings" => query_settings(pool, since, store_id).await?,
        // stock_norms: таблиці в реальній серверній схемі НЕМАЄ (див.
        // Alembic 0011 — зафіксовано аномалією). Дельти завжди порожні,
        // sync_meta.stock_norms лишається 0.
        "stock_norms" => (Vec::new(), false),
        _ => {
            return Err(SyncError::BadRequest(format!(
                "невідома сутність '{entity}'"
            )))
        }
    };

    // Пагінація: зайвий (501-й) рядок не віддаємо, сигналізуємо has_more.
    if changes.len() as i64 > PAGE_SIZE {
        changes.truncate(PAGE_SIZE as usize);
        has_more = true;
    }
    let to = changes.last().map(|c| c.version).unwrap_or(since);
    Ok((changes, to, has_more))
}

/// Будує Change з рядка: op залежить від is_deleted.
fn change(id: Uuid, version: i64, is_deleted: bool, data: Option<Value>) -> Change {
    Change {
        op: if is_deleted { "delete" } else { "upsert" },
        id: id.to_string(),
        version,
        data: if is_deleted { None } else { data },
    }
}

async fn query_categories(
    pool: &StorePool,
    since: i64,
    store_id: uuid::Uuid,
) -> Result<(Vec<Change>, bool), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, description, parent_id, is_deleted, server_version \
         FROM categories \
         WHERE server_version > $1 AND (store_id IS NULL OR store_id = $2) \
         ORDER BY server_version, id LIMIT $3",
    )
    .bind(since)
    .bind(store_id)
    .bind(FETCH_LIMIT)
    .fetch_all(pool)
    .await?;
    let changes = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get(0);
            let name: String = r.get(1);
            let description: Option<String> = r.get(2);
            let parent_id: Option<Uuid> = r.get(3);
            let deleted: bool = r.get(4);
            let version: i64 = r.get(5);
            change(
                id,
                version,
                deleted,
                Some(json!({"name": name, "description": description, "parent_id": parent_id})),
            )
        })
        .collect();
    Ok((changes, false))
}

async fn query_products(pool: &StorePool, since: i64) -> Result<(Vec<Change>, bool), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, barcode, title, price, unit, category_id, is_weight, \
                tax_rate, tax_group, is_deleted, server_version \
         FROM products WHERE server_version > $1 \
         ORDER BY server_version, id LIMIT $2",
    )
    .bind(since)
    .bind(FETCH_LIMIT)
    .fetch_all(pool)
    .await?;
    let changes = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get(0);
            let barcode: Option<String> = r.get(1);
            let title: String = r.get(2);
            let price: Option<bigdecimal::BigDecimal> = r.get(3);
            let unit: Option<String> = r.get(4);
            let category_id: Option<Uuid> = r.get(5);
            let is_weight: bool = r.get(6);
            let tax_rate: Option<bigdecimal::BigDecimal> = r.get(7);
            let tax_group: Option<String> = r.get(8);
            let deleted: bool = r.get(9);
            let version: i64 = r.get(10);
            change(
                id,
                version,
                deleted,
                Some(json!({
                    "name": title,
                    "barcode": barcode,
                    "price": price.map(|p| p.to_string()),
                    "unit": unit,
                    "category_id": category_id,
                    "is_weight": is_weight,
                    "tax_rate": tax_rate.map(|t| t.to_string()),
                    "tax_group": tax_group,
                })),
            )
        })
        .collect();
    Ok((changes, false))
}

async fn query_suppliers(pool: &StorePool, since: i64) -> Result<(Vec<Change>, bool), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, phone, edrpou, is_deleted, server_version \
         FROM suppliers WHERE server_version > $1 \
         ORDER BY server_version, id LIMIT $2",
    )
    .bind(since)
    .bind(FETCH_LIMIT)
    .fetch_all(pool)
    .await?;
    let changes = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get(0);
            let name: String = r.get(1);
            let phone: Option<String> = r.get(2);
            let _edrpou: Option<String> = r.get(3);
            let deleted: bool = r.get(4);
            let version: i64 = r.get(5);
            change(
                id,
                version,
                deleted,
                Some(json!({"name": name, "phone": phone})),
            )
        })
        .collect();
    Ok((changes, false))
}

async fn query_employees(pool: &StorePool, since: i64) -> Result<(Vec<Change>, bool), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, pin_code, role::text, is_deleted, server_version \
         FROM users WHERE server_version > $1 \
         ORDER BY server_version, id LIMIT $2",
    )
    .bind(since)
    .bind(FETCH_LIMIT)
    .fetch_all(pool)
    .await?;
    let changes = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get(0);
            let name: String = r.get(1);
            let pin_code: Option<String> = r.get(2);
            let role: String = r.get(3);
            let deleted: bool = r.get(4);
            let version: i64 = r.get(5);
            // НЕ віддаємо login/password_hash — каса потребує лише PIN-логін.
            change(
                id,
                version,
                deleted,
                Some(json!({"name": name, "pin_hash": pin_code, "role": role})),
            )
        })
        .collect();
    Ok((changes, false))
}

async fn query_settings(
    pool: &StorePool,
    since: i64,
    store_id: uuid::Uuid,
) -> Result<(Vec<Change>, bool), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, key, value, server_version \
         FROM system_settings \
         WHERE server_version > $1 AND store_id = $2 \
         ORDER BY server_version, id LIMIT $3",
    )
    .bind(since)
    .bind(store_id)
    .bind(FETCH_LIMIT)
    .fetch_all(pool)
    .await?;
    // system_settings НЕ має is_deleted (0011 не додавав) → op завжди upsert.
    let changes = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r.get(0);
            let key: String = r.get(1);
            let value: Option<String> = r.get(2);
            let version: i64 = r.get(3);
            change(
                id,
                version,
                false,
                Some(json!({"key": key, "value": value})),
            )
        })
        .collect();
    Ok((changes, false))
}

// ═══════════════════════════════════════════════════════════════════════════
// Push (ЕТАП 4 offline-first): каса → сервер.
// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/sync/push — приймає масив агрегатів (до 50) з outbox каси
// (дизайн 2.2/4.2). Кожен агрегат обробляється ОКРЕМОЮ транзакцією: помилка
// одного не валить решту пакета. Ідемпотентність — через client_uuid каси:
//   * SELECT ... FROM receipts WHERE client_uuid (до створення) →
//     already_exists;
//   * UNIQUE-індекс uq_receipts_client_uuid (Alembic 0013) ловить гонку
//     двох одночасних push з тим самим client_uuid → already_exists.
// Кожен прийом логується в sync_log (direction='push', payload_hash sha256) —
// дизайн 8.2.
//
// Приймач підтримує типи, що реально записуються локально касою (ЕТАП 6/7b
// + ADR-0007 §3.6, §11.6):
//   receipt / return_receipt → POST /v2/receipts/sale|return (через сервіс)
//   purchase_order, inventory, transfer, write_off → SQL-приймачі
//     (`sync_receivers.rs`, кожен зі stock-ефектом в одній транзакції);
//   invoice → сервіс накладних (§9); cash_operation → INSERT у
//     `cash_operations` (§11.6, Alembic 0017).
// Невідомий тип → per-item error без retry (каса позначить outbox failed —
// «потребує уваги»); тихого ack немає.
// ─────────────────────────────────────────────────────────────────────────────

/// Максимум агрегатів на один push-запит (дизайн 4.2: до 50).
pub const PUSH_BATCH_MAX: usize = 50;

/// Машинні класи помилок агрегата (ADR-0008 §4.3, §7.1-E1, етап E2b).
///
/// Клас вирішує долю агрегата на КЛІЄНТІ: `RETRYABLE_FK` → `defer` (агрегат
/// лишається в черзі й повторюється, бо батько ще доїде), решта → незворотний
/// `failed` (потрібне втручання оператора).
pub mod error_class {
    /// ПОВТОРЮВАНО: батьківський агрегат ще не прийнятий (FK / SQLSTATE 23503).
    pub const RETRYABLE_FK: &str = "RETRYABLE_FK";
    /// Незворотно: payload або стан не проходить валідацію.
    pub const VALIDATION: &str = "VALIDATION";
    /// Конфлікт даних (UNIQUE поза ідемпотентним `client_uuid`, SQLSTATE 23505).
    /// Окрема черга конфліктів — етап E5 (§7.1-D1); поки що незворотний.
    pub const CONFLICT: &str = "CONFLICT";
}

/// Заголовок ідентичності батча: клієнт штампує, сервер повертає той самий
/// (ADR-0008 §4.3 п.1 «клієнт штампує батч, сервер повертає його ж у
/// відповіді»). Критерій E3: на хабі `sync_log.batch_id` = батч вузла.
pub const BATCH_ID_HEADER: &str = "x-sync-batch-id";

/// Агрегат з outbox каси (дизайн 2.2).
#[derive(Debug, Deserialize, Serialize)]
pub struct PushEnvelope {
    #[serde(rename = "type")]
    pub kind: String,
    /// Ідемпотентний ключ каси (UUIDv4, генерується при створенні транзакції).
    pub client_uuid: Uuid,
    /// store_id каси; має збігатися з X-Store-Id (StoreCtx).
    pub store_id: Uuid,
    #[serde(default)]
    pub created_at: Option<String>,
    /// Вміст агрегата (для receipt — ReceiptCreate-подібний JSON).
    pub payload: serde_json::Value,
}

/// Результат обробки одного агрегата.
#[derive(Debug, Serialize)]
pub struct PushItemResult {
    pub client_uuid: Uuid,
    /// created | already_exists | error
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<Uuid>,
    /// Машинний клас помилки (ADR-0008 §7.1-E1): `RETRYABLE_FK` | `VALIDATION`
    /// | `CONFLICT`. Відсутній у успішних результатів. `status` лишається
    /// `error` (сумісність з наявними клієнтами й тестами) — саме `error_class`
    /// вирішує, чи це `defer`, чи незворотний `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PushItemResult {
    fn created(uuid: Uuid, server_id: Uuid) -> Self {
        Self {
            client_uuid: uuid,
            status: "created",
            server_id: Some(server_id),
            error_class: None,
            error: None,
        }
    }
    fn already_exists(uuid: Uuid, server_id: Uuid) -> Self {
        Self {
            client_uuid: uuid,
            status: "already_exists",
            server_id: Some(server_id),
            error_class: None,
            error: None,
        }
    }
    /// Помилка агрегата. Клас виводиться з ТІЛА помилки
    /// ([`classify_error_body`]) — щоб 25 місць формування помилки не мали
    /// власної думки про класифікацію (один контракт на всі шляхи).
    fn error(uuid: Uuid, msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            client_uuid: uuid,
            status: "error",
            server_id: None,
            error_class: Some(classify_error_body(&msg)),
            error: Some(msg),
        }
    }
}

/// POST /api/v1/sync/push → 200 (усі результати per-item у тілі).
/// Етап E2a (ADR-0008 §4.3, §7.1-A2): агрегати одного запиту — БАТЧ.
///
/// * `batch_id` — з заголовка [`BATCH_ID_HEADER`] (штамп клієнта: вузол → хаб
///   той самий id) або нова UUIDv4; рядок `sync_batches` створюється ДО
///   обробки з песимістичним `failed`, після — оновлюється фактичним статусом
///   (`accepted`/`partial`/`failed`). Гарантія: `sync_log.batch_id` завжди має
///   свого батька, а пакет, що впав посеред обробки, лишається видимим як
///   невзятий (відрізняється від «загублено»);
/// * `sync_log.batch_id` + `sync_log.error_class` — у кожного агрегата батча;
/// * обробка йде в ТОПОЛОГІЧНОМУ порядку ([`topological_order`] — батьки
///   раніше дітей), відповідь — у порядку запиту (клієнт шукає результат за
///   `client_uuid`, порядок для нього не значущий);
/// * `X-Sync-Batch-Id` у відповіді — той самий `batch_id` (ADR §4.3 п.1).
pub async fn push(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Vec<PushEnvelope>>,
) -> Result<impl axum::response::IntoResponse, SyncError> {
    if body.is_empty() {
        return Err(SyncError::BadRequest("порожній пакет push".to_string()));
    }
    if body.len() > PUSH_BATCH_MAX {
        return Err(SyncError::BadRequest(format!(
            "пакет push перевищує {PUSH_BATCH_MAX} агрегатів: {}",
            body.len()
        )));
    }

    let pool = state.store_pool.clone().ok_or(SyncError::Unavailable)?;
    let repo = state.pos.clone().ok_or_else(|| {
        SyncError::BadRequest("Rust-гілка POS вимкнена — push недоступний".to_string())
    })?;
    let svc = torgashka_application::PosServiceFacade::new(repo);

    let cashier = Uuid::parse_str(&claims.sub).ok();
    // StoreCtx (X-Store-Id, middleware) — явна перевірка store_id агрегата
    // проти контексту точки (не покладаємось лише на RLS — dev-роль
    // postgres має BYPASSRLS, див. master).
    let ctx_store = torgashka_infrastructure::store_ctx::current_store_ctx()
        .map(|c| c.store_id)
        .unwrap_or_else(uuid::Uuid::nil);

    // ── Батч (E2a) ─────────────────────────────────────────────────────────
    let batch_id = headers
        .get(BATCH_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::parse_str(v.trim()).ok())
        .unwrap_or_else(Uuid::new_v4);
    // `node_id` — ідентичність ВУЗЛА. Є лише в device-режимі (claims.sub =
    // device_id, Етап 2b); JWT-каса/admin — це користувач, не вузол → NULL
    // (вигадувати вузол із user_id не можна).
    let node_id = (claims.role == "device")
        .then(|| Uuid::parse_str(&claims.sub).ok())
        .flatten();
    open_batch(&pool, batch_id, ctx_store, node_id, body.len()).await;

    // Топологічний порядок: дитина не мусить іти раніше батька (E2b).
    let order = topological_order(&body);
    let mut slots: Vec<Option<PushItemResult>> = (0..body.len()).map(|_| None).collect();
    with_sync_batch(batch_id, async {
        for idx in order {
            let item = &body[idx];
            slots[idx] = Some(
                process_push_item(
                    &svc,
                    &pool,
                    state.invoices_v1.as_ref(),
                    state.return_invoices.as_ref(),
                    item,
                    cashier,
                    ctx_store,
                )
                .await,
            );
        }
    })
    .await;
    let results: Vec<PushItemResult> = slots.into_iter().flatten().collect();

    // Статус батча — за ФАКТИЧНИМ результатом (жодного «на віру»).
    finalize_batch(&pool, batch_id, batch_status(&results)).await;

    // Частина 4: device-каса — після успішного прийому пакета (усі агрегати
    // отримали результат) фіксуємо стан точки. JWT-каси/admin — без змін.
    if claims.role == "device" {
        if let Ok(device_id) = Uuid::parse_str(&claims.sub) {
            if let Err(e) = upsert_store_sync_state(&pool, ctx_store, device_id).await {
                eprintln!("[torgashka-api] sync/push: store_sync_state: {e}");
            }
        }
    }
    let mut resp_headers = axum::http::HeaderMap::new();
    if let Ok(v) = axum::http::HeaderValue::from_str(&batch_id.to_string()) {
        resp_headers.insert(BATCH_ID_HEADER, v);
    }
    Ok((resp_headers, Json(results)))
}

/// Обробляє ОДИН агрегат: ідемпотентний прийом + sync_log (ЕТАП 7b:
/// чеки + типи каси ЕТАПУ 6 — purchase_order/inventory/transfer/write_off)
/// та прибуткова накладна (invoice, ADR-0007 §3.4 — іде ЧЕРЕЗ СЕРВІС
/// інвойсів, а не через SQL-приймачі sync_receivers).
/// ФАЗА 3.8: `pub(crate)` — drain черги каси після promote застосовує агрегати
/// до ВЛАСНОГО PG ТИМ САМИМ ядром (жодного дублювання логіки apply).
pub(crate) async fn process_push_item(
    svc: &torgashka_application::PosServiceFacade<
        std::sync::Arc<dyn torgashka_domain::PosService + Send + Sync>,
    >,
    pool: &StorePool,
    invoices_v1: Option<&std::sync::Arc<dyn torgashka_domain::InvoicesV1Service + Send + Sync>>,
    return_invoices: Option<
        &std::sync::Arc<dyn torgashka_domain::return_invoices::ReturnInvoicesService + Send + Sync>,
    >,
    item: &PushEnvelope,
    cashier: Option<Uuid>,
    ctx_store: Uuid,
) -> PushItemResult {
    let hash = payload_hash(item);
    // 1. Валідація типу: приймач = таблиця-приймач client_uuid (0013/схема).
    let table = match receiver_table(&item.kind) {
        Some(t) => t,
        None => {
            log_sync(pool, item, ctx_store, "error", &hash, Some(format!(
                "тип '{}' не підтримується push (ЕТАП 7b приймає receipt/return_receipt/purchase_order/inventory/transfer/write_off/cash_operation; ADR-0007 — invoice/return_invoice/debtor_payment/supplier_ledger; ADR-0008 §7.1-B — debtor/work_session/prro_shift)", item.kind
            ))).await;
            return PushItemResult::error(
                item.client_uuid,
                format!("тип '{}' не підтримується push", item.kind),
            );
        }
    };

    // 2. store_id агрегата має збігатися з точкою запиту (X-Store-Id).
    if item.store_id != ctx_store {
        log_sync(
            pool,
            item,
            ctx_store,
            "error",
            &hash,
            Some(format!(
                "store_id агрегата {} не збігається з точкою запиту",
                item.store_id
            )),
        )
        .await;
        return PushItemResult::error(
            item.client_uuid,
            "store_id агрегата не збігається з X-Store-Id",
        );
    }

    // 3. Дублікат? (звичайний SELECT — гонку ловить UNIQUE 0013 нижче).
    if let Some(existing) = find_by_client_uuid_in(pool, table, item.client_uuid).await {
        log_sync(pool, item, ctx_store, "already_exists", &hash, None).await;
        return PushItemResult::already_exists(item.client_uuid, existing);
    }

    // 4. created_at каси (RFC3339, конверт PushEnvelope) → UTC; invalid → None
    //    (сервер напише now()). Не ламає прийом аномальних пакетів.
    let created_at = crate::sync_receivers::parse_created_at_utc(item.created_at.as_deref());

    // 5. Прийом за типом (окрема транзакція на агрегат).
    match item.kind.as_str() {
        "receipt" | "return_receipt" => {
            accept_receipt_kind(
                svc, pool, item, table, cashier, ctx_store, created_at, &hash,
            )
            .await
        }
        // Прибуткова накладна: приймач іде ЧЕРЕЗ СЕРВІС інвойсів (draft →
        // confirm: stock +qty, supplier_ledger, fiscal_stock, price-changes),
        // а не через SQL-приймачі sync_receivers.
        "invoice" => {
            accept_invoice_kind(invoices_v1, pool, item, table, cashier, ctx_store, &hash).await
        }
        // Повернення постачальнику: приймач теж іде ЧЕРЕЗ СЕРВІС (draft →
        // confirm: stock −qty, supplier_ledger, борг) — та сама логіка, що
        // локальний роут, а не друга реалізація SQL-приймача.
        "return_invoice" => {
            accept_return_invoice_kind(
                return_invoices,
                pool,
                item,
                table,
                cashier,
                ctx_store,
                &hash,
            )
            .await
        }
        kind => {
            accept_non_receipt_kind(
                pool, item, table, kind, cashier, ctx_store, created_at, &hash,
            )
            .await
        }
    }
}

// ─── Батч push (E2a): ідентичність пакета ───────────────────────────────────

tokio::task_local! {
    /// `batch_id` поточного push-пакета — property ЗАПИТУ, не агрегата (той
    /// самий підхід, що `store_ctx::StoreCtx`). Поза скоупом (drain черги
    /// каси після promote, `promote.rs`) — `None` → `sync_log.batch_id` NULL.
    static SYNC_BATCH_ID: Uuid;
}

/// Виконати обробку агрегатів у контексті батча.
pub async fn with_sync_batch<T>(batch_id: Uuid, fut: impl std::future::Future<Output = T>) -> T {
    SYNC_BATCH_ID.scope(batch_id, fut).await
}

/// `batch_id` поточного батча (якщо обробка йде в [`with_sync_batch`]).
fn current_sync_batch() -> Option<Uuid> {
    SYNC_BATCH_ID.try_with(|b| *b).ok()
}

/// Створити рядок батча ДО обробки (песимістичний `failed`).
///
/// Свідомо НЕ на «успіх»: якщо процес упаде між INSERT і UPDATE, батч
/// лишиться `failed` — тобто «не взято», а не «взято на віру». Аудит-рядок не
/// є контрактом прийому (як і `log_sync`): помилка INSERT'у не валить push.
async fn open_batch(
    pool: &StorePool,
    batch_id: Uuid,
    store_id: Uuid,
    node_id: Option<Uuid>,
    items: usize,
) {
    let res = sqlx::query(
        "INSERT INTO sync_batches (id, store_id, node_id, items, status) \
         VALUES ($1, $2, $3, $4, 'failed') ON CONFLICT (id) DO NOTHING",
    )
    .bind(batch_id)
    .bind(store_id)
    .bind(node_id)
    .bind(items as i32)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[sync/push] sync_batches INSERT не вдався: {e}");
    }
}

/// Підсумковий статус батча за фактичним результатом агрегатів.
async fn finalize_batch(pool: &StorePool, batch_id: Uuid, status: &str) {
    let res = sqlx::query("UPDATE sync_batches SET status = $2 WHERE id = $1")
        .bind(batch_id)
        .bind(status)
        .execute(pool)
        .await;
    if let Err(e) = res {
        eprintln!("[sync/push] sync_batches UPDATE не вдався: {e}");
    }
}

/// Статус батча за фактичним результатом (E2a):
/// усі прийняті (`created`/`already_exists`) → `accepted`; частина → `partial`;
/// жодного → `failed`.
///
/// Нюанс (свідомо, за специфікацією CHECK — лише 3 значення, ADR §7.1-A2):
/// агрегати, відкладені клієнтом (`error_class = RETRYABLE_FK`), на сервері
/// мають `status = error` → батч із ЛИШЕ такими агрегатами виходить `failed`.
/// Причина видима per-item (`error_class`) і в черзі вузла (агрегат живий,
/// не втрачений) — див. звіт E2b, аномалія «статус батча для deferred».
fn batch_status(results: &[PushItemResult]) -> &'static str {
    let accepted = results.iter().filter(|r| r.status != "error").count();
    if results.is_empty() {
        "failed"
    } else if accepted == results.len() {
        "accepted"
    } else if accepted == 0 {
        "failed"
    } else {
        "partial"
    }
}

// ─── Порядок обробки батча: DAG батько → дитина (E2b) ───────────────────────

/// DAG залежностей батча: kind-ДИТИНА → kind-и-БАТЬКИ, які мусять бути
/// прийняті раніше.
///
/// Джерело — ФАКТИЧНІ FK у PG (`pg_constraint`, перевірено на прод-схемі):
///   `debtor_payments.debtor_id → debtors.id`,
///   `receipts.debtor_id → debtors.id`,
///   `purchase_orders.invoice_id → invoices.id`,
///   `return_invoices.source_invoice_id → invoices.id`.
/// Діти-рядки (`receipt_items`, `*_items`) власного kind НЕ мають — їдуть у
/// конверті батька однією транзакцією (`sync_receivers`: `pool.begin()` …
/// `commit()`), тому в DAG їх немає: окремо вони не подорожують.
///
/// Батьки-ДОВІДНИКИ (`products`, `suppliers`, `users`) тут відсутні свідомо:
/// вони належать хабу (ADR-0008 §5) і kind'а push не мають — дитина з
/// невідомим довідником не стане валідною від очікування (це `VALIDATION`,
/// а не порядок).
fn push_kind_parents(kind: &str) -> &'static [&'static str] {
    match kind {
        "debtor_payment" | "receipt" | "return_receipt" => &["debtor"],
        "purchase_order" | "purchase" | "return_invoice" => &["invoice"],
        _ => &[],
    }
}

/// Глибина залежності kind у DAG (0 — батьків немає). `guard` — захист від
/// циклу в карті залежностей (сьогодні його немає; рекурсія обмежена).
fn kind_depth(kind: &str, guard: usize) -> usize {
    if guard == 0 {
        return 0;
    }
    push_kind_parents(kind)
        .iter()
        .map(|p| 1 + kind_depth(p, guard - 1))
        .max()
        .unwrap_or(0)
}

/// Порядок індексів агрегатів батча для обробки: БАТЬКИ раніше дітей.
///
/// Стабільний: у межах одного рівня зберігається порядок запиту (FIFO каси не
/// порушується — дизайн 4.2). Сортування за `(глибина, індекс запиту)`
/// детерміноване навіть для однакових глибин.
fn topological_order(items: &[PushEnvelope]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    idx.sort_by_key(|&i| (kind_depth(&items[i].kind, 8), i));
    idx
}

/// Таблиця-приймач client_uuid за типом агрегата (дизайн 2.2, ЕТАП 6 типи).
fn receiver_table(kind: &str) -> Option<&'static str> {
    match kind {
        "receipt" | "return_receipt" => Some("receipts"),
        "purchase_order" | "purchase" => Some("purchase_orders"),
        "inventory" => Some("inventories"),
        "transfer" | "transfer_out" | "transfer_in" => Some("transfers"),
        "write_off" => Some("write_offs"),
        // ADR-0007 §3.4: прибуткова накладна каси (partial UNIQUE 0016).
        "invoice" => Some("invoices"),
        // ADR-0007 §11.6: касова операція (внесення/інкасація; partial
        // UNIQUE uq_cash_operations_client_uuid, Alembic 0017).
        "cash_operation" => Some("cash_operations"),
        // ADR-0007 §11.7.9.7 (Фаза 3.3b): повернення ПОСТАЧАЛЬНИКУ (partial
        // UNIQUE 0018), оплата боргу покупця (debtor_payments) і ручний запис
        // книги постачальника (supplier_ledger). НЕ плутати з "return_receipt"
        // (чек повернення ПОКУПЦЯ → receipts).
        "return_invoice" => Some("return_invoices"),
        "debtor_payment" => Some("debtor_payments"),
        "supplier_ledger" => Some("supplier_ledger"),
        // ADR-0008 §7.1-B (етап E1): БАТЬКІВСЬКІ сутності, які створює ВУЗОЛ.
        // Без батька дитина не приймається (`debtor_payment` → FK
        // `debtor_payments.debtor_id` → `debtors.id`); `work_session` вузол уже
        // кладе в outbox (`offline/transactions.rs::open_work_session`), але хаб
        // його не приймав; `prro_shift` — аудит фіскалізації по точках.
        "debtor" => Some("debtors"),
        "work_session" => Some("work_sessions"),
        "prro_shift" => Some("prro_shifts"),
        _ => None,
    }
}

/// Чеки (існуючий шлях svc.create_sale/return_receipt) + created_at з payload.
#[allow(clippy::too_many_arguments)]
async fn accept_receipt_kind(
    svc: &torgashka_application::PosServiceFacade<
        std::sync::Arc<dyn torgashka_domain::PosService + Send + Sync>,
    >,
    pool: &StorePool,
    item: &PushEnvelope,
    table: &str,
    cashier: Option<Uuid>,
    ctx_store: Uuid,
    created_at: Option<chrono::NaiveDateTime>,
    hash: &str,
) -> PushItemResult {
    let receipt_kind = if item.kind == "receipt" {
        "sale"
    } else {
        "return"
    };
    // Парсинг payload → вхідні дані чека (та сама валідація, що v2 /sale).
    let mut input = match crate::pos::parse_receipt_create(&item.payload, cashier) {
        Ok(i) => i,
        Err(e) => {
            let msg = pos_err_msg(&e);
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            return PushItemResult::error(item.client_uuid, msg);
        }
    };
    input.client_uuid = Some(item.client_uuid);
    // ЕТАП 7b (QA §4.3.2): created_at каси, НЕ now() сервера.
    input.created_at = created_at;

    let created = if receipt_kind == "sale" {
        svc.create_sale_receipt(&input).await
    } else {
        svc.create_return_receipt(&input).await
    };

    match created {
        Ok(dto) => {
            log_sync(pool, item, ctx_store, "ok", hash, None).await;
            PushItemResult::created(item.client_uuid, dto.id)
        }
        Err(e) => {
            let msg = e.to_string();
            // Гонка: два одночасні push з тим самим client_uuid — UNIQUE
            // uq_receipts_client_uuid (0013) зловив другий атомарно.
            if msg.contains("uq_receipts_client_uuid") {
                if let Some(existing) = find_by_client_uuid_in(pool, table, item.client_uuid).await
                {
                    log_sync(pool, item, ctx_store, "already_exists", hash, None).await;
                    return PushItemResult::already_exists(item.client_uuid, existing);
                }
            }
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            // Тіло — БЕЗ сирого тексту БД: машинний клас `[DB_ERROR <sqlstate>]`.
            // Сирий текст причини лишається у `sync_log` (рядок вище).
            PushItemResult::error(item.client_uuid, domain_error_body(&e))
        }
    }
}

/// Прибуткова накладна каси (invoice, ADR-0007 §3.4, клас LOCAL_SQLITE).
///
/// Приймач іде ЧЕРЕЗ СЕРВІС [`torgashka_domain::InvoicesV1Service`] (а не
/// через SQL-приймачі sync_receivers), бо накладна має власну бізнес-логіку:
/// `create_v1` пише чернетку + позиції, а stock-ефект (+qty), supplier_ledger
/// і price-changes робить САМЕ `confirm_v1` (Python-еталон v1).
///
/// Ідемпотентність: `client_uuid` каси → `invoices.client_uuid`
/// (partial UNIQUE `uq_invoices_client_uuid`, Alembic 0016) + звичайний
/// SELECT-дублікат вище (крок 3 process_push_item).
///
/// Pre-flight валідація каталогу (постачальник, товари) — обов'язкова:
/// `create_v1` НЕ атомарний (INSERT invoices окремо від insert_items_v1), тож
/// неіснуючий постачальник/товар лишив би «сироту»-чернетку без позицій або
/// з частковою деталізацією. Валідація виконується ДО будь-якого INSERT.
#[allow(clippy::too_many_arguments)]
async fn accept_invoice_kind(
    invoices_v1: Option<&std::sync::Arc<dyn torgashka_domain::InvoicesV1Service + Send + Sync>>,
    pool: &StorePool,
    item: &PushEnvelope,
    table: &str,
    cashier: Option<Uuid>,
    ctx_store: Uuid,
    hash: &str,
) -> PushItemResult {
    // 1. Rust-гілка інвойсів не змонтована (TORGASHKA_RUST_INVOICES≠1) —
    //    НЕ тихий ack: каса має побачити error і лишити оп у outbox.
    let Some(invoices) = invoices_v1 else {
        let msg = format!(
            "Rust-гілка інвойсів вимкнена ({}≠1) — накладну не прийнято",
            crate::RUST_INVOICES_ENV
        );
        log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
        return PushItemResult::error(item.client_uuid, msg);
    };

    // 2. push вимагає JWT sub (created_by_id накладної).
    let Some(cashier) = cashier else {
        let msg = "invoice: push вимагає автентифікованого користувача (JWT sub)".to_string();
        log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
        return PushItemResult::error(item.client_uuid, msg);
    };

    // 3. Парсинг payload каси → вхідні дані v1 (+ ідемпотентний ключ).
    let mut input: torgashka_domain::invoices::InvoiceCreateV1Input =
        match serde_json::from_value(item.payload.clone()) {
            Ok(i) => i,
            Err(e) => {
                let msg = format!("invoice: невалідний payload накладної — {e}");
                log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
                return PushItemResult::error(item.client_uuid, msg);
            }
        };
    input.client_uuid = Some(item.client_uuid);

    // 4. Pre-flight: каталог (постачальник + товари) ПЕРЕД будь-яким INSERT.
    match exists_in(pool, "suppliers", input.supplier_id).await {
        Ok(true) => {}
        Ok(false) => {
            let msg = format!(
                "Постачальника {} не знайдено в каталозі — накладну відхилено",
                input.supplier_id
            );
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            return PushItemResult::error(item.client_uuid, msg);
        }
        Err(e) => {
            log_sync(pool, item, ctx_store, "error", hash, Some(e.to_string())).await;
            return PushItemResult::error(item.client_uuid, db_error_class_of(&e));
        }
    }
    for it in &input.items {
        match exists_in(pool, "products", it.product_id).await {
            Ok(true) => {}
            Ok(false) => {
                let msg = format!(
                    "Товар {} не знайдено в каталозі — накладну відхилено",
                    it.product_id
                );
                log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
                return PushItemResult::error(item.client_uuid, msg);
            }
            Err(e) => {
                log_sync(pool, item, ctx_store, "error", hash, Some(e.to_string())).await;
                return PushItemResult::error(item.client_uuid, db_error_class_of(&e));
            }
        }
    }

    // 5. Чернетка + позиції, далі confirm (stock +qty, ledger) — як v1-роут.
    match invoices.create_v1(&input, cashier).await {
        Ok(dto) => match invoices.confirm_v1(dto.id, "confirmed").await {
            Ok(_) => {
                log_sync(pool, item, ctx_store, "ok", hash, None).await;
                PushItemResult::created(item.client_uuid, dto.id)
            }
            Err(e) => {
                // Чернетка вже в БД (аномалія 2: create_v1 не атомарний).
                // Сирий текст причини → лог (sync_log + stderr).
                let msg = format!("накладну {} створено, але confirm не вдався: {e}", dto.id);
                log_sync(pool, item, ctx_store, "error", hash, Some(msg)).await;
                // Клієнту — людський контекст + машинний клас (без тексту БД).
                PushItemResult::error(
                    item.client_uuid,
                    format!(
                        "накладну {} створено, але confirm не вдався [DB_ERROR]",
                        dto.id
                    ),
                )
            }
        },
        Err(e) => {
            let msg = e.to_string();
            // Гонка: два одночасні push з тим самим client_uuid — partial
            // UNIQUE uq_invoices_client_uuid (0016) зловив другий атомарно.
            if msg.contains("uq_invoices_client_uuid") {
                if let Some(existing) = find_by_client_uuid_in(pool, table, item.client_uuid).await
                {
                    log_sync(pool, item, ctx_store, "already_exists", hash, None).await;
                    return PushItemResult::already_exists(item.client_uuid, existing);
                }
            }
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            // Тіло — БЕЗ сирого тексту БД: машинний клас `[DB_ERROR <sqlstate>]`.
            // Сирий текст причини лишається у `sync_log` (рядок вище).
            PushItemResult::error(item.client_uuid, domain_error_body(&e))
        }
    }
}

/// Приймач повернення ПОСТАЧАЛЬНИКУ (`return_invoice`) — Фаза 3.3b.
///
/// Дзеркало [`accept_invoice_kind`]: документ приймається ЧЕРЕЗ СЕРВІС
/// (`ReturnInvoicesService`), а не SQL-приймачем, щоб на primary діяла та сама
/// бізнес-логіка, що на локальному роуті (draft → confirm: stock −qty,
/// supplier_ledger, борг постачальнику, price-changes). `client_uuid` конверта
/// записується в `return_invoices.client_uuid` → partial UNIQUE 0018 =
/// ідемпотентність (повторний push → `already_exists`).
async fn accept_return_invoice_kind(
    return_invoices: Option<
        &std::sync::Arc<dyn torgashka_domain::return_invoices::ReturnInvoicesService + Send + Sync>,
    >,
    pool: &StorePool,
    item: &PushEnvelope,
    table: &str,
    cashier: Option<Uuid>,
    ctx_store: Uuid,
    hash: &str,
) -> PushItemResult {
    use torgashka_domain::return_invoices::{ReturnInvoiceConfirmInput, ReturnInvoiceCreateInput};

    // 1. Rust-гілка повернень не змонтована (TORGASHKA_RUST_RETURN_INVOICES≠1)
    //    — НЕ тихий ack: каса має побачити error і лишити оп у outbox.
    let Some(svc) = return_invoices else {
        let msg = format!(
            "Rust-гілка повернень постачальнику вимкнена ({}≠1) — повернення не прийнято",
            crate::RUST_RETURN_INVOICES_ENV
        );
        log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
        return PushItemResult::error(item.client_uuid, msg);
    };

    // 2. push вимагає JWT sub (created_by_id документа).
    let Some(cashier) = cashier else {
        let msg =
            "return_invoice: push вимагає автентифікованого користувача (JWT sub)".to_string();
        log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
        return PushItemResult::error(item.client_uuid, msg);
    };

    // 3. Парсинг payload каси → вхідні дані документа (+ ідемпотентний ключ).
    let mut input: ReturnInvoiceCreateInput = match serde_json::from_value(item.payload.clone()) {
        Ok(i) => i,
        Err(e) => {
            let msg = format!("return_invoice: невалідний payload повернення — {e}");
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            return PushItemResult::error(item.client_uuid, msg);
        }
    };
    input.client_uuid = Some(item.client_uuid);

    // 4. Pre-flight: каталог (постачальник + товари) ПЕРЕД будь-яким INSERT.
    match exists_in(pool, "suppliers", input.supplier_id).await {
        Ok(true) => {}
        Ok(false) => {
            let msg = format!(
                "Постачальника {} не знайдено в каталозі — повернення відхилено",
                input.supplier_id
            );
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            return PushItemResult::error(item.client_uuid, msg);
        }
        Err(e) => {
            log_sync(pool, item, ctx_store, "error", hash, Some(e.to_string())).await;
            return PushItemResult::error(item.client_uuid, db_error_class_of(&e));
        }
    }
    for it in &input.items {
        match exists_in(pool, "products", it.product_id).await {
            Ok(true) => {}
            Ok(false) => {
                let msg = format!(
                    "Товар {} не знайдено в каталозі — повернення відхилено",
                    it.product_id
                );
                log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
                return PushItemResult::error(item.client_uuid, msg);
            }
            Err(e) => {
                log_sync(pool, item, ctx_store, "error", hash, Some(e.to_string())).await;
                return PushItemResult::error(item.client_uuid, db_error_class_of(&e));
            }
        }
    }

    // 5. Чернетка + позиції, далі confirm (stock −qty, ledger, борг).
    match svc.create(&input, cashier).await {
        Ok(dto) => {
            let confirm = ReturnInvoiceConfirmInput {
                status: "confirmed".to_string(),
                exchange_items: None,
            };
            match svc.confirm(dto.id, &confirm, cashier).await {
                Ok(_) => {
                    log_sync(pool, item, ctx_store, "ok", hash, None).await;
                    PushItemResult::created(item.client_uuid, dto.id)
                }
                Err(e) => {
                    // Сирий текст причини → лог (sync_log + stderr).
                    let msg = format!("повернення {} створено, але confirm не вдався: {e}", dto.id);
                    log_sync(pool, item, ctx_store, "error", hash, Some(msg)).await;
                    // Клієнту — людський контекст + машинний клас (без тексту БД).
                    PushItemResult::error(
                        item.client_uuid,
                        format!(
                            "повернення {} створено, але confirm не вдався [DB_ERROR]",
                            dto.id
                        ),
                    )
                }
            }
        }
        Err(e) => {
            let msg = e.to_string();
            // Гонка: два одночасні push з тим самим client_uuid — partial
            // UNIQUE uq_return_invoices_client_uuid (0018) зловив другий атомарно.
            if msg.contains("uq_return_invoices_client_uuid") {
                if let Some(existing) = find_by_client_uuid_in(pool, table, item.client_uuid).await
                {
                    log_sync(pool, item, ctx_store, "already_exists", hash, None).await;
                    return PushItemResult::already_exists(item.client_uuid, existing);
                }
            }
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            // Тіло — БЕЗ сирого тексту БД: машинний клас `[DB_ERROR <sqlstate>]`.
            // Сирий текст причини лишається у `sync_log` (рядок вище).
            PushItemResult::error(item.client_uuid, domain_error_body(&e))
        }
    }
}

/// Чи існує рядок каталогу за id (pre-flight валідація перед INSERT).
/// Помилка самої БД — окремо від «немає рядка» (щоб не маскувати збій БД
/// під «немає в каталозі»).
async fn exists_in(pool: &StorePool, table: &str, id: Uuid) -> Result<bool, sqlx::Error> {
    let q = format!("SELECT 1 FROM {table} WHERE id = $1 LIMIT 1");
    sqlx::query_scalar::<_, i32>(&q)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map(|r| r.is_some())
}

/// Не-чекові типи каси ЕТАПУ 6 → SQL-приймачі sync_receivers.
#[allow(clippy::too_many_arguments)]
async fn accept_non_receipt_kind(
    pool: &StorePool,
    item: &PushEnvelope,
    table: &str,
    kind: &str,
    cashier: Option<Uuid>,
    ctx_store: Uuid,
    created_at: Option<chrono::NaiveDateTime>,
    hash: &str,
) -> PushItemResult {
    let Some(cashier) = cashier else {
        let msg = format!("{kind}: push вимагає автентифікованого користувача (JWT sub)");
        log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
        return PushItemResult::error(item.client_uuid, msg);
    };
    let res = match kind {
        "purchase_order" | "purchase" => {
            crate::sync_receivers::accept_purchase_order(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "inventory" => {
            crate::sync_receivers::accept_inventory(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "transfer" | "transfer_out" | "transfer_in" => {
            crate::sync_receivers::accept_transfer(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "write_off" => {
            crate::sync_receivers::accept_write_off(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "cash_operation" => {
            crate::sync_receivers::accept_cash_operation(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "debtor_payment" => {
            crate::sync_receivers::accept_debtor_payment(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "supplier_ledger" => {
            crate::sync_receivers::accept_supplier_ledger(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        // ADR-0008 §7.1-B (E1): батьківські сутності вузла — окремі SQL-приймачі
        // `sync_receivers` (як cash_operation/debtor_payment/supplier_ledger).
        "debtor" => {
            crate::sync_receivers::accept_debtor(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "work_session" => {
            crate::sync_receivers::accept_work_session(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        "prro_shift" => {
            crate::sync_receivers::accept_prro_shift(
                pool,
                ctx_store,
                cashier,
                item.client_uuid,
                created_at,
                &item.payload,
            )
            .await
        }
        _ => unreachable!("receiver_table пропустив тип"),
    };
    match res {
        Ok(server_id) => {
            log_sync(pool, item, ctx_store, "ok", hash, None).await;
            PushItemResult::created(item.client_uuid, server_id)
        }
        Err(e) => {
            let msg = e;
            // Гонка: два одночасні push з тим самим client_uuid — UNIQUE
            // uq_{table}_client_uuid (0013) зловив другий атомарно.
            let uq = format!("uq_{table}_client_uuid");
            if msg.contains(&uq) {
                if let Some(existing) = find_by_client_uuid_in(pool, table, item.client_uuid).await
                {
                    log_sync(pool, item, ctx_store, "already_exists", hash, None).await;
                    return PushItemResult::already_exists(item.client_uuid, existing);
                }
            }
            log_sync(pool, item, ctx_store, "error", hash, Some(msg.clone())).await;
            // Приймачі `sync_receivers` віддають String (тип `sqlx::Error` утрачено):
            // для DB-шляху — стабільний клас, людська валідація — як є.
            PushItemResult::error(item.client_uuid, receiver_error_body(&msg))
        }
    }
}

/// SELECT server_id за client_uuid у таблиці-приймачі (уже прийнятий агрегат).
async fn find_by_client_uuid_in(pool: &StorePool, table: &str, client_uuid: Uuid) -> Option<Uuid> {
    let q = format!("SELECT id FROM {table} WHERE client_uuid = $1 LIMIT 1");
    sqlx::query_scalar::<_, Uuid>(&q)
        .bind(client_uuid)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

/// Стабільний машинний код помилки БД для тіла відповіді (ADR-0007 §D, §3):
/// `[DB_ERROR <sqlstate>]` / `[DB_ERROR]`. Сирий текст PG — лише в лог.
fn db_error_class_of(e: &sqlx::Error) -> String {
    torgashka_infrastructure::readonly_guard::db_error_class(e)
}

/// Той самий код без SQLSTATE (коли на руках лише текст причини: приймачі
/// `sync_receivers` віддають String і тип `sqlx::Error` утрачено).
const DB_ERROR_BODY: &str = "[DB_ERROR]";

/// Доменна помилка, яка може нести сирий текст БД усередині.
trait InfraCarrier {
    /// `true`, якщо Display помилки містить текст PostgreSQL/драйвера.
    fn is_infra(&self) -> bool;
}

impl InfraCarrier for torgashka_domain::PosError {
    fn is_infra(&self) -> bool {
        matches!(
            self,
            torgashka_domain::PosError::Infrastructure(_)
                | torgashka_domain::PosError::Integrity(_)
        )
    }
}

impl InfraCarrier for torgashka_domain::invoices::InvoicesError {
    fn is_infra(&self) -> bool {
        matches!(
            self,
            torgashka_domain::invoices::InvoicesError::Infrastructure(_)
        )
    }
}

impl InfraCarrier for torgashka_domain::return_invoices::ReturnInvoicesError {
    fn is_infra(&self) -> bool {
        matches!(
            self,
            torgashka_domain::return_invoices::ReturnInvoicesError::Infrastructure(_)
        )
    }
}

/// Тіло `error` для доменної помилки сервісу: інфраструктурна (текст БД
/// усередині) → машинний клас; людська (валідація/конфлікт) → як була.
fn domain_error_body<T: InfraCarrier + std::fmt::Display>(e: &T) -> String {
    if e.is_infra() {
        DB_ERROR_BODY.to_string()
    } else {
        e.to_string()
    }
}

/// Чи текст причини приймача є текстом помилки БД (а не валідації payload).
///
/// Префікси — з `sync_receivers`: DB-шлях завжди маркує операцію
/// (`BEGIN`/`INSERT`/`COMMIT`/`номер`/`stock_*`), валідація дає людський текст.
fn receiver_error_is_db(msg: &str) -> bool {
    const DB_PREFIXES: [&str; 9] = [
        "BEGIN:",
        "COMMIT:",
        "INSERT ",
        "UPDATE ",
        "DELETE ",
        "SELECT ",
        "номер ",
        "stock_effect:",
        "stock_set:",
    ];
    DB_PREFIXES.iter().any(|p| msg.starts_with(p))
}

/// Тіло `error` для нечекових приймачів: DB-текст → клас, решта — як була.
///
/// Якщо приймач зберіг SQLSTATE у токені (`sync_receivers::db_err`) — тіло
/// несе його (`[DB_ERROR 23503]`): саме звідти [`classify_error_body`] бере
/// `RETRYABLE_FK`/`CONFLICT`. Без токена — загальний `[DB_ERROR]`.
fn receiver_error_body(msg: &str) -> String {
    if !receiver_error_is_db(msg) {
        return msg.to_string();
    }
    match sqlstate_token(msg) {
        Some(code) => format!("[DB_ERROR {code}]"),
        None => DB_ERROR_BODY.to_string(),
    }
}

/// SQLSTATE із машинного маркера тіла `[DB_ERROR <sqlstate>]`
/// (формат — `readonly_guard::db_error_class`; тут лише читання).
fn sqlstate_from_body(body: &str) -> Option<&str> {
    let rest = body.trim().strip_prefix("[DB_ERROR ")?;
    let code = rest.strip_suffix(']')?.trim();
    (!code.is_empty()).then_some(code)
}

/// SQLSTATE із токена каналу приймачів `[SQLSTATE 23503]`
/// (`sync_receivers::db_err`): типізований `sqlx::Error` губиться на межі
/// `Result<_, String>`, тому код їде поруч із текстом операції.
fn sqlstate_token(body: &str) -> Option<&str> {
    let start = body.rfind("[SQLSTATE ")? + "[SQLSTATE ".len();
    let rest = &body[start..];
    let code = rest[..rest.find(']')?].trim();
    (!code.is_empty()).then_some(code)
}

/// Клас помилки агрегата (ADR-0008 §4.3, §7.1-E1) — ЄДИНЕ місце класифікації:
/// і per-item відповідь клієнту, і `sync_log.error_class`.
///
/// * `RETRYABLE_FK` — FK-батько ще не прийнятий: SQLSTATE `23503` (у тілі
///   `[DB_ERROR 23503]` або токені `[SQLSTATE 23503]`) АБО pre-flight маркер
///   приймача `[MISSING_PARENT]` (той самий FK, перевірений наперед). Клієнт
///   робить `defer` — агрегат повертається в чергу, а не гине назавжди.
/// * `CONFLICT` — `23505` (UNIQUE поза ідемпотентним `client_uuid`).
/// * `VALIDATION` — усе інше (невалідний payload, стан): незворотний `failed`.
///
/// Свідома межа: якщо тіло втратило SQLSTATE (доменні `PosError::Integrity` /
/// `Infrastructure` сервісного шляху маскуються під `[DB_ERROR]` — сирий текст
/// PG у тіло не потрапляє), клас виходить `VALIDATION`. Хибний `failed`
/// безпечніший за хибний `defer` без кінця.
fn classify_error_body(body: &str) -> &'static str {
    if body.contains(crate::sync_receivers::MISSING_PARENT_MARKER) {
        return error_class::RETRYABLE_FK;
    }
    match sqlstate_from_body(body).or_else(|| sqlstate_token(body)) {
        Some("23503") => error_class::RETRYABLE_FK,
        Some("23505") => error_class::CONFLICT,
        _ => error_class::VALIDATION,
    }
}

/// Людське повідомлення помилки парсингу (PosErr не реалізує Display).
fn pos_err_msg(e: &crate::pos::PosErr) -> String {
    match e {
        crate::pos::PosErr::Service(pe) => pe.to_string(),
        crate::pos::PosErr::Validation(v) => v.to_string(),
        crate::pos::PosErr::Forbidden(s)
        | crate::pos::PosErr::Unauthorized(s)
        | crate::pos::PosErr::QueueUndeliverable(s) => s.clone(),
    }
}

/// sha256 (hex) канонічного JSON агрегата — payload_hash sync_log (дизайн 8.2).
fn payload_hash(item: &PushEnvelope) -> String {
    use sha2::Digest;
    let canonical = serde_json::to_string(item).unwrap_or_default();
    let digest = sha2::Sha256::digest(canonical.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// INSERT у sync_log (direction='push'). Помилка логування не впливає на
/// результат прийому (журнал — аудит, не контракт).
async fn log_sync(
    pool: &StorePool,
    item: &PushEnvelope,
    store_id: Uuid,
    status: &str,
    hash: &str,
    error: Option<String>,
) {
    let entity: String = item.kind.chars().take(32).collect();
    // `error_class` — той самий контракт, що в per-item відповіді (E2b):
    // журнал і клієнт бачать ОДИН клас, а не два різні тлумачення.
    let error_class = error.as_deref().map(classify_error_body);
    // `batch_id` — з контексту запиту (E2a): усі агрегати батча мають той самий.
    let batch_id = current_sync_batch();
    let res = sqlx::query(
        "INSERT INTO sync_log \
            (store_id, direction, entity, client_uuid, status, payload_hash, error, \
             error_class, batch_id) \
         VALUES ($1, 'push', $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(store_id)
    .bind(&entity)
    .bind(item.client_uuid)
    .bind(status)
    .bind(hash)
    .bind(error)
    .bind(error_class)
    .bind(batch_id)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[sync/push] sync_log запис не вдався: {e}");
    }
}
