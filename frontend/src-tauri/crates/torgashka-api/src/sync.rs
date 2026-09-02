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
use sqlx::Row;
use serde_json::{json, Value};
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
            SyncError::Unavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"detail": "серверна база недоступна"})))
                    .into_response()
            }
            SyncError::Db(e) => {
                eprintln!("[sync] DB помилка: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"detail": "помилка бази даних"})))
                    .into_response()
            }
        }
    }
}

// ─── Хендлер ────────────────────────────────────────────────────────────────

/// GET /api/v1/sync/master
pub async fn master(
    State(state): State<AppState>,
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

    let pool = state
        .store_pool
        .clone()
        .ok_or(SyncError::Unavailable)?;

    let (changes, to, has_more) = fetch_delta(&pool, &q.entity, since).await?;

    Ok(Json(MasterDelta {
        entity: q.entity.clone(),
        since,
        to,
        has_more,
        changes,
    }))
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
// ЕТАП 4 підтримує ТИЛЬКИ типи, що реально записуються локально касою:
//   receipt (продаж)        → POST /v2/receipts/sale    (PG приймач receipts)
//   return_receipt (повернення) → POST /v2/receipts/return (return_invoices…)
// Інші типи дизайну 2.2 (purchase/inventory/transfer/write_off/cash_operation/
// work_session) локально касою не створюються — TODO ЕТАП 6; сервер відповідає
// per-item error без retry (каса позначить outbox failed — «потребує уваги»).
// ─────────────────────────────────────────────────────────────────────────────

/// Максимум агрегатів на один push-запит (дизайн 4.2: до 50).
pub const PUSH_BATCH_MAX: usize = 50;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PushItemResult {
    fn created(uuid: Uuid, server_id: Uuid) -> Self {
        Self { client_uuid: uuid, status: "created", server_id: Some(server_id), error: None }
    }
    fn already_exists(uuid: Uuid, server_id: Uuid) -> Self {
        Self { client_uuid: uuid, status: "already_exists", server_id: Some(server_id), error: None }
    }
    fn error(uuid: Uuid, msg: impl Into<String>) -> Self {
        Self { client_uuid: uuid, status: "error", server_id: None, error: Some(msg.into()) }
    }
}

/// POST /api/v1/sync/push → 200 (усі результати per-item у тілі).
pub async fn push(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(body): Json<Vec<PushEnvelope>>,
) -> Result<Json<Vec<PushItemResult>>, SyncError> {
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

    let mut results = Vec::with_capacity(body.len());
    for item in &body {
        results.push(
            process_push_item(&svc, &pool, item, cashier, ctx_store).await,
        );
    }
    Ok(Json(results))
}

/// Обробляє ОДИН агрегат: ідемпотентний прийом + sync_log.
async fn process_push_item(
    svc: &torgashka_application::PosServiceFacade<
        std::sync::Arc<dyn torgashka_domain::PosService + Send + Sync>,
    >,
    pool: &StorePool,
    item: &PushEnvelope,
    cashier: Option<Uuid>,
    ctx_store: Uuid,
) -> PushItemResult {
    let hash = payload_hash(item);
    // 1. Валідація типу: ЕТАП 4 підтримує лише чеки каси (дизайн 2.2).
    let receipt_kind = match item.kind.as_str() {
        "receipt" => "sale",
        "return_receipt" => "return",
        other => {
            log_sync(pool, item, ctx_store, "error", &hash, Some(format!(
                "тип '{other}' не підтримується push ЕТАП 4 (TODO ЕТАП 6)"
            )))
            .await;
            return PushItemResult::error(item.client_uuid, format!(
                "тип '{other}' не підтримується push ЕТАП 4"
            ));
        }
    };
    // 2. store_id агрегата має збігатися з точкою запиту (X-Store-Id).
    if item.store_id != ctx_store {
        log_sync(pool, item, ctx_store, "error", &hash, Some(format!(
            "store_id агрегата {} не збігається з точкою запиту",
            item.store_id
        )))
        .await;
        return PushItemResult::error(
            item.client_uuid,
            "store_id агрегата не збігається з X-Store-Id",
        );
    }

    // 3. Дублікат? (звичайний SELECT — гонку ловить UNIQUE 0013 нижче).
    if let Some(existing) = find_by_client_uuid(pool, item.client_uuid).await {
        log_sync(pool, item, ctx_store, "already_exists", &hash, None).await;
        return PushItemResult::already_exists(item.client_uuid, existing);
    }

    // 4. Парсинг payload → вхідні дані чека (та сама валідація, що v2 /sale).
    let mut input = match crate::pos::parse_receipt_create(&item.payload, cashier) {
        Ok(i) => i,
        Err(e) => {
            let msg = pos_err_msg(&e);
            log_sync(pool, item, ctx_store, "error", &hash, Some(msg.clone())).await;
            return PushItemResult::error(item.client_uuid, msg);
        }
    };
    input.client_uuid = Some(item.client_uuid);

    // 5. Створення чека на сервері (існуюча POS-логіка: нумерація, items,
    //    stock-ефекти sale/return). Окрема транзакція на агрегат.
    let created = if receipt_kind == "sale" {
        svc.create_sale_receipt(&input).await
    } else {
        svc.create_return_receipt(&input).await
    };

    match created {
        Ok(dto) => {
            log_sync(pool, item, ctx_store, "ok", &hash, None).await;
            PushItemResult::created(item.client_uuid, dto.id)
        }
        Err(e) => {
            let msg = e.to_string();
            // Гонка: два одночасні push з тим самим client_uuid — UNIQUE
            // uq_receipts_client_uuid (0013) зловив другий атомарно.
            if msg.contains("uq_receipts_client_uuid") {
                if let Some(existing) = find_by_client_uuid(pool, item.client_uuid).await {
                    log_sync(pool, item, ctx_store, "already_exists", &hash, None).await;
                    return PushItemResult::already_exists(item.client_uuid, existing);
                }
            }
            log_sync(pool, item, ctx_store, "error", &hash, Some(msg.clone())).await;
            PushItemResult::error(item.client_uuid, msg)
        }
    }
}

/// SELECT server_id за client_uuid (уже прийнятий чек).
async fn find_by_client_uuid(pool: &StorePool, client_uuid: Uuid) -> Option<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM receipts WHERE client_uuid = $1 LIMIT 1",
    )
    .bind(client_uuid)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Людське повідомлення помилки парсингу (PosErr не реалізує Display).
fn pos_err_msg(e: &crate::pos::PosErr) -> String {
    match e {
        crate::pos::PosErr::Service(pe) => pe.to_string(),
        crate::pos::PosErr::Validation(v) => v.to_string(),
        crate::pos::PosErr::Forbidden(s) | crate::pos::PosErr::Unauthorized(s) => s.clone(),
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
    let res = sqlx::query(
        "INSERT INTO sync_log \
            (store_id, direction, entity, client_uuid, status, payload_hash, error) \
         VALUES ($1, 'push', 'receipt', $2, $3, $4, $5)",
    )
    .bind(store_id)
    .bind(item.client_uuid)
    .bind(status)
    .bind(hash)
    .bind(error)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[sync/push] sync_log запис не вдався: {e}");
    }
}
