"""ЕТАП 3 (offline-first): per-row server_version для pull майстер-даних.

Механізм (a) дизайну (sync-schema-design.md, розділ 1.4 «кожен change має
version»): кожен рядок довідника отримує колонку server_version BIGINT =
значення sync_meta.version НА МОМЕНТ зміни. Pull-ендпоінт
GET /api/v1/sync/master повертає рядки з server_version > since_version —
стабільні дельти без повторної видачі версій.

Що робить міграція:

  1. server_version BIGINT NOT NULL DEFAULT 0 на таблицях-довідниках:
     products, categories, suppliers, users (employees), system_settings
     (settings). stock_norms таблиці в реальній схемі НЕМАЄ (див. 0011) —
     її pull завжди порожній.

  2. bump_sync_version() переписується: тепер це BEFORE-тригер, який
     інкрементує sync_meta.version і ПРОСТАВЛЯЄ NEW.server_version =
     новій версії (у тій самій транзакції, що й DML — принцип 1.3).
     Для DELETE (фізичне видалення довідника — заборонено дизайном 1.4)
     функція лише інкрементує sync_meta (аудит); server_version проставити
     неможливо — рядок зникає.

  3. Backfill ІСНУЮЧИХ рядків унікальними монотонними версіями:
       sync_meta.version += count(rows)
       rows.server_version = 1..count (відсортовані за created_at)
     Унікальність критична для пагінації (WHERE server_version > since):
     рівні версії на різних рядках могли б «загубити» частину сторінки.
     Рядки is_deleted=true також отримують версії — каса першим pull
     позначить їх видаленими локально (op=delete).

Revision ID: 0012_server_version_columns
Revises: 0011_sync_server_schema
Create Date: 2026-09-10
"""

from typing import Sequence, Union

from alembic import op

# revision identifiers, used by Alembic.
revision: str = "0012_server_version_columns"
down_revision: Union[str, Sequence[str], None] = "0011_sync_server_schema"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# (таблиця, sync_meta entity) — server_version + BEFORE-тригер bump.
VERSION_TABLES: list[tuple[str, str]] = [
    ("products", "products"),
    ("categories", "categories"),
    ("suppliers", "suppliers"),
    ("users", "employees"),
    ("system_settings", "settings"),
]


def upgrade() -> None:
    # ── 1. Колонка server_version на довідниках ──────────────────────────
    for table, _entity in VERSION_TABLES:
        op.execute(
            f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS "
            "server_version bigint NOT NULL DEFAULT 0"
        )

    # ── 2. Тригер bump: BEFORE + проставляння NEW.server_version ─────────
    op.execute(
        """
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
            -- DELETE: фізичне видалення довідника (заборонено дизайном 1.4).
            -- server_version проставити неможливо — лише інкремент аудиту.
            RETURN OLD;
        END; $$ LANGUAGE plpgsql
        """
    )
    for table, entity in VERSION_TABLES:
        op.execute(f"DROP TRIGGER IF EXISTS trg_{table}_bump ON {table}")
        op.execute(
            f"CREATE TRIGGER trg_{table}_bump "
            f"BEFORE INSERT OR UPDATE OR DELETE ON {table} "
            f"FOR EACH ROW EXECUTE FUNCTION bump_sync_version('{entity}')"
        )

    # ── 3. Backfill: унікальні монотонні версії для існуючих рядків ───────
    # Кожна таблиця окремо (різні PK/created_at). Схема однакова:
    #   sync_meta.version += count(rows)
    #   rows.server_version = (нова version) - row_number + 1  → 1..count
    for table, entity in VERSION_TABLES:
        op.execute(
            f"""
            UPDATE sync_meta SET version = version + (SELECT count(*) FROM {table})
            WHERE entity = '{entity}'
            """
        )
        op.execute(
            f"""
            UPDATE {table} SET server_version = sub.new_ver
            FROM (
                SELECT id,
                       (SELECT version FROM sync_meta WHERE entity = '{entity}')
                       - row_number() OVER (ORDER BY created_at, id) + 1 AS new_ver
                FROM {table}
            ) sub
            WHERE {table}.id = sub.id
            """
        )


def downgrade() -> None:
    """Повернути AFTER-тригери 0011 та прибрати server_version.

    Примітка: зворотний backfill (server_version → 0) не виконується —
    версії в sync_meta вже зсунуті вперед і назад їх не відмотати без
    втрати історії. downgrade лише відновлює тригери 0011.
    """
    # Тригери 0011: AFTER INSERT OR UPDATE OR DELETE (без server_version).
    op.execute(
        """
        CREATE OR REPLACE FUNCTION bump_sync_version() RETURNS trigger AS $$
        BEGIN
            UPDATE sync_meta SET version = version + 1 WHERE entity = TG_ARGV[0];
            RETURN COALESCE(NEW, OLD);
        END; $$ LANGUAGE plpgsql
        """
    )
    for table, entity in VERSION_TABLES:
        op.execute(f"DROP TRIGGER IF EXISTS trg_{table}_bump ON {table}")
        op.execute(
            f"CREATE TRIGGER trg_{table}_bump "
            f"AFTER INSERT OR UPDATE OR DELETE ON {table} "
            f"FOR EACH ROW EXECUTE FUNCTION bump_sync_version('{entity}')"
        )
    for table, _entity in VERSION_TABLES:
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS server_version")
