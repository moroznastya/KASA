//! Sync-шар схеми PostgreSQL (Alembic 0011–0014) поверх `ensure_schema`.
//!
//! `ensure_schema` (torgashka-infrastructure) на порожній БД виконує
//! `schema.sql` — БАЗОВУ схему БЕЗ sync-механізму offline-first:
//!   * `sync_meta` / `sync_log` / `stock_projection` — таблиці немає;
//!   * `server_version` колонки + BEFORE-тригери bump — немає;
//!   * soft-delete `is_deleted` на довідниках — немає;
//!   * `client_uuid` + partial UNIQUE на приймачах push — немає.
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
/// + 0014 (drop kasa-спадку client_receipt_uuid).
const SYNC_DDL: &str = r##"
CREATE TABLE IF NOT EXISTS public.sync_meta (
    entity text PRIMARY KEY,
    version bigint NOT NULL DEFAULT 0
);
INSERT INTO sync_meta (entity) VALUES
    ('categories'), ('products'), ('stock_norms'),
    ('suppliers'), ('employees'), ('settings')
ON CONFLICT (entity) DO NOTHING;

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

ALTER TABLE receipts        ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE return_invoices ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE purchase_orders ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE inventories     ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE transfers       ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE write_offs      ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE debtor_payments ADD COLUMN IF NOT EXISTS client_uuid uuid;
ALTER TABLE work_sessions   ADD COLUMN IF NOT EXISTS client_uuid uuid;

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

DROP INDEX IF EXISTS uq_receipts_client_receipt_uuid;
ALTER TABLE receipts DROP COLUMN IF EXISTS client_receipt_uuid;
"##;

/// Застосувати sync-шар схеми (ідемпотентно). Викликається ПІСЛЯ ensure_schema.
pub async fn apply(pool: &sqlx::PgPool) {
    sqlx::raw_sql(SYNC_DDL)
        .execute(pool)
        .await
        .expect("sync-шар схеми (Alembic 0011-0014) на тестовій БД");
}
