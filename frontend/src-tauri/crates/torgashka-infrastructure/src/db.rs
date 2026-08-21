//! PostgreSQL-підключення (етап 1 — довідники READ).
//!
//! ТІЛЬКИ читання: пул `sqlx::PgPool`, жодних міграцій і жодних write-операцій.
//! DSN береться з env `DATABASE_URL`; fallback — компоненти `DB_*` з
//! `backend/.env` (спільна конфігурація з Python-бекендом).

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Помилки підключення до БД.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("DATABASE_URL не знайдено: задайте DATABASE_URL або DB_* у backend/.env")]
    MissingUrl,
    #[error("помилка читання конфігурації: {0}")]
    Io(#[from] std::io::Error),
    #[error("помилка підключення до PostgreSQL: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("невірний DATABASE_URL: {0}")]
    BadUrl(String),
}

/// Кандидати шляхів до `backend/.env` (залежно від CWD запуску).
fn env_file_candidates() -> Vec<std::path::PathBuf> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        // CWD = torgashka/ (репозиторій проєкту)
        std::path::PathBuf::from("backend/.env"),
        // CWD = crates/ (запуск тестів з кореня workspace)
        std::path::PathBuf::from("../../backend/.env"),
        // CWD = frontend/src-tauri (запуск фасаду через Tauri / cargo run)
        std::path::PathBuf::from("../../../backend/.env"),
        // CWD = crates/torgashka-infrastructure (запуск тестів цього крейта)
        std::path::PathBuf::from("../../../../backend/.env"),
        // Абсолютний шлях від маніфесту цього крейта (torgashka-infrastructure).
        manifest.join("../../../../backend/.env"),
    ]
}

/// Примітивний парсер значення з .env-файлу (без зовнішніх залежностей).
pub(crate) fn parse_env_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        if k.trim() == key {
            Some(v.trim().trim_matches('"').trim_matches('\'').to_string())
        } else {
            None
        }
    })
}

/// Резолв DATABASE_URL: env `DATABASE_URL` → `backend/.env` (DB_*) → Err.
pub fn resolve_database_url() -> Result<String, DbError> {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.trim().is_empty() {
            return Ok(url);
        }
    }
    for candidate in env_file_candidates() {
        if let Ok(content) = std::fs::read_to_string(&candidate) {
            let get = |k: &str| parse_env_value(&content, k);
            if let (Some(host), Some(port), Some(user), Some(pass), Some(db)) = (
                get("DB_HOST"),
                get("DB_PORT"),
                get("DB_USER"),
                get("DB_PASSWORD"),
                get("DB_NAME"),
            ) {
                return Ok(format!("postgresql://{user}:{pass}@{host}:{port}/{db}"));
            }
        }
    }
    Err(DbError::MissingUrl)
}

/// Створює пул читання PostgreSQL.
///
/// - `max_connections` — розмір пулу (за замовчуванням 5 — read-навантаження легке).
/// - `acquire_timeout` — 5 секунд, щоб фасад швидко віддав помилку, а не висів.
pub async fn connect_readonly_pool(max_connections: u32) -> Result<PgPool, DbError> {
    let url = resolve_database_url()?;
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await?;
    Ok(pool)
}

/// Ім'я БД з postgresql:// DSN (частина після останнього '/').
fn dbname_from_url(url: &str) -> Result<String, DbError> {
    let before = url.split('?').next().unwrap_or(url);
    let idx = before
        .rfind('/')
        .ok_or_else(|| DbError::BadUrl(url.to_string()))?;
    let name = &before[idx + 1..];
    if name.is_empty() {
        return Err(DbError::BadUrl(url.to_string()));
    }
    Ok(name.to_string())
}

/// Створює пул для integration-тестів — ІЗОЛЬОВАНО від робочої БД.
///
/// - env `TEST_DATABASE_URL` задано → використовується напряму (без перевірки);
/// - інакше береться робочий URL (`resolve_database_url`) і через `replace_dbname`
///   назва БД замінюється на `<dbname>_test` (pos_system_fresh → pos_system_fresh_test);
/// - якщо вже вказує на тестову БД (ім'я містить "test") — використовується як є;
/// - фінальне ім'я БД має містити "test", інакше — `DbError` (захист від
///   запуску integration-тестів проти робочої БД).
pub async fn connect_test_pool(max_connections: u32) -> Result<PgPool, DbError> {
    let url = if let Ok(u) = std::env::var("TEST_DATABASE_URL") {
        if u.trim().is_empty() {
            return Err(DbError::BadUrl(
                "TEST_DATABASE_URL задано, але порожній".to_string(),
            ));
        }
        u
    } else {
        let work = resolve_database_url()?;
        let work_name = dbname_from_url(&work)?;
        if work_name.contains("test") {
            work
        } else {
            replace_dbname(&work, &format!("{work_name}_test"))?
        }
    };
    let final_name = dbname_from_url(&url)?;
    if !final_name.contains("test") {
        return Err(DbError::BadUrl(format!(
            "integration тести заборонено запускати проти робочої БД '{final_name}'; задайте TEST_DATABASE_URL або створіть БД {final_name}_test"
        )));
    }
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await?;
    Ok(pool)
}

// ─────────────────────────────────────────────────────────────────────────────
// Авто-міграції (Частина 1.2) + схема для шаблонної БД (Частина 2)
// ─────────────────────────────────────────────────────────────────────────────
// Джерело істини схеми — backend/alembic/versions/0001..0005 (DDL згенеровано
// pg_dump з робочої БД на версії 0005). Схема застосовується:
//   1) при старті фасаду на fresh-БД (тільки якщо таблиці users немає);
//   2) при створенні шаблонної БД torgashka_template (перший setup).
// ─────────────────────────────────────────────────────────────────────────────

/// Повна схема БД: 34 таблиці + enums/індекси/RLS-політики + owners_db.
pub const SCHEMA_SQL: &str = include_str!("schema.sql");

/// DDL owners_db (ідемпотентний) — виконується ЗАВЖДИ при старті, щоб
/// існуючі БД (мігровані через alembic до 0005) також отримали мета-таблицю.
const OWNERS_DB_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS public.owners_db (
    owner_id uuid NOT NULL,
    db_name text NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT owners_db_pkey PRIMARY KEY (owner_id),
    CONSTRAINT owners_db_db_name_key UNIQUE (db_name),
    CONSTRAINT owners_db_owner_id_fkey FOREIGN KEY (owner_id)
        REFERENCES public.users(id) ON DELETE CASCADE
);
"#;

/// DDL cash_operations (ідемпотентний) — виконується ЗАВЖДИ при старті, щоб
/// існуючі БД (мігровані через alembic без таблиці) отримали її без fresh-install.
/// CHECK-обмеження лишаються в schema.sql (fresh); тут — лише типи + FK/індекс.
const CASH_OPS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS public.cash_operations (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    store_id uuid NOT NULL,
    user_id uuid NOT NULL,
    operation_type varchar(16) NOT NULL,
    cash_type varchar(8) DEFAULT 'cash'::character varying NOT NULL,
    amount numeric(12,2) NOT NULL,
    comment text,
    created_at timestamptz DEFAULT now() NOT NULL,
    CONSTRAINT cash_operations_pkey PRIMARY KEY (id),
    CONSTRAINT cash_operations_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE,
    CONSTRAINT cash_operations_cash_type_check
        CHECK ((cash_type = ANY (ARRAY['cash'::text, 'card'::text])))
);
-- Ідемпотентна міграція для вже існуючих БД (без cash_type).
ALTER TABLE public.cash_operations ADD COLUMN IF NOT EXISTS
    cash_type varchar(8) DEFAULT 'cash'::character varying NOT NULL;
ALTER TABLE public.cash_operations
    DROP CONSTRAINT IF EXISTS cash_operations_cash_type_check;
ALTER TABLE public.cash_operations
    ADD CONSTRAINT cash_operations_cash_type_check
    CHECK ((cash_type = ANY (ARRAY['cash'::text, 'card'::text])));
CREATE INDEX IF NOT EXISTS ix_cash_operations_store_id
    ON public.cash_operations (store_id);
"#;

/// Ідемпотентне застосування схеми при старті фасаду.
///
/// - Якщо таблиці `users` немає (fresh-БД) → виконується повна схема.
/// - `owners_db` створюється завжди (CREATE TABLE IF NOT EXISTS) — покриває
///   і fresh, і вже мігровані БД без неї.
pub async fn ensure_schema(pool: &PgPool) -> Result<(), DbError> {
    let has_users: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(pool)
        .await
        .map_err(DbError::Sqlx)?;
    if !has_users {
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(pool)
            .await
            .map_err(DbError::Sqlx)?;
        eprintln!(
            "[torgashka-infrastructure] схема БД застосована (fresh install: 34 таблиці)"
        );
    }
    sqlx::raw_sql(OWNERS_DB_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(CASH_OPS_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    Ok(())
}

/// Підміняє назву БД у postgresql:// DSN (для підключення до шаблонної БД).
pub fn replace_dbname(url: &str, new_db: &str) -> Result<String, DbError> {
    let (before, query) = url
        .split_once('?')
        .map(|(b, q)| (b, Some(q)))
        .unwrap_or((url, None));
    let idx = before
        .rfind('/')
        .ok_or_else(|| DbError::BadUrl(url.to_string()))?;
    let mut out = format!("{}/{}", &before[..idx], new_db);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    Ok(out)
}
