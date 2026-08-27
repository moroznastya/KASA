//! Ідемпотентне створення таблиць ПРРО (1:1 DDL Alembic 578fd283a156).
//! Виконується при ініціалізації репозиторію (IF NOT EXISTS — безпечно).

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

CREATE TABLE IF NOT EXISTS prro_settings (
    id          SERIAL PRIMARY KEY,
    key_name    VARCHAR(100) NOT NULL UNIQUE,
    value       TEXT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS prro_shifts (
    id                UUID PRIMARY KEY,
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
"#;

/// Ідемпотентно створює схему ПРРО (якщо таблиць ще немає).
pub async fn ensure_prro_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(DDL).execute(pool).await?;
    Ok(())
}
