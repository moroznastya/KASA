"""ЕТАП 3 (offline-first): серверна схема синхронізації — pull майстер-даних.

Реалізує серверну (PostgreSQL) частину offline-first синку за
docs/design/sync-schema-design.md, розділи 1 та 8.2:

  1. sync_meta        — версії майстер-даних (entity text PK → version BIGINT
                        DEFAULT 0) + початкові рядки: categories, products,
                        stock_norms, suppliers, employees, settings.
  2. sync_log         — журнал pull/push (аудит + SLA): store_id FK→stores,
                        direction CHECK (pull|push), status CHECK
                        (ok|error|already_exists), payload_hash, created_at
                        timestamptz; індекси (store_id, created_at DESC) та
                        (status, created_at).
  3. stock_projection — серверна проєкція залишків per store+product
                        (quantity NUMERIC(10,3), updated_at timestamptz),
                        PK (store_id, product_id).
  4. Soft-delete: products/categories/suppliers/users.is_deleted
     boolean NOT NULL DEFAULT false.
     updated_at ВЖЕ існує на всіх чотирьох таблицях (NOT NULL DEFAULT now())
     — не дублюється (звірено з реальною схемою на head b2b4).
  5. bump_sync_version() + тригери AFTER INSERT OR UPDATE OR DELETE
     FOR EACH ROW:
       products        → 'products'
       categories      → 'categories'
       suppliers       → 'suppliers'
       users           → 'employees'   (мапінг дизайну: таблиця users =
                                         сутність employees)
       system_settings → 'settings'
     Таблиці stock_norms НЕМАЄ в реальній схемі (перевірено: жодної моделі
     чи міграції) → sync_meta рядок 'stock_norms' сіється за дизайном, але
     тригер не створюється.
  6. RLS на stock_projection: ENABLE ROW LEVEL SECURITY + політика
     stock_projection_store_isolation (дзеркало stock_store_isolation з
     0004_rls: store_id = current_setting('app.store_id') АБО точка з
     user_stores користувача).

Принцип дизайну (розділ 1.3): інкремент версії відбувається в тій самій
транзакції, що й зміна даних — тригер виконує UPDATE sync_meta разом із
DML, що його викликав. sync_meta НЕ покривається RLS (версії глобальні
для всіх точок; каса зберігає власний since_version локально).

Revision ID: 0011_sync_server_schema
Revises: b2b4_prro_queue_sign
Create Date: 2026-09-02
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
import sqlalchemy.dialects.postgresql as postgresql


# revision identifiers, used by Alembic.
revision: str = "0011_sync_server_schema"
down_revision: Union[str, Sequence[str], None] = "b2b4_prro_queue_sign"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


# (таблиця, sync_meta entity) — тригери bump за дизайном 8.2.
# stock_norms відсутня в реальній схемі — тригер не створюється.
BUMP_TRIGGERS: list[tuple[str, str]] = [
    ("products", "products"),
    ("categories", "categories"),
    ("suppliers", "suppliers"),
    ("users", "employees"),
    ("system_settings", "settings"),
]

# Таблиці, що отримують soft-delete колонку (updated_at вже існує всюди).
SOFT_DELETE_TABLES: list[str] = ["products", "categories", "suppliers", "users"]


def upgrade() -> None:
    """Серверна схема синку: sync_meta, sync_log, stock_projection,
    soft-delete колонки, bump-тригери, RLS на stock_projection."""

    # ── 1. sync_meta: версії майстер-даних ────────────────────────────────
    op.create_table(
        "sync_meta",
        sa.Column("entity", sa.Text(), primary_key=True),
        sa.Column(
            "version",
            sa.BigInteger(),
            nullable=False,
            server_default=sa.text("0"),
        ),
    )
    # Початкові рядки (розділ 8.2): версії всіх сутностей майстер-даних.
    op.execute(
        "INSERT INTO sync_meta (entity) VALUES "
        "('categories'), ('products'), ('stock_norms'), "
        "('suppliers'), ('employees'), ('settings')"
    )

    # ── 2. sync_log: журнал синхронізації (аудит + SLA) ───────────────────
    op.create_table(
        "sync_log",
        sa.Column("id", sa.BigInteger(), primary_key=True, autoincrement=True),
        sa.Column("store_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("direction", sa.String(8), nullable=False),
        sa.Column("entity", sa.String(32), nullable=False),
        sa.Column("client_uuid", postgresql.UUID(as_uuid=True), nullable=True),
        sa.Column("status", sa.String(16), nullable=False),
        sa.Column("payload_hash", sa.Text(), nullable=True),
        sa.Column("error", sa.Text(), nullable=True),
        sa.Column(
            "created_at",
            sa.TIMESTAMP(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
        sa.ForeignKeyConstraint(
            ["store_id"], ["stores.id"],
            ondelete="CASCADE",
            name="sync_log_store_id_fkey",
        ),
        sa.CheckConstraint(
            "direction IN ('pull','push')",
            name="sync_log_direction_check",
        ),
        sa.CheckConstraint(
            "status IN ('ok','error','already_exists')",
            name="sync_log_status_check",
        ),
    )
    op.execute("CREATE INDEX ix_sync_log_store ON sync_log (store_id, created_at DESC)")
    op.execute("CREATE INDEX ix_sync_log_status ON sync_log (status, created_at)")

    # ── 3. stock_projection: серверна проєкція залишків ───────────────────
    op.create_table(
        "stock_projection",
        sa.Column("store_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column(
            "quantity",
            sa.Numeric(10, 3),
            nullable=False,
            server_default=sa.text("0"),
        ),
        sa.Column(
            "updated_at",
            sa.TIMESTAMP(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
        sa.ForeignKeyConstraint(
            ["store_id"], ["stores.id"], ondelete="CASCADE"
        ),
        sa.ForeignKeyConstraint(
            ["product_id"], ["products.id"], ondelete="CASCADE"
        ),
        sa.PrimaryKeyConstraint("store_id", "product_id"),
    )

    # ── 4. RLS на stock_projection (дзеркало stock_store_isolation) ───────
    op.execute("ALTER TABLE stock_projection ENABLE ROW LEVEL SECURITY")
    op.execute("DROP POLICY IF EXISTS stock_projection_store_isolation ON stock_projection")
    op.execute(
        """
        CREATE POLICY stock_projection_store_isolation ON stock_projection
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
        )
        """
    )

    # ── 5. Soft-delete колонки is_deleted ─────────────────────────────────
    # updated_at на всіх чотирьох таблицях уже існує (NOT NULL DEFAULT now()) —
    # додаємо ТІЛЬКИ is_deleted (дизайн використовує ADD COLUMN IF NOT EXISTS).
    for table in SOFT_DELETE_TABLES:
        op.execute(
            f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS "
            "is_deleted boolean NOT NULL DEFAULT false"
        )

    # ── 6. bump_sync_version() + тригери інкременту версій ────────────────
    op.execute(
        """
        CREATE OR REPLACE FUNCTION bump_sync_version() RETURNS trigger AS $$
        BEGIN
            UPDATE sync_meta SET version = version + 1 WHERE entity = TG_ARGV[0];
            RETURN NEW;
        END; $$ LANGUAGE plpgsql
        """
    )
    for table, entity in BUMP_TRIGGERS:
        op.execute(
            f"""
            CREATE TRIGGER trg_{table}_bump
            AFTER INSERT OR UPDATE OR DELETE ON {table}
            FOR EACH ROW EXECUTE FUNCTION bump_sync_version('{entity}')
            """
        )


def downgrade() -> None:
    """Видалити серверну схему синку (тригери → функцію → колонки → таблиці)."""
    for table, _entity in BUMP_TRIGGERS:
        op.execute(f"DROP TRIGGER IF EXISTS trg_{table}_bump ON {table}")
    op.execute("DROP FUNCTION IF EXISTS bump_sync_version()")

    for table in SOFT_DELETE_TABLES:
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS is_deleted")

    # RLS-політика та індекси зникають разом із таблицями.
    op.execute("DROP TABLE IF EXISTS stock_projection")
    op.execute("DROP TABLE IF EXISTS sync_log")
    op.execute("DROP TABLE IF EXISTS sync_meta")
