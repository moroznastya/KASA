//! E5-ЯДРО (ADR-0008 §4.2 «hub-as-authority», §7.1-D1/D2, §7.2 п.3/п.5):
//! арбітраж СПІЛЬНИХ довідників через протокол ПРОПОЗИЦІЙ.
//!
//! Чому окремий ендпоінт, а не kind `catalog_proposal` у `POST /api/v1/sync/push`
//! (ADR §7.2 п.3 дає обидві форми; обрано першу):
//!   * `push` приймає ОПЕРАЦІЙНІ документи («це сталося — прийми»), його
//!     прийняте стає чергою форвардингу вгору (`hub_outbox`, E3) і журналом
//!     фактів `sync_log`. Пропозиція довідника — НЕ факт, а ПРОХАННЯ до
//!     авторитета: її може бути відхилено/відкладено як конфлікт, а хаб НЕ
//!     мусить передавати її далі (він і є авторитет) — kind у `push` тягнув би
//!     псевдо-факт у журнал і в чергу вгору;
//!   * мова відповіді інша: `PushItemResult` (created/already_exists/error) не
//!     виражає `accepted`/`conflict`/`rejected` без перевантаження семантики,
//!     а вузол мусить дізнатись ПРИСВОЄНИЙ хабом `server_version`;
//!   * ідемпотентність тут своя: `client_uuid` пропозиції UNIQUE (повтор
//!     мережевого запиту повертає РАНІШЕ рішення, не створює другої правки).
//!
//! ОБСЯГ арбітражу (усі — СПІЛЬНІ сутності без винятку за точкою):
//!   * `products`, `suppliers` — ADR §4.2 (E5-ядро, 0022);
//!   * `store_product_prices` — рішення Творця Б1 (2026-09-12, ADR §10 №1):
//!     ціна точки — атрибут МЕРЕЖІ, шлях той самий, що `products.price`;
//!   * `users` — рішення Творця Б2 (2026-09-16, ADR §10 №2, варіант A):
//!     касир створюється ЛОКАЛЬНО на вузлі (offline-first, ADR §2.2) з
//!     локальним маркером `sync_state='pending_hub'` (§7.1-D3), а канонічним
//!     стає після прийняття пропозиції хабом (`sync_state='confirmed'`).
//!
//! НЕ додані (потребують рішення Творця, план §6):
//!   * `print_templates` — блокер C4 (каталог хаба чи локальний оверрайд);
//!   * `barcodes`/`product_images`/`write_off_reasons`/`system_settings` —
//!     C1/C2/C5: придатні для pull (0023), але власних пропозицій поки не
//!     приймають (розширення = рядок у `ARBITRATED_ENTITIES` + `apply_*`).
//!
//! Окремого push-kind для цін НЕМАЄ і не буде: `push` приймає ОПЕРАЦІЙНІ
//! факти («це сталося — прийми»), а спільна сутність вимагає АРБІТРАЖУ
//! (хаб присвоює ЄДИНИЙ `server_version`, дві правки на один рядок стають
//! видимим конфліктом). Двері `push` = last-write-wins в обхід арбітражу →
//! мовчазна втрата правки іншого вузла (ADR §4.2 прямо забороняє).
//!
//! Розширення = один рядок у `ARBITRATED_ENTITIES` + `apply_*` (явний SQL).

use std::str::FromStr;

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{auth_routes, sync::PUSH_BATCH_MAX, AppState};

// ─── Помилки → HTTP ({"detail": msg}, як решта модулів фасаду) ───────────────

#[derive(Debug)]
pub enum CatalogErr {
    BadRequest(String),
    /// БД фасаду не змонтована (write_pool=None).
    Unavailable,
    Db(sqlx::Error),
    /// require_admin (auth_routes) — 401/403/404 як у auth-гілці.
    Auth(auth_routes::AuthRouteError),
}

impl IntoResponse for CatalogErr {
    fn into_response(self) -> Response {
        match self {
            CatalogErr::BadRequest(m) => {
                (StatusCode::BAD_REQUEST, Json(json!({"detail": m}))).into_response()
            }
            CatalogErr::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"detail": "серверна база недоступна"})),
            )
                .into_response(),
            CatalogErr::Db(e) => {
                eprintln!("[catalog-proposal] помилка БД: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"detail": "помилка бази даних"})),
                )
                    .into_response()
            }
            CatalogErr::Auth(e) => e.into_response(),
        }
    }
}

impl From<sqlx::Error> for CatalogErr {
    fn from(e: sqlx::Error) -> Self {
        CatalogErr::Db(e)
    }
}

impl From<auth_routes::AuthRouteError> for CatalogErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        CatalogErr::Auth(e)
    }
}

// ─── Довідники під арбітражем (хаб — авторитет) ─────────────────────────────

/// Спільні довідники, для яких політика ADR §4.2 визначена однозначно.
/// ⚠ Розширення списку = рішення Творця (див. шапку: C4 `print_templates` —
/// єдиний незакритий блокер E5).
pub const ARBITRATED_ENTITIES: [&str; 4] =
    ["products", "suppliers", "store_product_prices", "users"];

/// Ключі payload-конверта: ТІ САМІ, що віддає pull-дельта
/// (`sync.rs::query_products`/`query_suppliers`), щоб вузол міг побудувати
/// пропозицію зі свого рядка без окремого мапінгу (симетрія pull↔proposal).
const PRODUCT_KEYS: [&str; 9] = [
    "name",
    "title",
    "barcode",
    "price",
    "unit",
    "tax_rate",
    "is_weight",
    "category_id",
    "tax_group",
];
const SUPPLIER_KEYS: [&str; 3] = ["name", "phone", "edrpou"];

/// Б5/C3: ціна точки. Природний ключ рядка — `(store_id, product_id)` (UNIQUE
/// у схемі), тому обидва поля входять у конверт: за ними хаб знаходить
/// КАНОНІЧНИЙ рядок, якщо вузол пропонує свій локальний uuid (див.
/// `apply_store_product_prices`).
const STORE_PRICE_KEYS: [&str; 3] = ["store_id", "product_id", "price"];

/// Б2/D3: касир. `name`/`pin_hash`/`role` — ТІ САМІ ключі, що віддає pull-дельта
/// `employees` (`sync.rs::query_employees`), тому вузол будує пропозицію прямо
/// зі свого рядка. `login`/`password_hash` додані свідомо: у схемі `users` вони
/// NOT NULL, і без них хаб не може створити канонічний рядок (вигадувати
/// логін/хеш за вузол — не наша справа). `pin_hash` — саме хеш (як у pull).
const USER_KEYS: [&str; 6] = [
    "name",
    "login",
    "password_hash",
    "pin_hash",
    "role",
    "is_active",
];

/// Ролі, які вузол може запропонувати — валідація ДО SQL (невідома роль дає
/// зрозумілу відмову, а не сирий текст приведення типу).
///
/// ⚠ `owner` СВІДОМО відсутній, хоч і є в enum `user_role`: політику задає
/// наявний шлях створення (`auth_routes::parse_role` — «owner лише через
/// setup/БД»), і пропозиція з вузла не має її обходити (ескалація привілеїв).
/// Тому набір = {admin, cashier, store_manager}, як у API.
const USER_ROLES: [&str; 3] = ["admin", "cashier", "store_manager"];

// ─── DTO ────────────────────────────────────────────────────────────────────

/// Пропозиція зміни спільного довідника (конверт вузла).
#[derive(Debug, Deserialize)]
pub struct CatalogProposal {
    /// `products` | `suppliers` (див. `ARBITRATED_ENTITIES`).
    pub entity: String,
    /// Рядок довідника, якого стосується правка (id на обох боках той самий).
    pub row_id: Uuid,
    /// `upsert` | `delete`.
    pub op: String,
    /// Дані зміни — конверт `Change.data` (як у pull-дельті).
    #[serde(default)]
    pub payload: Value,
    /// Ідемпотентність пропозиції (повтор → раніше рішення хабa).
    pub client_uuid: Uuid,
    /// Точка-автор правки (мусить збігатись із StoreCtx, як у push).
    pub store_id: Option<Uuid>,
    /// Версія рядка, яку автор БАЧИВ (0 = «новий рядок»). Ключ детермінованого
    /// правила ADR §4.2 п.5: вища версія >> час створення.
    #[serde(default)]
    pub base_version: i64,
    /// Явний пріоритет правки (ADR §4.2 п.5: «явний пріоритет >> час»).
    #[serde(default)]
    pub priority: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ProposalResult {
    pub entity: String,
    pub row_id: Uuid,
    pub client_uuid: Uuid,
    /// `accepted` | `conflict` | `rejected`.
    pub status: String,
    /// Версія, присвоєна ХАБОМ (єдине джерело канонічної версії); NULL для
    /// conflict/rejected.
    pub server_version: Option<i64>,
    /// Причина для rejected (і пояснення для conflict-участі).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ─── Хендлер: POST /api/v1/sync/catalog-proposal ─────────────────────────────

/// Прийом пакета пропозицій від вузла. Кожна пропозиція — ОКРЕМА транзакція:
/// відмова однієї не валить решту (та сама політика, що в `sync::push`).
pub async fn propose(
    State(state): State<AppState>,
    Extension(_claims): Extension<crate::auth::Claims>,
    Json(body): Json<Vec<CatalogProposal>>,
) -> Result<Json<Value>, CatalogErr> {
    if body.is_empty() {
        return Err(CatalogErr::BadRequest(
            "порожній пакет пропозицій".to_string(),
        ));
    }
    if body.len() > PUSH_BATCH_MAX {
        return Err(CatalogErr::BadRequest(format!(
            "пакет пропозицій перевищує {PUSH_BATCH_MAX}: {}",
            body.len()
        )));
    }
    let pool = state.store_pool.clone().ok_or(CatalogErr::Unavailable)?;
    // Явна перевірка store_id пропозиції проти контексту точки (не покладаємось
    // лише на RLS — dev-роль postgres має BYPASSRLS, як у `sync::push`).
    let ctx_store = torgashka_infrastructure::store_ctx::current_store_ctx()
        .map(|c| c.store_id)
        .unwrap_or(Uuid::nil());

    let mut results = Vec::with_capacity(body.len());
    for item in &body {
        results.push(decide_one(&pool, ctx_store, item).await?);
    }
    let count = |s: &str| results.iter().filter(|r| r.status == s).count();
    let (accepted, conflicts, rejected) = (count("accepted"), count("conflict"), count("rejected"));
    Ok(Json(json!({
        "results": results,
        "accepted": accepted,
        "conflicts": conflicts,
        "rejected": rejected,
    })))
}

/// Рішення по одній пропозиції: журнал → конфлікт → застосування на хабі.
///
/// Порядок навмисний: пропозиція СПОЧАТКУ лягає в журнал (`pending`), і лише
/// потім хаб вирішує. Якщо застосування впало — пропозиція лишається
/// `pending`/`rejected` з причиною, тобто жодна правка вузла не зникає.
async fn decide_one(
    pool: &PgPool,
    ctx_store: Uuid,
    item: &CatalogProposal,
) -> Result<ProposalResult, CatalogErr> {
    let entity = item.entity.trim();
    let client_uuid = item.client_uuid;
    let row_id = item.row_id;
    let base = |status: &str, version: Option<i64>, note: Option<String>| ProposalResult {
        entity: entity.to_string(),
        row_id,
        client_uuid,
        status: status.to_string(),
        server_version: version,
        note,
    };

    // ── 1. Валідація ЗАПИТУ (per-item відмова, не 400 на весь пакет) ──────
    if !ARBITRATED_ENTITIES.contains(&entity) {
        return Ok(base(
            "rejected",
            None,
            Some(format!(
                "довідник '{entity}' не під арбітражем E5 (дозволено: {}); \
                 розширення — рішення Творця (план §6)",
                ARBITRATED_ENTITIES.join(", ")
            )),
        ));
    }
    let op = item.op.trim().to_lowercase();
    if op != "upsert" && op != "delete" {
        return Ok(base(
            "rejected",
            None,
            Some(format!("невідома op '{op}' (дозволено upsert/delete)")),
        ));
    }
    if item
        .store_id
        .is_some_and(|s| s != ctx_store && !ctx_store.is_nil())
    {
        return Ok(base(
            "rejected",
            None,
            Some("store_id пропозиції не збігається з контекстом точки".to_string()),
        ));
    }
    let store_id = item.store_id.unwrap_or(ctx_store);
    if op == "upsert" && !item.payload.is_object() {
        return Ok(base(
            "rejected",
            None,
            Some("upsert без payload-об'єкта".to_string()),
        ));
    }
    // Поля поза арбітрованою поверхнею — ВІДМОВА (не тихе відкидання: інакше
    // вузол вважав би, що надіслав правку, а частина її зникла б).
    if op == "upsert" {
        let allowed = allowed_keys(entity);
        let unknown: Vec<&str> = item
            .payload
            .as_object()
            .map(|o| {
                o.keys()
                    .map(String::as_str)
                    .filter(|k| !allowed.contains(k))
                    .collect()
            })
            .unwrap_or_default();
        if !unknown.is_empty() {
            return Ok(base(
                "rejected",
                None,
                Some(format!(
                    "поля поза арбітрованою поверхнею {entity}: {unknown:?}; дозволено {allowed:?}"
                )),
            ));
        }
    }

    // ── 2. Транзакція: журнал → конфлікт → застосування ───────────────────
    let mut tx = pool.begin().await?;

    // 2.1. Журнал. UNIQUE(client_uuid) = ідемпотентність повторної доставки.
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO catalog_change_requests \
            (entity, row_id, op, payload, client_uuid, store_id, status, base_version, priority) \
         VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7, $8) \
         ON CONFLICT (client_uuid) DO NOTHING \
         RETURNING id",
    )
    .bind(entity)
    .bind(row_id)
    .bind(&op)
    .bind(&item.payload)
    .bind(client_uuid)
    .bind(store_id)
    .bind(item.base_version)
    .bind(item.priority)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(id) = inserted else {
        // Повторна доставка вже відомої пропозиції → РАНІШЕ рішення хабa
        // (без другого застосування — жодних подвійних правок).
        let (status, version): (String, Option<i64>) = sqlx::query_as(
            "SELECT status, server_version FROM catalog_change_requests WHERE client_uuid = $1",
        )
        .bind(client_uuid)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(base(
            &status,
            version,
            Some("повторна доставка".to_string()),
        ));
    };

    // 2.2. Конкуренти: інша НЕвирішена/застосована пропозиція на ТОЙ САМИЙ рядок.
    //      ADR §4.2 п.5: конфлікт ВИДИМИЙ, а не «злитий мовчки» → обидві
    //      пропозиції лишаються в журналі зі статусом `conflict`, до рядка
    //      довідника НЕ застосовується нічого (вирішує оператор).
    let competitors: Vec<(i64, Uuid)> = sqlx::query_as(
        "SELECT id, client_uuid FROM catalog_change_requests \
         WHERE entity = $1 AND row_id = $2 AND id <> $3 AND client_uuid <> $4 \
           AND status IN ('pending','accepted') \
         ORDER BY server_version DESC NULLS LAST, base_version DESC, \
                  priority DESC NULLS LAST, created_at ASC, id ASC \
         LIMIT 1",
    )
    .bind(entity)
    .bind(row_id)
    .bind(id)
    .bind(client_uuid)
    .fetch_all(&mut *tx)
    .await?;

    if let Some((competitor_id, competitor_cu)) = competitors.first().copied() {
        sqlx::query(
            "UPDATE catalog_change_requests SET status = 'conflict', decided_at = now() \
             WHERE entity = $1 AND row_id = $2 AND status IN ('pending','accepted')",
        )
        .bind(entity)
        .bind(row_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        eprintln!(
            "[catalog-proposal] КОНФЛІКТ {entity}/{row_id}: пропозиції {competitor_cu} і \
             {client_uuid} лишаються в журналі (status=conflict, id={competitor_id}/{id})"
        );
        return Ok(base(
            "conflict",
            None,
            Some(format!(
                "конфлікт із пропозицією {competitor_cu} на той самий рядок: \
                 обидві в журналі, до довідника не застосовано нічого \
                 (вирішує оператор — /api/v1/admin/sync/conflicts)"
            )),
        ));
    }

    // 2.3. Застосування до довідника на ХАБІ (авторитет). Єдиний
    //      `server_version` присвоює наявний BEFORE-тригер `bump_sync_version`
    //      (`trg_<table>_bump`, Alembic 0012) — жодного власного лічильника.
    //      SAVEPOINT: невдале застосування (валідація/конфлікт унікальності) не
    //      має «вбивати» транзакцію — журнал мусить доїхати з причиною.
    sqlx::query("SAVEPOINT proposal_apply")
        .execute(&mut *tx)
        .await?;
    match apply_proposal(&mut tx, entity, row_id, &op, &item.payload).await {
        Ok(applied) => {
            sqlx::query("RELEASE SAVEPOINT proposal_apply")
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "UPDATE catalog_change_requests \
                 SET status = 'accepted', server_version = $2, decided_at = now() \
                 WHERE id = $1",
            )
            .bind(id)
            .bind(applied.version)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            if let Some(v) = applied.version {
                eprintln!(
                    "[catalog-proposal] ПРИЙНЯТО {entity}/{row_id} op={op} → server_version={v} \
                     (роздача вузлам — наявним pull: server_version > since_version)"
                );
            }
            Ok(base("accepted", applied.version, applied.note))
        }
        Err(reason) => {
            // Застосувати не вдалось (валідація payload/рядка) — відкочуємо
            // ЛИШЕ застосування, а пропозиція лишається в журналі як rejected
            // З ПРИЧИНОЮ (не зникає; `current transaction is aborted` неможливий).
            sqlx::query("ROLLBACK TO SAVEPOINT proposal_apply")
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "UPDATE catalog_change_requests \
                 SET status = 'rejected', decided_at = now(), error = $2 \
                 WHERE id = $1",
            )
            .bind(id)
            .bind(&reason)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(base("rejected", None, Some(reason)))
        }
    }
}

/// Застосувати прийняту пропозицію до довідника на хабі.
///
/// `Ok(Some(v))` — рядок змінено, хаб присвоїв версію `v`;
/// `Ok(None)` — змінювати нічого (рядка немає — tombstone не потрібен);
/// `Err(reason)` — валідація payload/операції не пройдена (→ `rejected`).
async fn apply_proposal(
    tx: &mut PgConnection,
    entity: &str,
    row_id: Uuid,
    op: &str,
    payload: &Value,
) -> Result<Applied, String> {
    match entity {
        "products" => apply_products(tx, row_id, op, payload)
            .await
            .map(Applied::version),
        "suppliers" => apply_suppliers(tx, row_id, op, payload)
            .await
            .map(Applied::version),
        // E5-B5/C3: ціна точки — спільна сутність мережі (рішення Б1).
        "store_product_prices" => apply_store_product_prices(tx, row_id, op, payload).await,
        // E5-B2/D3: касир, створений локально на вузлі (рішення Б2).
        "users" => apply_users(tx, row_id, op, payload).await,
        other => Err(format!("довідник '{other}' не під арбітражем")),
    }
}

/// Результат застосування пропозиції до довідника на хабі.
///
/// `note` — необов'язкове пояснення для вузла-автора (те, що він мусить знати,
/// але що не влізає у `version`): напр. канонікалізація рядка за природним
/// ключем (`store_product_prices`) — вузол дізнається, що хаб застосував правку
/// до ІНШОГО рядка, і не вважатиме свій uuid канонічним.
struct Applied {
    version: Option<i64>,
    note: Option<String>,
}

impl Applied {
    fn version(version: Option<i64>) -> Self {
        Self {
            version,
            note: None,
        }
    }
}

async fn apply_products(
    tx: &mut PgConnection,
    row_id: Uuid,
    op: &str,
    payload: &Value,
) -> Result<Option<i64>, String> {
    if op == "delete" {
        // Tombstone (механізм уже є, ADR §4.2 п.3) + bump версії тригером.
        let version: Option<i64> = sqlx::query_scalar(
            "UPDATE products SET is_deleted = true, updated_at = now() \
             WHERE id = $1 RETURNING server_version",
        )
        .bind(row_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("tombstone products: {e}"))?;
        // Рядка на хабі немає → tombstone не потрібен (видалення локальне).
        // Чесна відмова з причиною: «тихо accepted» збрехало б вузлу.
        return match version {
            Some(v) => Ok(Some(v)),
            None => Err("рядка немає на хабі — tombstone не потрібен".to_string()),
        };
    }

    let title = opt_str(payload, &["name", "title"]);
    let barcode = opt_str(payload, &["barcode"]);
    let price = opt_decimal(payload, "price")?;
    let unit = opt_str(payload, &["unit"]);
    let tax_rate = opt_decimal(payload, "tax_rate")?;
    let is_weight = opt_bool(payload, "is_weight")?;
    let category_id = opt_uuid(payload, "category_id")?;
    let tax_group = opt_str(payload, &["tax_group"]);

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE id = $1)")
        .bind(row_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("перевірка products: {e}"))?;

    if exists {
        // Часткове злиття: NULL у параметрі = «поле не надіслали → не чіпати».
        let version: Option<i64> = sqlx::query_scalar(
            "UPDATE products SET \
                title = COALESCE($2, title), \
                barcode = COALESCE($3, barcode), \
                price = COALESCE($4, price), \
                unit = COALESCE($5, unit), \
                tax_rate = COALESCE($6, tax_rate), \
                is_weight = COALESCE($7, is_weight), \
                category_id = COALESCE($8, category_id), \
                tax_group = COALESCE($9, tax_group), \
                updated_at = now() \
             WHERE id = $1 RETURNING server_version",
        )
        .bind(row_id)
        .bind(title)
        .bind(barcode)
        .bind(price)
        .bind(unit)
        .bind(tax_rate)
        .bind(is_weight)
        .bind(category_id)
        .bind(tax_group.clone())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("upsert products: {e}"))?;
        Ok(version)
    } else {
        // Новий рядок каталогу: `title` — NOT NULL без default, вигадувати
        // його не можна → відмова з причиною (жодного «порожнього товару»).
        let Some(title) = title else {
            return Err("новий товар без назви (payload.name) — title NOT NULL".to_string());
        };
        let version: Option<i64> = sqlx::query_scalar(
            "INSERT INTO products \
                (id, title, barcode, price, unit, tax_rate, is_weight, category_id, tax_group) \
             VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, false), $8, $9) \
             RETURNING server_version",
        )
        .bind(row_id)
        .bind(title)
        .bind(barcode)
        .bind(price)
        .bind(unit)
        .bind(tax_rate)
        .bind(is_weight)
        .bind(category_id)
        .bind(tax_group)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("insert products: {e}"))?;
        Ok(version)
    }
}

async fn apply_suppliers(
    tx: &mut PgConnection,
    row_id: Uuid,
    op: &str,
    payload: &Value,
) -> Result<Option<i64>, String> {
    if op == "delete" {
        let version: Option<i64> = sqlx::query_scalar(
            "UPDATE suppliers SET is_deleted = true, updated_at = now() \
             WHERE id = $1 RETURNING server_version",
        )
        .bind(row_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("tombstone suppliers: {e}"))?;
        // Рядка на хабі немає → tombstone не потрібен (видалення локальне).
        // Чесна відмова з причиною: «тихо accepted» збрехало б вузлу.
        return match version {
            Some(v) => Ok(Some(v)),
            None => Err("рядка немає на хабі — tombstone не потрібен".to_string()),
        };
    }

    let name = opt_str(payload, &["name"]);
    let phone = opt_str(payload, &["phone"]);
    let edrpou = opt_str(payload, &["edrpou"]);

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM suppliers WHERE id = $1)")
        .bind(row_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("перевірка suppliers: {e}"))?;

    if exists {
        let version: Option<i64> = sqlx::query_scalar(
            "UPDATE suppliers SET \
                name = COALESCE($2, name), \
                phone = COALESCE($3, phone), \
                edrpou = COALESCE($4, edrpou), \
                updated_at = now() \
             WHERE id = $1 RETURNING server_version",
        )
        .bind(row_id)
        .bind(name)
        .bind(phone)
        .bind(edrpou)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("upsert suppliers: {e}"))?;
        Ok(version)
    } else {
        let Some(name) = name else {
            return Err("новий постачальник без назви (payload.name) — name NOT NULL".to_string());
        };
        let version: Option<i64> = sqlx::query_scalar(
            "INSERT INTO suppliers (id, name, phone, edrpou) VALUES ($1, $2, $3, $4) \
             RETURNING server_version",
        )
        .bind(row_id)
        .bind(name)
        .bind(phone)
        .bind(edrpou)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("insert suppliers: {e}"))?;
        Ok(version)
    }
}

/// E5-B5/C3 (ADR-0008 §7.1-B5, §5 рядок 33, рішення Творця Б1 2026-09-12):
/// ціна точки — СПІЛЬНА сутність МЕРЕЖІ. Хаб — авторитет: присвоює ЄДИНИЙ
/// `server_version` (тригер `trg_store_product_prices_bump`), роздає всім вузлам
/// наявним pull. Шлях той самий, що `products.price`.
///
/// ⚠ Ключова відмінність від `products`/`suppliers`: рядок має ПРИРОДНИЙ ключ
/// `(store_id, product_id)` (UNIQUE у схемі), тобто два вузли незалежно можуть
/// створити ДВІ локальні рядки для однієї пари (різні `id`). Тому upsert
/// застосовується у три кроки (id → природний ключ → INSERT) і, якщо хаб
/// застосував правку до ІНШОГО рядка, у відповідь додається `note` з
/// канонічним id — інакше вузол вважав би канонічним свій локальний uuid, а
/// `INSERT` із ним упав би на UNIQUE і правка вузла не доїхала б зовсім.
async fn apply_store_product_prices(
    tx: &mut PgConnection,
    row_id: Uuid,
    op: &str,
    payload: &Value,
) -> Result<Applied, String> {
    if op == "delete" {
        // Tombstone: «перевизначення ціни знято» ≠ «рядка не існує» (історія
        // ціни лишається, вузли отримують op=delete — як у products).
        let version: Option<i64> = sqlx::query_scalar(
            "UPDATE store_product_prices SET is_deleted = true, updated_at = now() \
             WHERE id = $1 RETURNING server_version",
        )
        .bind(row_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("tombstone store_product_prices: {e}"))?;
        return match version {
            Some(v) => Ok(Applied::version(Some(v))),
            None => Err("рядка немає на хабі — tombstone не потрібен".to_string()),
        };
    }

    let store_id = opt_uuid(payload, "store_id")?;
    let product_id = opt_uuid(payload, "product_id")?;
    let price = opt_decimal(payload, "price")?;

    // Крок 1: рядок за id — звичайний шлях (вузол пропонує рядок, який уже
    // канонічний на хабі). `is_deleted = false` СВІДОМО (на відміну від
    // products/suppliers): tombstone ціни означає «перевизначення знято», а
    // нова пропозиція на ту саму пару = перевизначення повернуто; лишити
    // is_deleted=true означало б тихо поховати живу правку вузла.
    let by_id: Option<i64> = sqlx::query_scalar(
        "UPDATE store_product_prices \
         SET price = COALESCE($2, price), is_deleted = false, updated_at = now() \
         WHERE id = $1 RETURNING server_version",
    )
    .bind(row_id)
    .bind(price.clone())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("upsert store_product_prices (id): {e}"))?;
    if let Some(v) = by_id {
        return Ok(Applied::version(Some(v)));
    }

    // Крок 2: КАНОНІЧНИЙ рядок за природним ключем. Сюди потрапляємо, коли
    // uuid вузла на хабі невідомий, але рядок для пари вже є (створив інший
    // вузол/адмінка хаба). Застосовуємо до канонічного і повідомляємо його id.
    if let (Some(sid), Some(pid)) = (store_id, product_id) {
        let canonical: Option<(i64, Uuid)> = sqlx::query_as(
            "UPDATE store_product_prices \
             SET price = COALESCE($3, price), is_deleted = false, updated_at = now() \
             WHERE store_id = $1 AND product_id = $2 \
             RETURNING server_version, id",
        )
        .bind(sid)
        .bind(pid)
        .bind(price.clone())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("upsert store_product_prices (store,product): {e}"))?;
        if let Some((version, hub_id)) = canonical {
            return Ok(Applied {
                version: Some(version),
                note: Some(format!(
                    "рядок {row_id} на хабі відсутній; правку застосовано до КАНОНІЧНОГО                      рядка {hub_id} за (store_id, product_id) — локальний id вузла не                      канонічний, орієнтуйтесь на delta pull"
                )),
            });
        }
    }

    // Крок 3: новий рядок. Поля NOT NULL у схемі → без них відмова з причиною
    // (вигадувати ціну/пару за вузол заборонено).
    let (Some(sid), Some(pid), Some(price)) = (store_id, product_id, price) else {
        return Err(
            "новий рядок ціни вимагає payload.store_id, payload.product_id, payload.price"
                .to_string(),
        );
    };
    let version: Option<i64> = sqlx::query_scalar(
        "INSERT INTO store_product_prices (id, store_id, product_id, price) \
         VALUES ($1, $2, $3, $4) RETURNING server_version",
    )
    .bind(row_id)
    .bind(sid)
    .bind(pid)
    .bind(price)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("insert store_product_prices: {e}"))?;
    Ok(Applied::version(version))
}

/// E5-B2/D3 (ADR-0008 §7.1-D3, §10 №2, рішення Творця Б2 2026-09-16, варіант A):
/// касир створюється ЛОКАЛЬНО на вузлі (offline-first, §2.2) і стає
/// канонічним після прийняття пропозиції хабом.
///
/// Хаб ставить рядку `sync_state='confirmed'` — він і є авторитет (§4.2 п.3).
/// Локальний маркер вузла (`pending_hub`) при цьому НЕ роздається в pull: рядок,
/// що приїхав із хаба, канонічний за визначенням (DEFAULT колонки).
///
/// `login`/`password_hash` обов'язкові для INSERT (NOT NULL у схемі), `role`
/// валідується ДО SQL (невідома роль → зрозуміла відмова, не текст приведення).
/// Колізія `login` (UNIQUE) → пропозиція стає `rejected` з текстом причини:
/// злити двох різних касирів в одного хаб не має права.
async fn apply_users(
    tx: &mut PgConnection,
    row_id: Uuid,
    op: &str,
    payload: &Value,
) -> Result<Applied, String> {
    if op == "delete" {
        let version: Option<i64> = sqlx::query_scalar(
            "UPDATE users SET is_deleted = true, updated_at = now() \
             WHERE id = $1 RETURNING server_version",
        )
        .bind(row_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("tombstone users: {e}"))?;
        return match version {
            Some(v) => Ok(Applied::version(Some(v))),
            None => Err("рядка немає на хабі — tombstone не потрібен".to_string()),
        };
    }

    let name = opt_str(payload, &["name"]);
    let login = opt_str(payload, &["login"]);
    let password_hash = opt_str(payload, &["password_hash"]);
    let pin_hash = opt_str(payload, &["pin_hash"]);
    let role = opt_str(payload, &["role"]);
    let is_active = opt_bool(payload, "is_active")?;
    if let Some(r) = &role {
        if !USER_ROLES.contains(&r.as_str()) {
            return Err(format!(
                "невідома роль '{r}' (дозволено: {})",
                USER_ROLES.join(", ")
            ));
        }
    }

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(row_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("перевірка users: {e}"))?;

    if exists {
        // Часткове злиття (як у products/suppliers): NULL = «поле не надіслали
        // → не чіпати». `sync_state='confirmed'` — хаб канонізує рядок.
        let version: Option<i64> = sqlx::query_scalar(
            "UPDATE users SET \
                name = COALESCE($2, name), \
                login = COALESCE($3, login), \
                password_hash = COALESCE($4, password_hash), \
                pin_code = COALESCE($5, pin_code), \
                role = COALESCE($6::user_role, role), \
                is_active = COALESCE($7, is_active), \
                sync_state = 'confirmed', \
                updated_at = now() \
             WHERE id = $1 RETURNING server_version",
        )
        .bind(row_id)
        .bind(name)
        .bind(login)
        .bind(password_hash)
        .bind(pin_hash)
        .bind(role)
        .bind(is_active)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("upsert users: {e}"))?;
        Ok(Applied::version(version))
    } else {
        let (Some(name), Some(login), Some(password_hash)) = (name, login, password_hash) else {
            return Err(
                "новий касир вимагає payload.name, payload.login, payload.password_hash \
                 (NOT NULL у схемі `users`; логін/пароль створює вузол, хаб не вигадує)"
                    .to_string(),
            );
        };
        let version: Option<i64> = sqlx::query_scalar(
            "INSERT INTO users \
                (id, name, login, password_hash, pin_code, role, is_active, sync_state) \
             VALUES ($1, $2, $3, $4, $5, COALESCE($6, 'cashier')::user_role, \
                     COALESCE($7, true), 'confirmed') \
             RETURNING server_version",
        )
        .bind(row_id)
        .bind(name)
        .bind(login)
        .bind(password_hash)
        .bind(pin_hash)
        .bind(role)
        .bind(is_active)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("insert users: {e}"))?;
        Ok(Applied::version(version))
    }
}

// ─── Хендлер: GET /api/v1/admin/sync/conflicts ───────────────────────────────

/// Правило детермінованого порядку з ADR §4.2 п.5 — використовується і для
/// вибірки, і для поля `recommended` у відповіді (одне джерело правила).
pub const CONFLICT_RULE: &str =
    "server_version DESC (уже канонічне) >> base_version DESC >> explicit priority DESC >> created_at ASC";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConflictProposal {
    pub id: i64,
    pub entity: String,
    pub row_id: Uuid,
    pub op: String,
    pub client_uuid: Uuid,
    pub store_id: Option<Uuid>,
    pub base_version: i64,
    pub priority: Option<i32>,
    pub status: String,
    pub server_version: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
}

/// Черга конфліктів спільних довідників — рішення ОПЕРАТОРА (ADR §7.2 п.5).
///
/// Конфлікти НЕ зливаються автоматично: тут видно КОЖНУ пропозицію-учасницю з
/// її `client_uuid`/`store_id`, а `recommended` показує, кого правило ADR §4.2
/// п.5 ставить першим (детерміновано). Автозастосування переможця — НЕ робимо:
/// це мовчазна втрата правки іншого вузла, яку ADR прямо забороняє; вибір
/// переможця оператором — рішення Творця (політика, не механізм).
pub async fn conflicts(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
) -> Result<Json<Value>, CatalogErr> {
    let pool = state.store_pool.clone().ok_or(CatalogErr::Unavailable)?;
    auth_routes::require_admin(&state, &claims).await?;

    let rows: Vec<ConflictProposal> = sqlx::query_as(
        "SELECT id, entity, row_id, op, client_uuid, store_id, base_version, priority, \
                status, server_version, created_at, decided_at, error \
         FROM catalog_change_requests WHERE status = 'conflict' \
         ORDER BY entity, row_id, server_version DESC NULLS LAST, base_version DESC, \
                  priority DESC NULLS LAST, created_at ASC, id ASC",
    )
    .fetch_all(&pool)
    .await?;

    // Групування за (entity,row_id): оператор бачить СТОРІН конфлікту разом.
    let mut groups: Vec<Value> = Vec::new();
    let mut current: Option<(String, Uuid, Vec<ConflictProposal>)> = None;
    for row in rows {
        match &mut current {
            Some((entity, row_id, items)) if *entity == row.entity && *row_id == row.row_id => {
                items.push(row);
            }
            _ => {
                if let Some((entity, row_id, items)) = current.take() {
                    groups.push(conflict_group(&entity, row_id, &items));
                }
                current = Some((row.entity.clone(), row.row_id, vec![row]));
            }
        }
    }
    if let Some((entity, row_id, items)) = current.take() {
        groups.push(conflict_group(&entity, row_id, &items));
    }

    let total: usize = groups
        .iter()
        .map(|g| g["proposals"].as_array().map(Vec::len).unwrap_or(0))
        .sum();
    Ok(Json(json!({
        "count": total,
        "groups": groups,
        "rule": CONFLICT_RULE,
        "resolution": "рішення оператора (автозастосування переможця не робиться — \
                       мовчазна втрата правки заборонена ADR §4.2)",
    })))
}

fn conflict_group(entity: &str, row_id: Uuid, items: &[ConflictProposal]) -> Value {
    // items уже в детермінованому порядку правила → перший і є recommended.
    let recommended = items.first().map(|p| {
        json!({
            "client_uuid": p.client_uuid,
            "store_id": p.store_id,
            "applied": p.server_version.is_some(),
            "server_version": p.server_version,
        })
    });
    json!({
        "entity": entity,
        "row_id": row_id,
        "proposals": items,
        "recommended": recommended,
    })
}

// ─── Витяг полів payload (без вигадування значень) ──────────────────────────

fn opt_str(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| payload.get(*k))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn opt_uuid(payload: &Value, key: &str) -> Result<Option<Uuid>, String> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Uuid::parse_str(s.trim())
            .map(Some)
            .map_err(|e| format!("{key}: невалідний uuid ({e})")),
        Some(other) => Err(format!("{key}: очікували uuid-рядок, маємо {other}")),
    }
}

fn opt_decimal(payload: &Value, key: &str) -> Result<Option<BigDecimal>, String> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(v @ (Value::String(_) | Value::Number(_))) => {
            let raw = match v {
                Value::String(s) => s.trim().to_string(),
                other => other.to_string(),
            };
            BigDecimal::from_str(&raw)
                .map(Some)
                .map_err(|e| format!("{key}: не число ({e})"))
        }
        Some(other) => Err(format!("{key}: очікували число, маємо {other}")),
    }
}

fn opt_bool(payload: &Value, key: &str) -> Result<Option<bool>, String> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::Number(n)) if n.as_i64() == Some(0) => Ok(Some(false)),
        Some(Value::Number(n)) if n.as_i64() == Some(1) => Ok(Some(true)),
        Some(other) => Err(format!("{key}: очікували bool, маємо {other}")),
    }
}

/// Ключі payload, дозволені для довідника (для документації/тестів).
pub fn allowed_keys(entity: &str) -> &'static [&'static str] {
    match entity {
        "products" => &PRODUCT_KEYS,
        "suppliers" => &SUPPLIER_KEYS,
        "store_product_prices" => &STORE_PRICE_KEYS,
        "users" => &USER_KEYS,
        _ => &[],
    }
}
