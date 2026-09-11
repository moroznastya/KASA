//! Ідемпотентне створення таблиць ПРРО (1:1 DDL Alembic 578fd283a156).
//! Виконується при ініціалізації репозиторію (IF NOT EXISTS — безпечно).
//!
//! ── Per-store модель «Один магазин — один ПРРО» ─────────────────────────────
//! Раніше prro_settings / prro_shifts / prro_queue_items були ЄДИНИМИ
//! мультиточковими таблицями БЕЗ store_id і БЕЗ RLS (аномалія, задокументована
//! в admin_prro.rs Етапа 5). Цей DDL ідемпотентно переводить їх на per-store:
//!   • додає store_id uuid NOT NULL REFERENCES stores(id) (+ FK, якщо stores є);
//!   • переносить ІСНУЮЧІ глобальні рядки першому активному магазину
//!     (SELECT ... WHERE is_active=true ORDER BY created_at, id LIMIT 1);
//!   • settings: UNIQUE(key_name) → UNIQUE(store_id, key_name);
//!   • індекси (store_id, opened_at) / (store_id, status, created_at);
//!   • ENABLE ROW LEVEL SECURITY + POLICY ..._store_isolation (шаблон schema.sql).
//! Якщо stores ще немає (тільки тестовий сценарій ensure_prro_schema без повної
//! схеми) — FK/бекфіл пропускаються: у продакшені ensure_schema створює stores
//! раніше (setup виконується до ПРРО), тож магазин гарантовано є.

use sqlx::PgPool;

const DDL: &str = r#"
-- enum-типи (1:1 Alembic prro_shift_status / prro_queue_status)
DO $$ BEGIN
    CREATE TYPE prro_shift_status AS ENUM ('open', 'closed');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
DO $$ BEGIN
    CREATE TYPE prro_queue_status AS ENUM ('pending', 'sent', 'failed');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- Fresh-install: нові таблиці ОДРАЗУ з store_id (schema.sql має те саме).
CREATE TABLE IF NOT EXISTS prro_settings (
    id          SERIAL PRIMARY KEY,
    store_id    UUID NOT NULL,
    key_name    VARCHAR(100) NOT NULL,
    value       TEXT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS prro_shifts (
    id                UUID PRIMARY KEY,
    store_id          UUID NOT NULL,
    shift_number      INTEGER NOT NULL,
    opened_at         TIMESTAMPTZ NOT NULL,
    closed_at         TIMESTAMPTZ,
    signer_serial     VARCHAR(255),
    signer_name       VARCHAR(255),
    closed_by         VARCHAR(255),
    zreport_number    VARCHAR(50),
    status            prro_shift_status NOT NULL DEFAULT 'open',
    receipt_count     INTEGER NOT NULL DEFAULT 0,
    total_amount      NUMERIC(12,2) NOT NULL DEFAULT 0,
    last_local_number INTEGER NOT NULL DEFAULT 0,
    last_mac          TEXT
);
CREATE INDEX IF NOT EXISTS ix_prro_shifts_shift_number ON prro_shifts (shift_number);

CREATE TABLE IF NOT EXISTS prro_queue_items (
    id            UUID PRIMARY KEY,
    store_id      UUID NOT NULL,
    receipt_id    UUID REFERENCES receipts(id) ON DELETE SET NULL,
    shift_id      UUID REFERENCES prro_shifts(id) ON DELETE SET NULL,
    local_number  INTEGER NOT NULL,
    check_type    VARCHAR(10) NOT NULL,
    xml_body      TEXT NOT NULL,
    mac           TEXT,
    status        prro_queue_status NOT NULL DEFAULT 'pending',
    error         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at       TIMESTAMPTZ
);
-- B2: повний підписаний check_sign (ідемпотентність sync). Ідемпотентно для
-- вже існуючих БД (CREATE TABLE IF NOT EXISTS не додає колонку).
ALTER TABLE prro_queue_items ADD COLUMN IF NOT EXISTS check_sign TEXT;
-- B4: офлайн-ідентифікатор offline-чека.
ALTER TABLE prro_queue_items ADD COLUMN IF NOT EXISTS id_offline TEXT;
CREATE INDEX IF NOT EXISTS ix_prro_queue_items_receipt_id ON prro_queue_items (receipt_id);
CREATE INDEX IF NOT EXISTS ix_prro_queue_items_shift_id ON prro_queue_items (shift_id);
CREATE INDEX IF NOT EXISTS ix_prro_queue_items_status ON prro_queue_items (status);

-- ─── Міграція існуючих БД: per-store (ідемпотентно) ────────────────────────

-- 1. Колонки store_id (nullable на цьому кроці — далі бекфіл + NOT NULL).
ALTER TABLE prro_settings ADD COLUMN IF NOT EXISTS store_id UUID;
ALTER TABLE prro_shifts   ADD COLUMN IF NOT EXISTS store_id UUID;
ALTER TABLE prro_queue_items ADD COLUMN IF NOT EXISTS store_id UUID;

-- 2. FK → stores(id) ON DELETE CASCADE (лише якщо stores існує й FK ще немає).
DO $$
BEGIN
    IF to_regclass('public.stores') IS NOT NULL THEN
        IF NOT EXISTS (SELECT 1 FROM pg_constraint c
                       WHERE c.conname = 'prro_settings_store_id_fkey'
                         AND c.conrelid = 'prro_settings'::regclass) THEN
            ALTER TABLE prro_settings ADD CONSTRAINT prro_settings_store_id_fkey
                FOREIGN KEY (store_id) REFERENCES stores(id) ON DELETE CASCADE;
        END IF;
        IF NOT EXISTS (SELECT 1 FROM pg_constraint c
                       WHERE c.conname = 'prro_shifts_store_id_fkey'
                         AND c.conrelid = 'prro_shifts'::regclass) THEN
            ALTER TABLE prro_shifts ADD CONSTRAINT prro_shifts_store_id_fkey
                FOREIGN KEY (store_id) REFERENCES stores(id) ON DELETE CASCADE;
        END IF;
        IF NOT EXISTS (SELECT 1 FROM pg_constraint c
                       WHERE c.conname = 'prro_queue_items_store_id_fkey'
                         AND c.conrelid = 'prro_queue_items'::regclass) THEN
            ALTER TABLE prro_queue_items ADD CONSTRAINT prro_queue_items_store_id_fkey
                FOREIGN KEY (store_id) REFERENCES stores(id) ON DELETE CASCADE;
        END IF;
    END IF;
END
$$;

-- 3. Перенесення глобальних рядків (store_id IS NULL) першому активному
--    магазину. Правило «перший активний» збігається з адмін-міграцією §9
--    (admin_migrate.rs): ORDER BY created_at, id LIMIT 1.
--    Guard: stores може бути відсутня лише в ізольованому тестовому
--    ensure_prro_schema без повної схеми (у продакшені ensure_schema
--    створює stores раніше).
DO $$
BEGIN
    IF to_regclass('public.stores') IS NOT NULL THEN
        UPDATE prro_settings s SET store_id = (
            SELECT st.id FROM stores st
            WHERE st.is_active = true
            ORDER BY st.created_at, st.id LIMIT 1
        ) WHERE s.store_id IS NULL;
        UPDATE prro_shifts s SET store_id = (
            SELECT st.id FROM stores st
            WHERE st.is_active = true
            ORDER BY st.created_at, st.id LIMIT 1
        ) WHERE s.store_id IS NULL;
        UPDATE prro_queue_items s SET store_id = (
            SELECT st.id FROM stores st
            WHERE st.is_active = true
            ORDER BY st.created_at, st.id LIMIT 1
        ) WHERE s.store_id IS NULL;
    END IF;
END
$$;

-- 4. NOT NULL — лише коли рядків без store_id не лишилось (якщо stores ще
--    порожня/відсутня на етапі тесту — колонка лишається nullable до створення
--    першого магазину; у продакшені ensure_schema створює stores раніше).
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM prro_settings WHERE store_id IS NULL) THEN
        ALTER TABLE prro_settings ALTER COLUMN store_id SET NOT NULL;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM prro_shifts WHERE store_id IS NULL) THEN
        ALTER TABLE prro_shifts ALTER COLUMN store_id SET NOT NULL;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM prro_queue_items WHERE store_id IS NULL) THEN
        ALTER TABLE prro_queue_items ALTER COLUMN store_id SET NOT NULL;
    END IF;
END
$$;

-- 5. Ключі settings: UNIQUE(key_name) → UNIQUE(store_id, key_name).
--    Залежно від історії БД обмеження могло бути табличним (CREATE TABLE ...
--    UNIQUE у цьому DDL) або окремим індексом (schema.sql ix_prro_settings_key_name).
ALTER TABLE prro_settings DROP CONSTRAINT IF EXISTS prro_settings_key_name_key;
DROP INDEX IF EXISTS ix_prro_settings_key_name;
CREATE UNIQUE INDEX IF NOT EXISTS ux_prro_settings_store_key
    ON prro_settings (store_id, key_name);

-- 6. Індекси доступу за точкою.
CREATE INDEX IF NOT EXISTS ix_prro_shifts_store_opened
    ON prro_shifts (store_id, opened_at);
CREATE INDEX IF NOT EXISTS ix_prro_queue_items_store_status
    ON prro_queue_items (store_id, status, created_at);

-- 7. Row-Level Security (шаблон schema.sql, рядки 1041+): policy ..._store_isolation.
ALTER TABLE prro_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE prro_shifts ENABLE ROW LEVEL SECURITY;
ALTER TABLE prro_queue_items ENABLE ROW LEVEL SECURITY;

DO $$
BEGIN
    IF to_regclass('public.user_stores') IS NOT NULL THEN
        IF NOT EXISTS (SELECT 1 FROM pg_policies
                       WHERE schemaname = 'public' AND tablename = 'prro_settings'
                         AND policyname = 'prro_settings_store_isolation') THEN
            EXECUTE $pol$
                CREATE POLICY prro_settings_store_isolation ON public.prro_settings
                USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid)
                        OR (store_id IN (SELECT user_stores.store_id FROM public.user_stores
                                         WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))))
                WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid)
                             OR (store_id IN (SELECT user_stores.store_id FROM public.user_stores
                                              WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));
            $pol$;
        END IF;
        IF NOT EXISTS (SELECT 1 FROM pg_policies
                       WHERE schemaname = 'public' AND tablename = 'prro_shifts'
                         AND policyname = 'prro_shifts_store_isolation') THEN
            EXECUTE $pol$
                CREATE POLICY prro_shifts_store_isolation ON public.prro_shifts
                USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid)
                        OR (store_id IN (SELECT user_stores.store_id FROM public.user_stores
                                         WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))))
                WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid)
                             OR (store_id IN (SELECT user_stores.store_id FROM public.user_stores
                                              WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));
            $pol$;
        END IF;
        IF NOT EXISTS (SELECT 1 FROM pg_policies
                       WHERE schemaname = 'public' AND tablename = 'prro_queue_items'
                         AND policyname = 'prro_queue_items_store_isolation') THEN
            EXECUTE $pol$
                CREATE POLICY prro_queue_items_store_isolation ON public.prro_queue_items
                USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid)
                        OR (store_id IN (SELECT user_stores.store_id FROM public.user_stores
                                         WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))))
                WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid)
                             OR (store_id IN (SELECT user_stores.store_id FROM public.user_stores
                                              WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));
            $pol$;
        END IF;
    END IF;
END
$$;
"#;

/// Ідемпотентно створює схему ПРРО (якщо таблиць ще немає) і застосовує
/// per-store міграцію (додавання store_id, перенесення даних, RLS) —
/// безпечно для fresh та вже ініціалізованих БД.
pub async fn ensure_prro_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(DDL).execute(pool).await?;
    Ok(())
}
