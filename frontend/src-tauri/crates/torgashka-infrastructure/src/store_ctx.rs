//! StoreContext — контекст поточної торговельної точки (Етап 3 мультиточковості).
//!
//! Задача: кожен запит Rust-фасаду несе `X-Store-Id` + JWT `sub` (user_id).
//! Для Row-Level Security (міграція 0004_rls) PostgreSQL має бачити
//! `current_setting('app.user_id')` / `current_setting('app.store_id')`
//! на КОЖНОМУ запиті, що виконується в межах цього HTTP-запиту.
//!
//! Проблема: репозиторії працюють через `sqlx::PgPool` — пул з'єднань, і
//! послідовні запити можуть потрапити на РІЗНІ з'єднання. `set_config(..., true)`
//! (is_local) діє лише в межах поточної транзакції.
//!
//! Рішення — `StorePool`:
//!   - `begin()` — відкриває транзакцію і одразу проставляє `app.user_id`/
//!     `app.store_id` з `is_local=true` (живе до commit/rollback);
//!   - `Executor` для `&StorePool` — кожен одиночний запит виконується на
//!     окремому з'єднанні: set_config (is_local=false) → запит → reset.
//!     Reset обов'язковий, щоб контекст не протікав у пул (безпека точок).
//!
//! Контекст зберігається в `tokio::task_local!` — middleware фасаду
//! обгортає `next.run(req)` у `with_store_ctx(ctx, ...)`, тому ВСІ запити
//! хендлера (той самий таск) бачать поточну точку.

use std::ops::Deref;

use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use futures_util::TryStreamExt;
use sqlx::{Describe, Either, Error, Execute, Executor, PgPool, Postgres};
use sqlx::Database;
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
    sqlx::query("SELECT set_config('app.user_id', $1, false), set_config('app.store_id', $2, false)")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(&mut *conn)
        .await?;
    Ok(())
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
        let pool = self.0.clone();
        let ctx = current_store_ctx();
        Box::pin(
            futures_util::stream::once(async move {
                let mut conn = pool.acquire().await?;
                if let Some(ctx) = &ctx {
                    set_config(&mut conn, ctx, false).await?;
                }
                // Повне (eager) виконання: fetch_many → Vec. Після цього reset.
                let result = (&mut *conn).fetch_many(query).try_collect::<Vec<_>>().await;
                if ctx.is_some() {
                    let _ = reset_config(&mut conn).await;
                }
                result
            })
            .map_ok(|v| futures_util::stream::iter(v.into_iter().map(Ok)))
            .try_flatten(),
        )
    }

    fn fetch_optional<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxFuture<'e, Result<Option<<Self::Database as Database>::Row>, Error>>
    where
        E: 'q + Execute<'q, Self::Database>,
    {
        let pool = self.0.clone();
        let ctx = current_store_ctx();
        Box::pin(async move {
            let mut conn = pool.acquire().await?;
            if let Some(ctx) = &ctx {
                set_config(&mut conn, ctx, false).await?;
            }
            let result = (&mut *conn).fetch_optional(query).await;
            if ctx.is_some() {
                let _ = reset_config(&mut conn).await;
            }
            result
        })
    }

    fn prepare_with<'e, 'q: 'e>(
        self,
        sql: &'q str,
        parameters: &'e [<Self::Database as Database>::TypeInfo],
    ) -> BoxFuture<'e, Result<<Self::Database as Database>::Statement<'q>, Error>> {
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
