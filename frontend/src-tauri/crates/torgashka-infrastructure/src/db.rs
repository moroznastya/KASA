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
    #[error("помилка джерел даних (db_sources.toml): {0}")]
    DbSources(String),
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

/// Резолв DATABASE_URL (порядок пріоритету):
///   1. env `DATABASE_URL` (явний override — тести/CI/embedded PG bootstrap);
///   2. активне джерело з `db_sources.toml` (Етап 3 адмін-панелі, ТЗ 2.4/5.8) —
///      UI «Джерело даних» зберігає `active`, яке застосовується ПРИ СТАРТІ
///      (stability_first: гарячого перепідключення пулів немає, див.
///      torgashka_infrastructure::db_sources);
///   3. `backend/.env` (DB_*) → Err.
///
/// db_sources.toml без `active` або без файлу — мовчазно пропускається
/// (робота як раніше). Якщо `active` задано, але джерело/ключ некоректні —
/// повертається чесна помилка (без мовчазного fallback на іншу БД).
pub fn resolve_database_url() -> Result<String, DbError> {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.trim().is_empty() {
            return Ok(url);
        }
    }
    match crate::db_sources::active_source_url() {
        Ok(Some(url)) => {
            eprintln!(
                "[torgashka-infrastructure] активне джерело даних (db_sources.toml): використовується конфігурована БД"
            );
            return Ok(url);
        }
        Ok(None) => {}
        Err(e) => return Err(DbError::DbSources(e.to_string())),
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

/// Створює пул з автоматичним скиданням RLS-контексту точки.
///
/// Хук `after_release` виконується ПЕРЕД тим, як з'єднання повернеться в пул:
/// саме тут скидаються `app.user_id`/`app.store_id`, які проставив контекст
/// запиту ([`crate::store_ctx::StoreRequest`]). Це (1) гарантує, що жоден
/// споживач — у т.ч. прямий `&PgPool` в адмін-гілках — не побачить точку
/// попереднього запиту, і (2) знімає скидання з критичного шляху відповіді
/// (≈29 мс на касі ZeroTier).
///
/// Якщо скидання не вдалося — стан з'єднання невідомий, тому повертаємо
/// `Ok(false)`: sqlx закриє його, а не віддасть у пул.
pub async fn connect_pool_with_ctx_reset(max_connections: u32) -> Result<PgPool, DbError> {
    let url = resolve_database_url()?;
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .after_release(|conn, _meta| {
            Box::pin(async move {
                match crate::store_ctx::reset_store_ctx(conn).await {
                    Ok(()) => Ok(true),
                    Err(e) => {
                        crate::embedded_pg::pg_log(
                            "WARN",
                            &format!(
                                "пул: скидання RLS-контексту не вдалося ({e}) — з'єднання закрито"
                            ),
                        );
                        Ok(false)
                    }
                }
            })
        })
        .connect(&url)
        .await?;
    Ok(pool)
}

/// Створює пул ЗАПИСУ в апстрім (primary) для standby-вузла (ADR-0007 F3).
///
/// URL передається ЯВНО (`NodeConfig::resolve_upstream_write_url`) і НЕ
/// резолвиться через [`resolve_database_url`]: на standby той вказує на
/// локальну репліку, а репліка НІКОЛИ не є ціллю запису (F5).
///
/// Порожній/пробільний URL → [`DbError::BadUrl`]; `acquire_timeout` 5 с
/// (як у [`connect_readonly_pool`]), щоб адмін-запит швидко віддав 503 (F4).
pub async fn connect_upstream_write_pool(
    url: &str,
    max_connections: u32,
) -> Result<PgPool, DbError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(DbError::BadUrl(
            "upstream_write_url порожній — апстрім-запис недоступний".to_string(),
        ));
    }
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(url)
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

/// Повна схема БД: 40 таблиць + enums/індекси/RLS-політики + owners_db.
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

/// DDL мережевого рівня власника (ідемпотентний) — виконується ЗАВЖДИ при
/// старті: існуючі БД (мігровані через alembic / fresh) отримують 5 owner-
/// таблиць (devices, store_activation_codes, store_product_prices, audit_log,
/// store_sync_state) та enum device_status без fresh-install.
const NETWORK_DDL: &str = r#"
-- ============================================================================
-- Мережевий рівень власника (Частина 3): devices, store_activation_codes,
-- store_product_prices, audit_log, store_sync_state.
-- Створюються ідемпотентно: fresh (schema.sql) і вже мігровані БД (NETWORK_DDL).
-- ============================================================================

-- device_status: PG не має CREATE TYPE IF NOT EXISTS — через DO-блок.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'device_status') THEN
        CREATE TYPE public.device_status AS ENUM ('pending', 'active', 'blocked', 'deleted');
    END IF;
END
$$;

-- Фізичні каси/пристрої мережі. store_id → точка; status — життєвий цикл
-- пристрою (pending → active → blocked/deleted). device_token_hash — токен
-- аутентифікації пристрою (зберігається лише хеш).
CREATE TABLE IF NOT EXISTS public.devices (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    store_id uuid NOT NULL,
    name character varying(255) NOT NULL,
    device_token_hash character varying(255) NOT NULL,
    -- source='legacy_migration' — касу зареєстровано міграцією §9 (БЕЗ коду
    -- активації: вона вже «своя»). NULL — звичайна активація за кодом точки.
    source character varying(50),
    status public.device_status DEFAULT 'pending'::public.device_status NOT NULL,
    app_version character varying(50),
    last_seen_at timestamp without time zone,
    activated_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT devices_pkey PRIMARY KEY (id),
    CONSTRAINT devices_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);
-- Вже існуючі БД (devices створено до Етапа 6): додаємо source без fresh.
ALTER TABLE public.devices ADD COLUMN IF NOT EXISTS
    source character varying(50);

-- Код активації точки (8 символів; колонка varchar(9) — резерв під префікс).
-- Один рядок на магазин: повторна генерація оновлює код (regenerated_at).
CREATE TABLE IF NOT EXISTS public.store_activation_codes (
    store_id uuid NOT NULL,
    code character varying(9) NOT NULL,
    created_by uuid,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    regenerated_at timestamp without time zone,
    CONSTRAINT store_activation_codes_pkey PRIMARY KEY (store_id),
    CONSTRAINT store_activation_codes_code_key UNIQUE (code),
    CONSTRAINT store_activation_codes_created_by_fkey FOREIGN KEY (created_by)
        REFERENCES public.users(id) ON DELETE SET NULL,
    CONSTRAINT store_activation_codes_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);

-- Перевизначення ціни товару по конкретній точці (owner-рівень; на відміну
-- від stock.price — окрема сутність, не залежить від залишків).
CREATE TABLE IF NOT EXISTS public.store_product_prices (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    store_id uuid NOT NULL,
    product_id uuid NOT NULL,
    price numeric(10,2) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT store_product_prices_pkey PRIMARY KEY (id),
    CONSTRAINT store_product_prices_store_id_product_id_key UNIQUE (store_id, product_id),
    CONSTRAINT store_product_prices_product_id_fkey FOREIGN KEY (product_id)
        REFERENCES public.products(id) ON DELETE CASCADE,
    CONSTRAINT store_product_prices_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);

-- Аудит дій адмінки (хто/що/коли; payload — контекст дії в JSON).
-- FK з ON DELETE SET NULL: аудит-слід зберігається при видаленні юзера/точки.
CREATE TABLE IF NOT EXISTS public.audit_log (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    actor_user_id uuid,
    action character varying(100) NOT NULL,
    entity_type character varying(50),
    entity_id uuid,
    store_id uuid,
    payload jsonb,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT audit_log_pkey PRIMARY KEY (id),
    CONSTRAINT audit_log_actor_user_id_fkey FOREIGN KEY (actor_user_id)
        REFERENCES public.users(id) ON DELETE SET NULL,
    CONSTRAINT audit_log_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE SET NULL
);

-- Стан синхронізації точки (офлайн-first): час останньої синхронізації та
-- останній локальний seq, до якого сервер отримав дані точки.
CREATE TABLE IF NOT EXISTS public.store_sync_state (
    store_id uuid NOT NULL,
    device_id uuid,
    last_synced_at timestamp without time zone,
    last_local_seq bigint DEFAULT 0 NOT NULL,
    status character varying(20) DEFAULT 'unknown'::character varying NOT NULL,
    CONSTRAINT store_sync_state_pkey PRIMARY KEY (store_id),
    CONSTRAINT store_sync_state_device_id_fkey FOREIGN KEY (device_id)
        REFERENCES public.devices(id) ON DELETE SET NULL,
    CONSTRAINT store_sync_state_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS ix_devices_store_id ON public.devices USING btree (store_id);

-- Не більше одного legacy-пристрою (source='legacy_migration') на точку:
-- ідемпотентність POST /admin/migrate/legacy на рівні БД (race-safe).
CREATE UNIQUE INDEX IF NOT EXISTS ux_devices_legacy_migration_store
    ON public.devices (store_id) WHERE (source = 'legacy_migration');

CREATE INDEX IF NOT EXISTS ix_store_product_prices_product_id
    ON public.store_product_prices USING btree (product_id);

CREATE INDEX IF NOT EXISTS ix_audit_log_store_id_created_at
    ON public.audit_log USING btree (store_id, created_at);
"#;

/// DDL реєстру вузлів мережі магазинів (ЕТАП 15, network-replication-etap15-20.md
/// §3.1) — ідемпотентний, виконується ЗАВЖДИ при старті: і fresh (schema.sql),
/// і вже мігровані БД отримують network_nodes без fresh-install. node_role/
/// node_status — через DO-блок (PG не має CREATE TYPE IF NOT EXISTS).
const NETWORK_NODES_DDL: &str = r#"
-- ============================================================================
-- Мережа магазинів (ЕТАП 15): реєстр вузлів-standby (копії primary).
-- Реплікація — ЕТАП 16+; тут лише реєстр + join/heartbeat/archive API.
-- Створюються ідемпотентно: fresh (schema.sql) і вже мігровані БД (NETWORK_NODES_DDL).
-- ============================================================================

-- node_role: primary — сам сервер; standby — копія в магазині.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'node_role') THEN
        CREATE TYPE public.node_role AS ENUM ('primary', 'standby');
    END IF;
END
$$;

-- node_status: життєвий цикл вузла (§4 стану-машина):
-- provisioning → syncing → active/lagging → offline/archived.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'node_status') THEN
        CREATE TYPE public.node_status AS ENUM (
            'provisioning', 'syncing', 'active', 'lagging', 'offline', 'archived'
        );
    END IF;
END
$$;

-- Вузол мережі. store_id NULL — сам сервер (primary). join_token_hash —
-- SHA-256 одноразового join-коду (TTL 30 хв); node_token_hash — SHA-256
-- довготривалого токена вузла для heartbeat. Секрети зберігаються лише
-- хешами (як device_token_hash у network.rs).
CREATE TABLE IF NOT EXISTS public.network_nodes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    store_id uuid REFERENCES public.stores(id),  -- NULL для самого сервера (primary)
    name text NOT NULL,
    role public.node_role NOT NULL DEFAULT 'standby',
    status public.node_status NOT NULL DEFAULT 'provisioning',

    -- Провіжинінг (одноразовий код, аналог store_activation_codes)
    join_token_hash text,               -- SHA-256, як device_token_hash
    join_token_expires_at timestamp,    -- TTL 30 хв

    -- Довготривалий токен вузла для heartbeat (окремо від JWT користувача)
    node_token_hash text,

    -- Реплікація (креденшли створюються в ЕТАП 16; імена резервуються тут)
    replication_role_name text,         -- напр. replicator_a1b2c3
    replication_slot_name text,         -- напр. standby_a1b2c3

    -- Телеметрія (оновлюється heartbeat-ом)
    host text,
    app_version text,
    last_seen_at timestamp,
    replication_lag_bytes bigint,
    db_size_bytes bigint,

    created_at timestamp NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at timestamp NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);

CREATE UNIQUE INDEX IF NOT EXISTS network_nodes_replication_slot_uq
    ON public.network_nodes (replication_slot_name)
    WHERE replication_slot_name IS NOT NULL;

CREATE INDEX IF NOT EXISTS network_nodes_store_id_idx
    ON public.network_nodes (store_id);
"#;

/// DDL журналу мережевих подій (рішення Творця, додаток до
/// network-replication-etap15-20.md): детальне логування взаємодії вузлів для
/// діагностики. Ідемпотентний — виконується ЗАВЖДИ при старті (fresh БД
/// отримують його з schema.sql, мігровані — звідси). event — один з
/// контрольованого набору (join|joined|node_created|heartbeat|status_change|
/// promoted|repoint_requested|archived|resync_requested|degraded_local|
/// primary_restored|sync_error|reject_stale); detail — контекст (old_status,
/// new_status, lag_bytes, помилка тощо). node_id NULL для подій без вузла.
const NETWORK_EVENTS_DDL: &str = r#"
-- ============================================================================
-- Мережеві події (рішення Творця): діагностичний журнал взаємодії вузлів.
-- Лог — побічний ефект: записи робляться через log_node_event (не панікує,
-- не валить основний запит). node_id з ON DELETE SET NULL — журнал зберігається
-- навіть якщо вузол видалено (архівація не видаляє; SET NULL — страхівка).
-- ============================================================================
CREATE TABLE IF NOT EXISTS public.network_events (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    node_id uuid,
    event character varying(64) NOT NULL,
    level character varying(16) NOT NULL DEFAULT 'info',
    detail jsonb,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT network_events_pkey PRIMARY KEY (id),
    CONSTRAINT network_events_node_id_fkey FOREIGN KEY (node_id)
        REFERENCES public.network_nodes(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS ix_network_events_node_id_created_at
    ON public.network_events (node_id, created_at DESC);
"#;

/// DDL адмін-панелі власника (Етап 1): юрособа/ЄДРПОУ точки + роль
/// `store_manager` в enum user_role. Ідемпотентно — виконується ЗАВЖДИ при
/// старті: і fresh-схема (schema.sql уже містить колонки/значення), і вже
/// мігровані БД (alembic 0001–0014 без цих змін) отримують їх без fresh-install.
const STORE_LEGAL_COLUMNS_DDL: &str = r#"
ALTER TABLE public.stores ADD COLUMN IF NOT EXISTS
    legal_name character varying(255);
ALTER TABLE public.stores ADD COLUMN IF NOT EXISTS
    edrpou character varying(20);
DO $$
BEGIN
    ALTER TYPE public.user_role ADD VALUE IF NOT EXISTS 'store_manager';
EXCEPTION
    WHEN duplicate_object THEN NULL;
END
$$;
"#;

/// ADR-0008 §7.1-D3 (E5, Alembic 0024): локальний маркер касира вузла.
///
/// `users.sync_state` розрізняє «локально створене, ще не підтверджене хабом»
/// (`pending_hub`) від канонічного (`confirmed`); `local` — зарезервовано під
/// явне локальне створення. Читає його `repositories::auth` (вхід касира за
/// політикою `sync.require_hub_confirm_before_login`), пише — `create_user`.
///
/// Чому тут, а не лише в `schema.sql`: знімок `scripts/schema.sql` (його
/// використовує CI/стенд) старіший за 0024, тому DDL мусить додаватися
/// ідемпотентно при старті фасаду — інакше `INSERT INTO users (… sync_state)`
/// падає з 500 «Помилка БД» на будь-якій БД, зібраній зі знімка.
const USERS_SYNC_STATE_DDL: &str = r#"
ALTER TABLE public.users ADD COLUMN IF NOT EXISTS
    sync_state text NOT NULL DEFAULT 'confirmed';
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'users_sync_state_check'
    ) THEN
        ALTER TABLE public.users ADD CONSTRAINT users_sync_state_check
            CHECK (sync_state IN ('local', 'pending_hub', 'confirmed'));
    END IF;
END
$$;
"#;

/// ADR-0008 §7.1-B5/C3 (E5, Alembic 0024): ціна точки — сутність мережі.
///
/// `server_version` — канонічна версія для дельти pull (`sync.rs`,
/// `query_store_product_prices`), `is_deleted` — tombstone («перевизначення
/// ціни знято» ≠ «рядок зник», `catalog_proposal::apply_store_product_prices`).
///
/// Тригер bump + рядок `sync_meta` сюди НЕ додаються свідомо: вони спираються
/// на функцію `bump_sync_version` з Alembic 0012, якої на БД без sync-шару
/// немає — DDL упав би на старті фасаду. Цю частину шару відтворює
/// `tests/common/sync_schema.rs` (тести) і Alembic (прод).
const STORE_PRICES_VERSION_DDL: &str = r#"
ALTER TABLE public.store_product_prices ADD COLUMN IF NOT EXISTS
    server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE public.store_product_prices ADD COLUMN IF NOT EXISTS
    is_deleted boolean NOT NULL DEFAULT false;
"#;

const RLS_FORCE_DDL: &str = r#"
-- ============================================================================
-- ЕТАП 7: FORCE ROW LEVEL SECURITY для вже мігрованих БД.
-- Ідемпотентно: виконується ЗАВЖДИ при старті (fresh schema.sql уже містить).
-- Політики (app.store_id/app.user_id) визначені в schema.sql / попередніх
-- міграціях; FORCE змушує проходити їх навіть власника таблиць.
-- ============================================================================
ALTER TABLE public.barcodes FORCE ROW LEVEL SECURITY;
ALTER TABLE public.categories FORCE ROW LEVEL SECURITY;
ALTER TABLE public.debtor_payments FORCE ROW LEVEL SECURITY;
ALTER TABLE public.debtors FORCE ROW LEVEL SECURITY;
ALTER TABLE public.inventories FORCE ROW LEVEL SECURITY;
ALTER TABLE public.inventory_items FORCE ROW LEVEL SECURITY;
ALTER TABLE public.invoice_items FORCE ROW LEVEL SECURITY;
ALTER TABLE public.invoices FORCE ROW LEVEL SECURITY;
ALTER TABLE public.product_images FORCE ROW LEVEL SECURITY;
ALTER TABLE public.purchase_order_items FORCE ROW LEVEL SECURITY;
ALTER TABLE public.purchase_orders FORCE ROW LEVEL SECURITY;
ALTER TABLE public.receipt_items FORCE ROW LEVEL SECURITY;
ALTER TABLE public.receipts FORCE ROW LEVEL SECURITY;
ALTER TABLE public.return_invoice_items FORCE ROW LEVEL SECURITY;
ALTER TABLE public.return_invoices FORCE ROW LEVEL SECURITY;
ALTER TABLE public.stock FORCE ROW LEVEL SECURITY;
ALTER TABLE public.stores FORCE ROW LEVEL SECURITY;
ALTER TABLE public.supplier_ledger FORCE ROW LEVEL SECURITY;
ALTER TABLE public.system_settings FORCE ROW LEVEL SECURITY;
ALTER TABLE public.transfer_items FORCE ROW LEVEL SECURITY;
ALTER TABLE public.transfers FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_stores FORCE ROW LEVEL SECURITY;
ALTER TABLE public.work_sessions FORCE ROW LEVEL SECURITY;
ALTER TABLE public.write_off_items FORCE ROW LEVEL SECURITY;
ALTER TABLE public.write_offs FORCE ROW LEVEL SECURITY;
ALTER TABLE public.prro_settings FORCE ROW LEVEL SECURITY;
ALTER TABLE public.prro_shifts FORCE ROW LEVEL SECURITY;
ALTER TABLE public.prro_queue_items FORCE ROW LEVEL SECURITY;
"#;

const RECEIPTS_CLIENT_UUID_DDL: &str = r#"
-- ============================================================================
-- ЕТАП 8: ідемпотентність синхронізації чеків (client_uuid) для наявних БД.
-- Клієнт генерує client_uuid на чек і передає при push (sync.rs PushItem).
-- UNIQUE-індекс ловить гонку двох одночасних push з тим самим client_uuid.
-- ============================================================================
ALTER TABLE public.receipts ADD COLUMN IF NOT EXISTS client_uuid uuid;
CREATE UNIQUE INDEX IF NOT EXISTS uq_receipts_client_uuid
    ON public.receipts (client_uuid)
    WHERE client_uuid IS NOT NULL;
"#;
/// Легасі WAL-політика з часів мережі на фізичній реплікації (ЕТАП 20 §13):
/// `max_slot_wal_keep_size = 10GB` — захист диска від WAL-накопичення
/// застарілими replication-слотами. ADR-0008 скасував фізичну реплікацію
/// (E7 прибрав і код, що читав цю політику), але параметр лишається
/// безпечним для наявних кластерів і не заважає прикладній синхронізації.
///
/// УВАГА: `ALTER SYSTEM` заборонено всередині транзакції (PostgreSQL виконує
/// multi-statement simple query в одній неявній транзакції) — тому
/// `pg_reload_conf()` виконується ОКРЕМИМ raw_sql-викликом у ensure_schema.
const WAL_POLICY_DDL: &str = r#"
ALTER SYSTEM SET max_slot_wal_keep_size = '10GB';
"#;

/// Ідемпотентне застосування схеми при старті фасаду.
///
/// - Якщо таблиці `users` немає (fresh-БД) → виконується повна схема.
/// - `owners_db`, `cash_operations`, мережевий рівень (devices/audit_log/…)
///   створюються завжди (CREATE TABLE IF NOT EXISTS) — покривають і fresh,
///   і вже мігровані БД без них.
pub async fn ensure_schema(pool: &PgPool) -> Result<(), DbError> {
    // Фаза 2.2 (ізоляція тестів): DDL бере AccessExclusiveLock навіть коли
    // змін не потрібно (`ALTER TABLE ... ADD COLUMN IF NOT EXISTS`,
    // `DROP/CREATE POLICY`). Поки один процес виконує такий DDL, будь-який
    // інший паралельний INSERT (напр. `INSERT user_stores` у setup тесту)
    // може зайти в цикл очікування з ним → `40P01 deadlock detected` у
    // тестовому прогоні. Тому: (1) усі DDL серіалізовані advisory-локом
    // (`SCHEMA_DDL_LOCK_KEY`), (2) перед застосуванням перевіряється
    // fingerprint схеми (`public.schema_revision`) — якщо він збігається,
    // DDL не виконується ВЗАГАЛІ (гарячий шлях = один SELECT у каталозі).
    //
    // Fingerprint — хеш усіх DDL-констант + `SCHEMA_REVISION_EXTRA`
    // (та частина, що генерується в рантаймі: `ensure_prro_schema`). DDL
    // змінився → fingerprint інший → DDL застосується знову (ідемпотентно).
    let mut lock_conn = pool.acquire().await.map_err(DbError::Sqlx)?;
    advisory_lock(&mut lock_conn).await?;
    let res = async {
        let fp = schema_fingerprint();
        if schema_revision_matches(pool, &fp).await? {
            return Ok(());
        }
        ensure_schema_inner(pool).await?;
        record_schema_revision(pool, &fp).await
    }
    .await;
    // Бест-ефект: знімаємо лок; якщо не вдалося — сесію закриє pool і PG
    // відпустить лок сам.
    advisory_unlock(&mut lock_conn).await;
    drop(lock_conn);
    res
}

/// Ручна складова fingerprint схеми: DDL, що генерується в рантаймі
/// (`prro::ensure_prro_schema`, бекофіл `store_id`). Змінив такий DDL —
/// підніми рядок, інакше гарячий шлях не побачить зміни.
// E9 (ADR-0008 §10 №8, варіант A): у SCHEMA_REVISION_DDL додано
// major/minor — DDL змінився → рядок піднято (інакше гарячий шлях
// не побачив би зміни на вже мігрованих БД).
const SCHEMA_REVISION_EXTRA: &str = "2026-10-02/3.5";

/// Fingerprint схеми: хеш усіх DDL-частин `ensure_schema` + ручна складова.
fn schema_fingerprint() -> String {
    let mut fp: u64 = FNV_OFFSET;
    for part in [
        SCHEMA_SQL,
        OWNERS_DB_DDL,
        CASH_OPS_DDL,
        NETWORK_DDL,
        NETWORK_NODES_DDL,
        NETWORK_EVENTS_DDL,
        STORE_LEGAL_COLUMNS_DDL,
        USERS_SYNC_STATE_DDL,
        STORE_PRICES_VERSION_DDL,
        RLS_FORCE_DDL,
        RECEIPTS_CLIENT_UUID_DDL,
        WAL_POLICY_DDL,
        SCHEMA_REVISION_EXTRA,
    ] {
        fnv1a_update(&mut fp, part.as_bytes());
        fnv1a_update(&mut fp, &[0]);
    }
    format!("{fp:016x}")
}

/// Чи застосована поточна ревізія схеми (гарячий шлях: 1 SELECT без DDL).
///
/// `to_regclass`-охорона: якщо хтось зніс таблиці, але лишив маркер —
/// маркер не дійсний (DDL мусить застосуватись знову).
async fn schema_revision_matches(pool: &PgPool, fp: &str) -> Result<bool, DbError> {
    // ТРИ окремі запити — свідомо: PG резолвить усі relations запиту на етапі
    // планування, тому `CASE WHEN to_regclass(...) IS NULL ... ELSE (SELECT …
    // FROM schema_revision …)` падає з `42P01 relation does not exist` на
    // свіжій БД (перевірено прогоном). Кожен наступний запит виконується лише
    // після підтвердження існування таблиці.
    let has_users: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(pool)
        .await
        .map_err(DbError::Sqlx)?;
    if !has_users {
        return Ok(false);
    }
    let has_marker: bool =
        sqlx::query_scalar("SELECT to_regclass('public.schema_revision') IS NOT NULL")
            .fetch_one(pool)
            .await
            .map_err(DbError::Sqlx)?;
    if !has_marker {
        return Ok(false);
    }
    let found: Option<String> =
        sqlx::query_scalar("SELECT fingerprint FROM public.schema_revision WHERE id = 1")
            .fetch_optional(pool)
            .await
            .map_err(DbError::Sqlx)?;
    Ok(found.as_deref() == Some(fp))
}

/// Записати fingerprint застосованої схеми (під тим самим advisory-локом).
async fn record_schema_revision(pool: &PgPool, fp: &str) -> Result<(), DbError> {
    sqlx::raw_sql(SCHEMA_REVISION_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    // E9: для СВІЖОЇ БД версію беремо з константи бінарника; на конфлікті
    // major/minor НЕ перезаписуємо — їх власник — міграція/оператор
    // (`schema_major()` читає саме колонку, а не константу).
    sqlx::query(
        "INSERT INTO public.schema_revision (id, fingerprint, applied_at, major, minor) \
         VALUES (1, $1, now(), $2, $3) \
         ON CONFLICT (id) DO UPDATE SET fingerprint = excluded.fingerprint, \
                                        applied_at = now()",
    )
    .bind(fp)
    .bind(crate::sync_schema::SCHEMA_MAJOR as i32)
    .bind(crate::sync_schema::SCHEMA_MINOR as i32)
    .execute(pool)
    .await
    .map_err(DbError::Sqlx)?;
    eprintln!("[torgashka-infrastructure] схема: ревізія застосована (fingerprint {fp})");
    Ok(())
}

/// `schema_revision` — службова таблиця маркера ревізії схеми (Фаза 2.2) +
/// major/minor версія схеми (E9, ADR-0008 §10 №8, варіант A — «ЗАКРИТО»).
///
/// `major`/`minor` — не «ще один fingerprint»: fingerprint відповідає на
/// питання «чи застосовано цей DDL», а major/minor — «чи сумісна схема цієї
/// БД зі схемою співрозмовника» (хаб відхиляє push із чужою major,
/// `torgashka_infrastructure::sync_schema`).
///
/// `ADD COLUMN IF NOT EXISTS` (а не UPDATE) — свідомо: значення, записане
/// міграцією/оператором, повторний DDL-прохід НЕ перетирає (ідемпотентність
/// DDL не має права скидати версію схеми назад до константи бінарника).
const SCHEMA_REVISION_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS public.schema_revision (
    id         integer PRIMARY KEY,
    fingerprint text NOT NULL,
    applied_at timestamptz NOT NULL DEFAULT now(),
    major      integer NOT NULL DEFAULT 1,
    minor      integer NOT NULL DEFAULT 0
);
ALTER TABLE public.schema_revision ADD COLUMN IF NOT EXISTS major integer NOT NULL DEFAULT 1;
ALTER TABLE public.schema_revision ADD COLUMN IF NOT EXISTS minor integer NOT NULL DEFAULT 0;
"#;

/// Зняти/взяти сесійний advisory-лок DDL (окреме з'єднання).
async fn advisory_lock(conn: &mut sqlx::PgConnection) -> Result<(), DbError> {
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SCHEMA_DDL_LOCK_KEY)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(DbError::Sqlx)
}

async fn advisory_unlock(conn: &mut sqlx::PgConnection) {
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SCHEMA_DDL_LOCK_KEY)
        .execute(&mut *conn)
        .await;
}

/// Ідемпотентний DDL, який виконується РІВНО ОДИН РАЗ на ревізію (Фаза 2.2).
///
/// Призначення: тестові та сервісні DDL-хелпери (`sync_schema::apply` у
/// `tests/common`, майбутні інтеграційні набори) мусять бути сумісні з
/// паралельними прогонами на СПІЛЬНІЙ БД. Повторне виконання
/// `ALTER TABLE`/`DROP POLICY` на кожен тест-бінар дає AccessExclusiveLock,
/// який дедлочиться з одночасними INSERT'ами тестів. Контракт:
///   * той самий advisory-лок, що `ensure_schema` (DDL глобально серіалізований);
///   * fingerprint DDL у `public.ddl_markers` → повторний виклик = 1 SELECT;
///   * повертає `true`, якщо DDL цієї ревізії застосовано цим викликом.
pub async fn ensure_ddl_once(pool: &PgPool, marker: &str, ddl: &str) -> Result<bool, DbError> {
    let fp = ddl_fingerprint(ddl);
    let mut conn = pool.acquire().await.map_err(DbError::Sqlx)?;
    advisory_lock(&mut conn).await?;
    let res = async {
        sqlx::raw_sql(DDL_MARKERS_DDL)
            .execute(&mut *conn)
            .await
            .map_err(DbError::Sqlx)?;
        let applied: Option<String> =
            sqlx::query_scalar("SELECT fingerprint FROM public.ddl_markers WHERE key = $1")
                .bind(marker)
                .fetch_optional(&mut *conn)
                .await
                .map_err(DbError::Sqlx)?;
        if applied.as_deref() == Some(fp.as_str()) {
            return Ok(false);
        }
        sqlx::raw_sql(ddl)
            .execute(&mut *conn)
            .await
            .map_err(DbError::Sqlx)?;
        sqlx::query(
            "INSERT INTO public.ddl_markers (key, fingerprint, applied_at) VALUES ($1, $2, now()) \
             ON CONFLICT (key) DO UPDATE SET fingerprint = excluded.fingerprint, \
                                            applied_at = now()",
        )
        .bind(marker)
        .bind(&fp)
        .execute(&mut *conn)
        .await
        .map_err(DbError::Sqlx)?;
        Ok(true)
    }
    .await;
    advisory_unlock(&mut conn).await;
    drop(conn);
    res
}

/// `ddl_markers` — службові мітки «DDL ревізії X застосовано» (Фаза 2.2).
const DDL_MARKERS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS public.ddl_markers (
    key        text PRIMARY KEY,
    fingerprint text NOT NULL,
    applied_at timestamptz NOT NULL DEFAULT now()
);
"#;

/// SHA-256 тексту DDL (стабільний між прогонами й машинами).
pub fn ddl_fingerprint(ddl: &str) -> String {
    let mut fp: u64 = FNV_OFFSET;
    fnv1a_update(&mut fp, ddl.as_bytes());
    format!("{fp:016x}")
}

/// База FNV-1a (64 біт) — стабільний між прогонами/машинами хеш без залежностей.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_update(state: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *state ^= u64::from(*b);
        *state = state.wrapping_mul(FNV_PRIME);
    }
}

/// Ключ advisory-лока серіалізації DDL схеми (`ensure_schema`). Довільна
/// стала: не перетинається з іншими advisory-локами проєкту.
const SCHEMA_DDL_LOCK_KEY: i64 = 0x54_4F_52_47_41_53_48; // "TORGASH"

/// Тіло `ensure_schema` під advisory-локом (див. вище).
async fn ensure_schema_inner(pool: &PgPool) -> Result<(), DbError> {
    let has_users: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(pool)
        .await
        .map_err(DbError::Sqlx)?;
    if !has_users {
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(pool)
            .await
            .map_err(DbError::Sqlx)?;
        eprintln!("[torgashka-infrastructure] схема БД застосована (fresh install: 40 таблиць)");
    }
    sqlx::raw_sql(OWNERS_DB_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(CASH_OPS_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(NETWORK_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(NETWORK_NODES_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(NETWORK_EVENTS_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(RLS_FORCE_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(RECEIPTS_CLIENT_UUID_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    // ЕТАП 20 §13: WAL-політика (10 ГБ на replication-слот). ALTER SYSTEM —
    // поза транзакцією: два окремих raw_sql (multi-statement simple query PG
    // виконав би в одній неявній транзакції). Потребує superuser БД; якщо роль
    // не має прав (42501) — попередження без зупинки старту (застосує адмін).
    match sqlx::raw_sql(WAL_POLICY_DDL).execute(pool).await {
        Ok(_) => {
            sqlx::raw_sql("SELECT pg_reload_conf();")
                .execute(pool)
                .await
                .map_err(DbError::Sqlx)?;
            eprintln!(
                "[torgashka-infrastructure] WAL-політика застосована: max_slot_wal_keep_size=10GB (ЕТАП 20 §13)"
            );
        }
        Err(e) => {
            let no_privilege = e
                .as_database_error()
                .and_then(|d| d.code().map(|c| c.as_ref() == "42501"))
                .unwrap_or(false);
            if no_privilege {
                eprintln!(
                    "[torgashka-infrastructure] WAL_POLICY пропущено: роль БД не superuser ({e}).                      Застосуйте вручну: ALTER SYSTEM SET max_slot_wal_keep_size='10GB'; SELECT pg_reload_conf();"
                );
            } else {
                return Err(DbError::Sqlx(e));
            }
        }
    }
    sqlx::raw_sql(STORE_LEGAL_COLUMNS_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    // E5 (Alembic 0024): колонки sync-шару, яких немає у знімку scripts/schema.sql.
    // Ідемпотентно і без залежностей (ні функцій, ні тригерів) — тому безпечно
    // на будь-якій БД, включно з тією, де sync-шар узагалі відсутній.
    sqlx::raw_sql(USERS_SYNC_STATE_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    sqlx::raw_sql(STORE_PRICES_VERSION_DDL)
        .execute(pool)
        .await
        .map_err(DbError::Sqlx)?;
    // «Один магазин — один ПРРО»: per-store міграція prro_settings/shifts/queue
    // (store_id + бекфіл до першого активного магазину + RLS). Ідемпотентно:
    // виконується ЗАВЖДИ при старті (fresh schema.sql уже містить store_id).
    crate::prro::ensure_prro_schema(pool)
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

// ─────────────────────────────────────────────────────────────────────────────
// Спільні SQL-хелпери провіжинінгу БД (використовуються repositories/setup.rs
// та repositories/provision.rs — без дублів).
// ─────────────────────────────────────────────────────────────────────────────

/// Quote SQL-ідентифікатора (для db_name / role у DDL; захист від ін'єкцій).
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Чи існує БД у PostgreSQL (pg_database).
pub async fn database_exists(pool: &PgPool, db_name: &str) -> Result<bool, sqlx::Error> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(db_name)
            .fetch_one(pool)
            .await?;
    Ok(exists)
}

#[cfg(test)]
mod upstream_write_pool_tests {
    use super::*;

    #[tokio::test]
    async fn empty_url_is_bad_url_and_never_connects() {
        let res = connect_upstream_write_pool("", 5).await;
        assert!(
            matches!(res, Err(DbError::BadUrl(_))),
            "порожній URL → BadUrl (жодних спроб з'єднання): {res:?}"
        );
        let res = connect_upstream_write_pool("   ", 5).await;
        assert!(matches!(res, Err(DbError::BadUrl(_))), "пробіли = порожній");
    }
}
