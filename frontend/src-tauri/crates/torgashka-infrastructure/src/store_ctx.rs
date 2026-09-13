//! StoreContext — контекст поточної торговельної точки (Етап 3 мультиточковості).
//!
//! Задача: кожен запит Rust-фасаду несе `X-Store-Id` + JWT `sub` (user_id).
//! Для Row-Level Security (міграція 0004_rls) PostgreSQL має бачити
//! `current_setting('app.user_id')` / `current_setting('app.store_id')`
//! на КОЖНОМУ запиті, що виконується в межах цього HTTP-запиту.
//!
//! Проблема: репозиторії працюють через `sqlx::PgPool` — пул з'єднань, і
//! послідовні запити можуть потрапити на РІЗНІ з'єднання.
//!
//! Рішення — [`StorePool`] + [`StoreRequest`] (контекст запиту):
//!   - `StoreRequest::open()` — ОДИН круг: бере з'єднання з пула і ставить
//!     `app.user_id`/`app.store_id` (`is_local=false`) + (для бізнес-шляхів)
//!     перевірку доступу `user_stores` — одним SELECT;
//!   - далі ВСІ одиночні запити репозиторіїв (`&StorePool` як `Executor`)
//!     виконуються на ЦЬОМУ з'єднанні, БЕЗ `set_config`/`reset` на statement;
//!   - `StoreRequest::finish()` — з'єднання повертається в пул, а RLS-контекст
//!     скидає хук пула `after_release` (`db::connect_pool_with_ctx_reset`) —
//!     тобто скидання йде ПОЗА критичним шляхом відповіді.
//!
//! Ціна HTTP-запиту: N+2 мережевих круги замість 3N+2 (N — кількість
//! одиночних запитів). Виміряно на касі (ZeroTier, 1 круг ≈ 29 мс):
//! `GET /api/v1/products?size=20` — 455-552 мс / 15-16 SQL-подій → ≤150 мс / ≤5.
//!
//! Чому НЕ «одна транзакція на запит» (BEGIN … COMMIT): (1) sqlx не дозволяє
//! сумістити `BEGIN` і `set_config` в один круг без `raw_sql(&dynamic_str)`, а
//! `raw_sql` із позиченим рядком ламає lifetime-інференс `Executor` в
//! дженерик-контексті (E0277 «Executor is not general enough»); (2) явна
//! транзакція на весь запит змінює семантику помилок (aborted-tx: після першої
//! ж помилки падає весь запит, 25P02) і семантику записів при скасуванні
//! ф'ючери. Сесійний контекст запиту дає ті самі N+2 круги, зберігаючи
//! per-statement семантику (авто-коміт кожного statement — як було).
//!
//! Безпека ізоляції точок:
//!   - `set_config(..., is_local=false)` живе в СЕСІЇ з'єднання, тому контекст
//!     ОБОВ'ЯЗКОВО скидається ДО повернення з'єднання в пул (хук
//!     `after_release`) — жоден споживач, зокрема прямий `&PgPool` в адмін-гілках,
//!     не побачить точку попереднього запиту;
//!   - якщо запит скасовано/панікував (клієнт відключився) — `Drop` фізично
//!     ЗАКРИВАЄ з'єднання (`close_on_drop`), і жоден «брудний» контекст не
//!     потрапляє в пул;
//!   - fallback-гілка (без контексту запиту — фонові таски, `tokio::spawn`,
//!     тести) працює за старою моделлю: `set_config` → запит → `reset`.
//!
//! Контекст зберігається в `tokio::task_local!` — middleware фасаду
//! обгортає `next.run(req)` у `with_store_ctx(ctx, ...)` + [`StoreRequest::scope`],
//! тому ВСІ запити хендлера (той самий таск) бачать поточну точку.
//!
use std::ops::Deref;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use futures_util::TryStreamExt;
use sqlx::Database;
use sqlx::{Describe, Either, Error, Execute, Executor, PgPool, Postgres};
use uuid::Uuid;

/// Контекст поточного запиту: користувач + активна точка.
#[derive(Debug, Clone)]
pub struct StoreCtx {
    /// user_id (JWT `sub`).
    pub user_id: Uuid,
    /// store_id (заголовок `X-Store-Id`).
    pub store_id: Uuid,
    /// Роль користувача (owner|admin|cashier) — з JWT.
    pub role: String,
}

tokio::task_local! {
    /// Task-local контекст точки (видимий у всьому хендлері запиту).
    static STORE_CTX: StoreCtx;
}

/// Поточний контекст точки, якщо middleware його встановив.
pub fn current_store_ctx() -> Option<StoreCtx> {
    STORE_CTX.try_with(|c| c.clone()).ok()
}

/// Виконує ф'ючеру в межах контексту точки (обгортка middleware).
pub async fn with_store_ctx<T>(ctx: StoreCtx, fut: impl std::future::Future<Output = T>) -> T {
    STORE_CTX.scope(ctx, fut).await
}

tokio::task_local! {
    /// Контекст запиту (А): з'єднання з уже проставленим `app.user_id`/
    /// `app.store_id` + прапорці стану. Одиночні запити репозиторіїв
    /// (`&StorePool` як `Executor`) виконуються на ньому — без `set_config`
    /// і `reset` на кожен statement.
    static REQ_TX: Arc<RequestTx>;
}

/// Контекст одного HTTP-запиту: з'єднання з RLS-контекстом точки.
///
/// Свідомо на `std::sync::Mutex` (не `tokio::sync::Mutex`): критична секція —
/// лише «взяти/повернути» `Option<PoolConnection>` (жодного `.await` під замком).
/// З'єднання виходить із замка НА ЧАС statement — саме тому воно має той самий
/// borrow/lifetime-контур, що й звичний `&mut *conn` у sqlx (`&mut **guard` із
/// живим замком дає E0277 «Executor is not general enough»).
pub struct RequestTx {
    conn: std::sync::Mutex<Option<sqlx::pool::PoolConnection<Postgres>>>,
    /// `true` після `finish`: з'єднання, що повернеться пізніше (statement у
    /// польоті), мусить бути ЗАКРИТЕ, а не віддане в пул (контекст точки вже
    /// скинуто, і повертати «чуже» з'єднання не можна).
    finished: std::sync::atomic::AtomicBool,
}

impl RequestTx {
    fn new(conn: sqlx::pool::PoolConnection<Postgres>) -> Self {
        Self {
            conn: std::sync::Mutex::new(Some(conn)),
            finished: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Забирає з'єднання на час одного statement.
    /// `None` — з'єднання зайняте іншим statement цього ж запиту, або контекст
    /// уже закрито (тоді викликач іде в безпечний fallback — стару модель).
    fn take(&self) -> Option<sqlx::pool::PoolConnection<Postgres>> {
        self.conn.lock().ok().and_then(|mut g| g.take())
    }

    /// Повертає з'єднання після statement.
    fn put(&self, mut conn: sqlx::pool::PoolConnection<Postgres>) {
        if self.finished.load(std::sync::atomic::Ordering::SeqCst) {
            conn.close_on_drop();
            return;
        }
        match self.conn.lock() {
            Ok(mut g) => *g = Some(conn),
            Err(_) => conn.close_on_drop(),
        }
    }
}

impl Drop for RequestTx {
    fn drop(&mut self) {
        // Контекст запиту ще не скинуто (паніка/скасування ф'ючери — клієнт
        // відключився): з'єднання НЕ можна повертати в пул, інакше
        // `app.user_id`/`app.store_id` точки протекли б у наступний запит.
        // Закриваємо фізично — сесія вмирає разом із контекстом.
        if let Ok(mut g) = self.conn.lock() {
            if let Some(mut c) = g.take() {
                c.close_on_drop();
            }
        }
    }
}

/// Контекст (з'єднання з RLS-параметрами точки) одного HTTP-запиту.
///
/// ```ignore
/// let (req, allowed) = StoreRequest::open(&pool, &ctx).await?;   // 1 круг
/// if !allowed { /* 403 */ }
/// let resp = req.scope(async { /* хендлер: N запитів */ }).await;
/// req.finish().await?;                                            // 1 круг
/// ```
pub struct StoreRequest {
    tx: Arc<RequestTx>,
}

impl StoreRequest {
    /// Відкриває контекст запиту БЕЗ перевірки доступу (шляхи управління
    /// точками: `X-Store-Id` там опційний).
    pub async fn open_unchecked(pool: &PgPool, ctx: &StoreCtx) -> Result<Self, Error> {
        let mut conn = pool.acquire().await?;
        // ОДИН круг: обидва `set_config` — одним statement з bind-параметрами.
        // (`raw_sql`/`format!` тут неможливі: `raw_sql(&dynamic)` ламає
        //  Executor-lifetime у дженерик-контексті — див. шапку модуля.)
        sqlx::query(
            "SELECT set_config('app.user_id', $1, false), \
             set_config('app.store_id', $2, false)",
        )
        .bind(ctx.user_id.to_string())
        .bind(ctx.store_id.to_string())
        .execute(&mut *conn)
        .await?;
        Ok(Self {
            tx: Arc::new(RequestTx::new(conn)),
        })
    }

    /// Відкриває контекст запиту і ОДНИМ кругом перевіряє доступ користувача
    /// до точки (`user_stores`) — у тому ж контексті, що й запити хендлера.
    pub async fn open(pool: &PgPool, ctx: &StoreCtx) -> Result<(Self, bool), Error> {
        let mut conn = pool.acquire().await?;
        // Три вирази в одному SELECT: контекст + перевірка доступу = 1 круг.
        let row: (String, String, bool) = sqlx::query_as(
            "SELECT set_config('app.user_id', $1, false), \
                    set_config('app.store_id', $2, false), \
                    EXISTS(SELECT 1 FROM user_stores WHERE user_id = $3 AND store_id = $4)",
        )
        .bind(ctx.user_id.to_string())
        .bind(ctx.store_id.to_string())
        .bind(ctx.user_id)
        .bind(ctx.store_id)
        .fetch_one(&mut *conn)
        .await?;
        let allowed = row.2;
        Ok((
            Self {
                tx: Arc::new(RequestTx::new(conn)),
            },
            allowed,
        ))
    }

    /// Виконує майбутнє в межах контексту запиту (task-local scope).
    pub async fn scope<T>(&self, fut: impl std::future::Future<Output = T>) -> T {
        REQ_TX.scope(self.tx.clone(), fut).await
    }

    /// Завершує контекст запиту: з'єднання повертається в пул, а RLS-контекст
    /// скидає хук пула `after_release` (див. `db::connect_pool_with_ctx_reset`).
    ///
    /// Скидання саме в `after_release`, а не тут, економить один мережевий круг
    /// НА КРИТИЧНОМУ ШЛЯХУ відповіді (≈29 мс на касі); для пула без хука
    /// (тести) контекст скидає fallback-гілка `Executor` перед своїм запитом.
    ///
    /// Інваріант безпеки: з'єднання НІКОЛИ не повертається в пул із живим
    /// контекстом точки — або його скидає хук, або (якщо стан невідомий)
    /// з'єднання фізично закривається (`Drop`/`put`).
    pub async fn finish(self) -> Result<(), Error> {
        self.tx
            .finished
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // З'єднання може бути зайняте statement-ом у польоті — тоді його закриє
        // `put()` (finished=true). Інакше просто повертаємо в пул (Drop).
        drop(self.tx.take());
        Ok(())
    }
}

/// Контекст запиту, якщо такий відкрито (викликається з `Executor`).
fn current_request_tx() -> Option<Arc<RequestTx>> {
    REQ_TX.try_with(|t| t.clone()).ok()
}

/// Обгортка `PgPool`, яка проставляє RLS-контекст на кожен запит.
///
/// Репозиторії тримають `pool: StorePool` замість `PgPool`:
///   - `self.pool.begin()` → транзакція з `set_config(is_local=true)`;
///   - `sqlx::query(...).fetch_*(&self.pool)` → кожен запит на окремому
///     з'єднанні з `set_config(is_local=false)` + reset після виконання.
#[derive(Debug, Clone)]
pub struct StorePool(pub PgPool);

impl StorePool {
    pub fn new(pool: PgPool) -> Self {
        Self(pool)
    }

    /// Починає транзакцію з RLS-контекстом (is_local=true — діє до commit).
    pub async fn begin(&self) -> Result<sqlx::Transaction<'static, Postgres>, Error> {
        let mut tx = self.0.begin().await?;
        if let Some(ctx) = current_store_ctx() {
            set_config(&mut tx, &ctx, true).await?;
        }
        Ok(tx)
    }
}

impl Deref for StorePool {
    type Target = PgPool;

    fn deref(&self) -> &PgPool {
        &self.0
    }
}

/// Скидає RLS-контекст точки на з'єднанні (викликається хуком пула
/// `after_release` та fallback-гілкою `Executor`).
///
/// Публічна, бо потрібна `db::connect_pool_with_ctx_reset` — єдина точка, де
/// гарантується, що з'єднання в пулі завжди без чужого контексту точки.
pub async fn reset_store_ctx(conn: &mut sqlx::PgConnection) -> Result<(), Error> {
    reset_config(conn).await
}

/// Проставляє app.user_id/app.store_id на з'єднанні.
async fn set_config(
    conn: &mut sqlx::PgConnection,
    ctx: &StoreCtx,
    is_local: bool,
) -> Result<(), Error> {
    sqlx::query("SELECT set_config('app.user_id', $1, $3), set_config('app.store_id', $2, $3)")
        .bind(ctx.user_id.to_string())
        .bind(ctx.store_id.to_string())
        .bind(is_local)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Скидає RLS-контекст зі з'єднання (захист від протікання в пул).
///
/// PostgreSQL-квірк (перевірено емпірично): для custom-параметрів (з крапкою)
/// НЕМОЖЛИВО повернути стан «ніколи не встановлений» у живій сесії —
/// `set_config(..., NULL)`, `RESET` і навіть `DISCARD ALL` лишають
/// `current_setting('app.store_id', true) = ''` (порожній рядок), а не NULL.
/// `RESET` — найближчий доступний стан: параметр зникає з `pg_settings`.
///
/// Тому кожен споживач ОБОВ'ЯЗКОВО трактує '' як NULL:
///   `NULLIF(current_setting('app.store_id', true), '')::uuid`
/// (усі такі касти в репозиторіях обгорнуті NULLIF — auth.rs, pos.rs,
/// write.rs, debtors.rs, products_v2.rs, directories.rs; RLS-політики —
/// backend/alembic/versions/0004_rls.py).
async fn reset_config(conn: &mut sqlx::PgConnection) -> Result<(), Error> {
    // set_config(..., NULL, false) ≡ RESET для custom-параметрів (див. вище):
    // лишає '' — споживачі трактують '' як NULL через NULLIF.
    // (raw_sql("RESET ...") тут не використано: ламає lifetime-інференс
    //  Executor у fetch_many — відомий sqlx-нюанс.)
    // Текст навмисно ВІДРІЗНЯЄТЬСЯ від `set_config` з біндами (ctx-open):
    // у pg_stat_activity скидання й відкриття контексту видно окремо, інакше
    // два різні за змістом statement-и виглядають як «два resets підряд».
    // NULL-літерали замість біндів дають той самий стан (див. вище) без
    // парсингу параметрів.
    sqlx::query(
        "SELECT set_config('app.user_id', NULL, false), set_config('app.store_id', NULL, false)",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Фунел запису: `Executor for &StorePool` — ЄДИНЕ місце, через яке фізично
// проходять усі одиночні запити репозиторіїв (`query(...).fetch_*`/`execute`).
//
// Шлях 1 («контекст запиту», А): з'єднання вже має `app.user_id`/`app.store_id`
//   — жодного `set_config`/`reset` на statement;
// Шлях 2 (fallback: контексту запиту немає — фонові таски, `tokio::spawn`,
//   тести, пул без middleware): стара модель `set_config → запит → reset`.
//
// ADR-0008: перехоплення помилки `read_only_sql_transaction` (SQLSTATE 25006)
// тут БІЛЬШЕ НЕМАЄ — концепція «вузол-репліка, що не може писати» видалена
// (E7): кожен вузол пише у ВЛАСНУ read-write БД, а помилки БД класифікуються
// стабільним SQLSTATE-кодом у [`crate::db_error`].
// ─────────────────────────────────────────────────────────────────────────────

/// Один результат одиночного запиту (як `Executor::fetch_many`).
type QueryOutcome =
    Result<Either<<Postgres as Database>::QueryResult, <Postgres as Database>::Row>, Error>;

/// Потік результатів одиночного запиту.
type QueryStream<'e> = BoxStream<'e, QueryOutcome>;

/// `fetch_many` на з'єднанні контексту запиту (шлях 1).
fn req_tx_fetch_many<'e, 'q: 'e, E>(req_tx: Arc<RequestTx>, query: E) -> QueryStream<'e>
where
    E: 'q + Execute<'q, Postgres>,
{
    Box::pin(
        futures_util::stream::once(async move {
            let Some(mut conn) = req_tx.take() else {
                // З'єднання тримає інший statement цього ж запиту — викликач
                // піде в безпечний fallback (стара модель, RLS зберігається).
                return Err(Error::PoolTimedOut);
            };
            let result = (&mut *conn).fetch_many(query).try_collect::<Vec<_>>().await;
            req_tx.put(conn);
            result
        })
        .map_ok(|v| futures_util::stream::iter(v.into_iter().map(Ok)))
        .try_flatten(),
    )
}

/// `fetch_optional` на з'єднанні контексту запиту (шлях 1).
fn req_tx_fetch_optional<'e, 'q: 'e, E>(
    req_tx: Arc<RequestTx>,
    query: E,
) -> BoxFuture<'e, Result<Option<<Postgres as Database>::Row>, Error>>
where
    E: 'q + Execute<'q, Postgres>,
{
    Box::pin(async move {
        let Some(mut conn) = req_tx.take() else {
            return Err(Error::PoolTimedOut);
        };
        let result = (&mut *conn).fetch_optional(query).await;
        req_tx.put(conn);
        result
    })
}

/// `fetch_many` у СТАРІЙ моделі (шлях 2 — без контексту запиту).
fn legacy_fetch_many<'e, 'q: 'e, E>(
    pool: PgPool,
    ctx: Option<StoreCtx>,
    query: E,
) -> QueryStream<'e>
where
    E: 'q + Execute<'q, Postgres>,
{
    Box::pin(
        futures_util::stream::once(async move {
            let mut conn = pool.acquire().await?;
            if let Some(ctx) = &ctx {
                set_config(&mut conn, ctx, false).await?;
            }
            // Повне (eager) виконання: fetch_many → Vec. Після цього reset.
            let result = (&mut *conn).fetch_many(query).try_collect::<Vec<_>>().await;
            // Скидаємо контекст ЛИШЕ якщо ми його ставили. Інакше — зайвий
            // мережевий круг (≈29 мс), а на пулі з хуком `after_release`
            // (production) — ДРУГЕ скидання підряд: хвіст запиту показував
            // два reset-и поспіль (виміряно `measure_resets.py`: 2.2 на
            // запит; після фіксу — 1.0).
            //
            // Безпека: інваріант «з'єднання в пулі завжди без чужого
            // контексту» тримається і без цього reset-у, коли ctx=None —
            // жодного `set_config` на цьому з'єднанні не виконувалось.
            if ctx.is_some() {
                let _ = reset_config(&mut conn).await;
            }
            result
        })
        .map_ok(|v| futures_util::stream::iter(v.into_iter().map(Ok)))
        .try_flatten(),
    )
}

/// `fetch_optional` у СТАРІЙ моделі (шлях 2).
fn legacy_fetch_optional<'e, 'q: 'e, E>(
    pool: PgPool,
    ctx: Option<StoreCtx>,
    query: E,
) -> BoxFuture<'e, Result<Option<<Postgres as Database>::Row>, Error>>
where
    E: 'q + Execute<'q, Postgres>,
{
    Box::pin(async move {
        let mut conn = pool.acquire().await?;
        if let Some(ctx) = &ctx {
            set_config(&mut conn, ctx, false).await?;
        }
        let result = (&mut *conn).fetch_optional(query).await;
        // Див. коментар у `legacy_fetch_many`: скидаємо лише те, що ставили.
        if ctx.is_some() {
            let _ = reset_config(&mut conn).await;
        }
        result
    })
}

impl<'p> Executor<'p> for &'_ StorePool {
    type Database = Postgres;

    fn fetch_many<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxStream<
        'e,
        Result<
            Either<<Self::Database as Database>::QueryResult, <Self::Database as Database>::Row>,
            Error,
        >,
    >
    where
        E: 'q + Execute<'q, Self::Database>,
    {
        // ── Шлях 1: контекст запиту (А) — 0 кругів обв'язки на statement ──
        if let Some(req_tx) = current_request_tx() {
            return req_tx_fetch_many(req_tx, query);
        }
        // ── Шлях 2: fallback (без контексту запиту) ──
        legacy_fetch_many(self.0.clone(), current_store_ctx(), query)
    }

    fn fetch_optional<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxFuture<'e, Result<Option<<Self::Database as Database>::Row>, Error>>
    where
        E: 'q + Execute<'q, Self::Database>,
    {
        // ── Шлях 1: контекст запиту (А) ──
        if let Some(req_tx) = current_request_tx() {
            return req_tx_fetch_optional(req_tx, query);
        }
        // ── Шлях 2: fallback ──
        legacy_fetch_optional(self.0.clone(), current_store_ctx(), query)
    }

    fn prepare_with<'e, 'q: 'e>(
        self,
        sql: &'q str,
        parameters: &'e [<Self::Database as Database>::TypeInfo],
    ) -> BoxFuture<'e, Result<<Self::Database as Database>::Statement<'q>, Error>> {
        // Підготовка запиту контексту не потребує (PREPARE не виконує SQL).
        let pool = self.0.clone();
        Box::pin(async move { pool.acquire().await?.prepare_with(sql, parameters).await })
    }

    #[doc(hidden)]
    fn describe<'e, 'q: 'e>(
        self,
        sql: &'q str,
    ) -> BoxFuture<'e, Result<Describe<Self::Database>, Error>> {
        let pool = self.0.clone();
        Box::pin(async move { pool.acquire().await?.describe(sql).await })
    }
}
