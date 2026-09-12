//! Sync-шар схеми PostgreSQL (Alembic 0011–0014) поверх `ensure_schema`.
//!
//! `ensure_schema` (torgashka-infrastructure) на порожній БД виконує
//! `schema.sql` — БАЗОВУ схему БЕЗ sync-механізму offline-first:
//!   * `sync_meta` / `sync_log` / `stock_projection` — таблиці немає;
//!   * `server_version` колонки + BEFORE-тригери bump — немає;
//!   * soft-delete `is_deleted` на довідниках — немає;
//!   * `client_uuid` + partial UNIQUE на приймачах push — немає.
//!
//! У проді цей шар додає Alembic (backend, міграції 0011–0014). Rust-тести
//! не можуть запускати alembic → sync-e2e відтворюють ФІНАЛЬНИЙ стан
//! sync-шару цим хелпером (ідемпотентно, IF NOT EXISTS / OR REPLACE) —
//! самодостатність на свіжій порожній БД (drop+create → ensure_schema →
//! sync_schema::apply).
//!
//! Підключається через `#[path = "common/sync_schema.rs"] mod sync_schema;`
//! (common/mod.rs не чіпається — він лише force_test_db).

/// Фінальний стан Alembic 0011 (sync_meta/sync_log/stock_projection/soft-delete)
/// + 0012 (server_version + BEFORE bump) + 0013 (client_uuid на приймачах)
/// + 0014 (drop kasa-спадку client_receipt_uuid) + 0019 (батьківські kinds)
/// + 0020 (sync_batches + sync_log.batch_id/error_class, етап E2a)
/// + 0021 (hub_forwarded_at/hub_forward_status + hub_outbox, етап E3)
/// + 0022 (catalog_change_requests — журнал пропозицій довідників, етап E5)
/// + 0023 (E5 «частина A»: barcodes/product_images/write_off_reasons —
///   server_version + тригери + sync_meta; write_off_reasons/system_settings
///   is_deleted; stock_norms прибрано з sync_meta — §7.1-C1/C2/C5/C6)
/// + 0024 (E5 «частина B»: store_product_prices — server_version + тригер +
///   sync_meta + tombstone, §7.1-B5/C3; users.sync_state — локальний маркер
///   вузла, §7.1-D3).
const SYNC_DDL: &str = r##"
CREATE TABLE IF NOT EXISTS public.sync_meta (
    entity text PRIMARY KEY,
    version bigint NOT NULL DEFAULT 0
);
INSERT INTO sync_meta (entity) VALUES
    ('categories'), ('products'),
    ('suppliers'), ('employees'), ('settings'),
    -- 0023 (E5-C1/C2): нові довідники, придатні для pull.
    ('barcodes'), ('product_images'), ('write_off_reasons'),
    -- 0024 (E5-B5/C3): ціна точки — спільна сутність мережі (рішення Б1).
    ('store_product_prices')
ON CONFLICT (entity) DO NOTHING;
-- 0023 (E5-C6): stock_norms прибрано — таблиці в серверній схемі немає.
-- DELETE (а не лише відсутність у seed): тестова БД переживає прогони.
DELETE FROM sync_meta WHERE entity = 'stock_norms';

CREATE TABLE IF NOT EXISTS public.sync_log (
    id bigserial PRIMARY KEY,
    store_id uuid NOT NULL REFERENCES public.stores(id) ON DELETE CASCADE,
    direction varchar(8) NOT NULL CHECK (direction IN ('pull','push')),
    entity varchar(32) NOT NULL,
    client_uuid uuid,
    status varchar(16) NOT NULL CHECK (status IN ('ok','error','already_exists')),
    payload_hash text,
    error text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS ix_sync_log_store ON sync_log (store_id, created_at DESC);
CREATE INDEX IF NOT EXISTS ix_sync_log_status ON sync_log (status, created_at);

-- 0020 (ADR-0008 §7.1-A2, етап E2a): батчі push — ідентичність пакета
-- (`sync_batches` + `sync_log.batch_id`) і машинний клас помилки агрегата
-- (`sync_log.error_class`, §7.1-E1). Дзеркало Alembic 0020_sync_batches.
ALTER TABLE public.sync_log ADD COLUMN IF NOT EXISTS batch_id uuid;
ALTER TABLE public.sync_log ADD COLUMN IF NOT EXISTS error_class text;
CREATE TABLE IF NOT EXISTS public.sync_batches (
    id uuid PRIMARY KEY,
    store_id uuid NOT NULL,
    node_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    items int NOT NULL,
    status text NOT NULL CHECK (status IN ('accepted','partial','failed'))
);
CREATE INDEX IF NOT EXISTS ix_sync_batches_store
    ON sync_batches (store_id, created_at DESC);
CREATE INDEX IF NOT EXISTS ix_sync_log_batch
    ON sync_log (batch_id) WHERE batch_id IS NOT NULL;

-- 0021 (ADR-0008 §7.1-A1, етап E3): форвардинг прийнятого вузлом у хаб —
-- стан передачі в журналі прийому + черга `hub_outbox`. Дзеркало Alembic
-- 0021_hub_forwarding.
ALTER TABLE public.sync_log ADD COLUMN IF NOT EXISTS hub_forwarded_at timestamptz;
ALTER TABLE public.sync_log ADD COLUMN IF NOT EXISTS hub_forward_status text;
CREATE INDEX IF NOT EXISTS ix_sync_log_hub_pending
    ON sync_log (store_id, hub_forwarded_at) WHERE hub_forwarded_at IS NULL;
CREATE TABLE IF NOT EXISTS public.hub_outbox (
    id bigserial PRIMARY KEY,
    store_id uuid NOT NULL REFERENCES public.stores(id) ON DELETE CASCADE,
    entity text NOT NULL,
    client_uuid uuid NOT NULL,
    batch_id uuid,
    envelope jsonb NOT NULL,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','done','failed')),
    attempts int NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    forwarded_at timestamptz,
    forward_status text
        CHECK (forward_status IS NULL OR forward_status IN ('accepted','failed')),
    error text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_hub_outbox_store_client_entity
    ON hub_outbox (store_id, client_uuid, entity);
CREATE INDEX IF NOT EXISTS ix_hub_outbox_pending
    ON hub_outbox (status, next_attempt_at, id);

-- 0022 (ADR-0008 §7.1-D1/D2, етап E5): журнал ПРОПОЗИЦІЙ спільних довідників
-- (хаб — авторитет; вузол пропонує, хаб присвоює єдиний server_version).
-- Дзеркало Alembic 0022_catalog_change_requests.
CREATE TABLE IF NOT EXISTS public.catalog_change_requests (
    id bigserial PRIMARY KEY,
    entity text NOT NULL,
    row_id uuid NOT NULL,
    op text NOT NULL CHECK (op IN ('upsert','delete')),
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    client_uuid uuid NOT NULL,
    store_id uuid REFERENCES public.stores(id) ON DELETE SET NULL,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','accepted','conflict','rejected')),
    server_version bigint,
    base_version bigint NOT NULL DEFAULT 0,
    priority integer,
    error text,
    decided_by uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    decided_at timestamptz
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_catalog_change_requests_client_uuid
    ON catalog_change_requests (client_uuid);
CREATE INDEX IF NOT EXISTS ix_catalog_change_requests_queue
    ON catalog_change_requests (status, created_at);
CREATE INDEX IF NOT EXISTS ix_catalog_change_requests_row
    ON catalog_change_requests (entity, row_id, status);

CREATE TABLE IF NOT EXISTS public.stock_projection (
    store_id uuid NOT NULL REFERENCES public.stores(id) ON DELETE CASCADE,
    product_id uuid NOT NULL REFERENCES public.products(id) ON DELETE CASCADE,
    quantity numeric(10,3) NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (store_id, product_id)
);
ALTER TABLE public.stock_projection ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS stock_projection_store_isolation ON public.stock_projection;
CREATE POLICY stock_projection_store_isolation ON public.stock_projection
    USING (
        store_id = NULLIF(current_setting('app.store_id', true), '')::uuid
        OR store_id IN (
            SELECT store_id FROM user_stores
            WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid
        )
    )
    WITH CHECK (
        store_id = NULLIF(current_setting('app.store_id', true), '')::uuid
        OR store_id IN (
            SELECT store_id FROM user_stores
            WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid
        )
    );

ALTER TABLE products   ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
ALTER TABLE categories ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
ALTER TABLE suppliers  ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
ALTER TABLE users      ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;

ALTER TABLE products        ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE categories      ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE suppliers       ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE users           ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE system_settings ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;

CREATE OR REPLACE FUNCTION bump_sync_version() RETURNS trigger AS $$
DECLARE
    new_ver bigint;
BEGIN
    UPDATE sync_meta SET version = version + 1
    WHERE entity = TG_ARGV[0]
    RETURNING version INTO new_ver;

    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        NEW.server_version := new_ver;
        RETURN NEW;
    END IF;
    RETURN OLD;
END; $$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_products_bump ON products;
CREATE TRIGGER trg_products_bump BEFORE INSERT OR UPDATE OR DELETE ON products
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('products');
DROP TRIGGER IF EXISTS trg_categories_bump ON categories;
CREATE TRIGGER trg_categories_bump BEFORE INSERT OR UPDATE OR DELETE ON categories
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('categories');
DROP TRIGGER IF EXISTS trg_suppliers_bump ON suppliers;
CREATE TRIGGER trg_suppliers_bump BEFORE INSERT OR UPDATE OR DELETE ON suppliers
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('suppliers');
DROP TRIGGER IF EXISTS trg_users_bump ON users;
CREATE TRIGGER trg_users_bump BEFORE INSERT OR UPDATE OR DELETE ON users
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('employees');
DROP TRIGGER IF EXISTS trg_system_settings_bump ON system_settings;
CREATE TRIGGER trg_system_settings_bump BEFORE INSERT OR UPDATE OR DELETE ON system_settings
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('settings');

-- ── 0023 (E5 «частина A», ADR-0008 §7.1-C): довідники, придатні для pull ────
-- C1: barcodes, product_images — server_version + тригер bump.
ALTER TABLE barcodes       ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE product_images ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
-- C2: write_off_reasons — версія + tombstone (pull віддає op=delete).
ALTER TABLE write_off_reasons ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE write_off_reasons ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
-- C5: system_settings.is_deleted — інакше pull завжди видає upsert.
ALTER TABLE system_settings ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;

DROP TRIGGER IF EXISTS trg_barcodes_bump ON barcodes;
CREATE TRIGGER trg_barcodes_bump BEFORE INSERT OR UPDATE OR DELETE ON barcodes
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('barcodes');
DROP TRIGGER IF EXISTS trg_product_images_bump ON product_images;
CREATE TRIGGER trg_product_images_bump BEFORE INSERT OR UPDATE OR DELETE ON product_images
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('product_images');
DROP TRIGGER IF EXISTS trg_write_off_reasons_bump ON write_off_reasons;
CREATE TRIGGER trg_write_off_reasons_bump BEFORE INSERT OR UPDATE OR DELETE ON write_off_reasons
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('write_off_reasons');

-- ── 0024 (E5 «частина B», ADR-0008 §7.1-B5/C3/D3): ціна точки як СПІЛЬНА
-- сутність мережі (рішення Б1: хаб арбітрує єдину версію, роздає всім) +
-- локальний маркер касира. Дзеркало Alembic 0024. ────────────────────────────
-- C3/B5: store_product_prices — версія + tombstone («перевизначення знято»).
ALTER TABLE store_product_prices ADD COLUMN IF NOT EXISTS server_version bigint NOT NULL DEFAULT 0;
ALTER TABLE store_product_prices ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
DROP TRIGGER IF EXISTS trg_store_product_prices_bump ON store_product_prices;
CREATE TRIGGER trg_store_product_prices_bump BEFORE INSERT OR UPDATE OR DELETE ON store_product_prices
    FOR EACH ROW EXECUTE FUNCTION bump_sync_version('store_product_prices');
-- D3: users.sync_state — «локально створене, ще не підтверджене» (offline-first).
ALTER TABLE users ADD COLUMN IF NOT EXISTS sync_state text NOT NULL DEFAULT 'confirmed';
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'users_sync_state_check') THEN
        ALTER TABLE users ADD CONSTRAINT users_sync_state_check
            CHECK (sync_state IN ('local','pending_hub','confirmed'));
    END IF;
END $$;

ALTER TABLE receipts        ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE return_invoices ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE purchase_orders ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE inventories     ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE transfers       ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE write_offs      ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE debtor_payments ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE work_sessions   ADD COLUMN IF NOT EXISTS client_uuid uuid;
-- 0016 (invoice push idempotency): дзеркало Alembic 0016 для тестової БД
-- (schema.sql її не має) — partial UNIQUE, як на проді.
ALTER TABLE invoices        ADD COLUMN IF NOT EXISTS client_uuid uuid;
-- 0017 (cash operation push idempotency): дзеркало Alembic 0017.
ALTER TABLE cash_operations ADD COLUMN IF NOT EXISTS client_uuid uuid;
-- 0019 (ADR-0008 §7.1-B, етап E1): батьківські сутності вузла — боржник і
-- ПРРО-зміна. Дзеркало Alembic 0019_peer_parents_push_kinds.
ALTER TABLE debtors     ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE prro_shifts ADD COLUMN IF NOT EXISTS client_uuid uuid;

DROP INDEX IF EXISTS uq_receipts_client_uuid;
CREATE UNIQUE INDEX uq_receipts_client_uuid ON receipts (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_return_invoices_client_uuid;
CREATE UNIQUE INDEX uq_return_invoices_client_uuid ON return_invoices (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_purchase_orders_client_uuid;
CREATE UNIQUE INDEX uq_purchase_orders_client_uuid ON purchase_orders (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_inventories_client_uuid;
CREATE UNIQUE INDEX uq_inventories_client_uuid ON inventories (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_transfers_client_uuid;
CREATE UNIQUE INDEX uq_transfers_client_uuid ON transfers (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_write_offs_client_uuid;
CREATE UNIQUE INDEX uq_write_offs_client_uuid ON write_offs (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_debtor_payments_client_uuid;
CREATE UNIQUE INDEX uq_debtor_payments_client_uuid ON debtor_payments (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_work_sessions_client_uuid;
CREATE UNIQUE INDEX uq_work_sessions_client_uuid ON work_sessions (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_invoices_client_uuid;
CREATE UNIQUE INDEX uq_invoices_client_uuid ON invoices (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_cash_operations_client_uuid;
CREATE UNIQUE INDEX uq_cash_operations_client_uuid ON cash_operations (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_debtors_client_uuid;
CREATE UNIQUE INDEX uq_debtors_client_uuid ON debtors (client_uuid) WHERE client_uuid IS NOT NULL;
DROP INDEX IF EXISTS uq_prro_shifts_client_uuid;
CREATE UNIQUE INDEX uq_prro_shifts_client_uuid ON prro_shifts (client_uuid) WHERE client_uuid IS NOT NULL;

DROP INDEX IF EXISTS uq_receipts_client_receipt_uuid;
ALTER TABLE receipts DROP COLUMN IF EXISTS client_receipt_uuid;
"##;

/// Застосувати sync-шар схеми (ідемпотентно). Викликається ПІСЛЯ ensure_schema.
///
/// Фаза 2.2 (ізоляція тестів): DDL іде через `db::ensure_ddl_once` — під тим
/// самим advisory-локом, що `ensure_schema`, і з fingerprint-маркером
/// (`public.ddl_markers`). Наслідок: `ALTER TABLE`/`DROP POLICY` виконуються
/// РІВНО ОДИН РАЗ на ревізію DDL на всю тестову БД, а не в кожному
/// тест-бінарі — саме ці AccessExclusiveLock'и дедлочились із паралельними
/// INSERT'ами setup'ів (`40P01`) у попередніх прогонах.
pub async fn apply(pool: &sqlx::PgPool) {
    torgashka_infrastructure::db::ensure_ddl_once(pool, "test_sync_schema", SYNC_DDL)
        .await
        .expect("sync-шар схеми (Alembic 0011-0014) на тестовій БД");
}
