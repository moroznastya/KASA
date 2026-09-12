"""Е5 «частина A» (ADR-0008 §7.1-C): довідники, придатні для pull.

Проблема: частина довідників не мала ні `server_version`, ні рядка в
`sync_meta`, ні місця в `ALLOWED_ENTITIES`. Наслідок — зміна на хабі НЕ
доїжджала до вузлів узагалі (немає версії → немає дельти; немає entity →
pull повертає 400).

Що робить міграція (рівно C1, C2, C5, C6; C3 `store_product_prices` —
наступний етап, C4 `print_templates` — рішення Творця, див. нижче):

  C1 `barcodes`, `product_images`:
     `server_version bigint NOT NULL DEFAULT 0` + BEFORE-тригер
     `trg_<table>_bump` на наявну функцію `bump_sync_version(entity)` +
     рядок у `sync_meta` (6 → 8 мінус C6 = 7) + entity у `ALLOWED_ENTITIES`
     (код: `torgashka-api/src/sync.rs`).

  C2 `write_off_reasons`:
     `server_version` + `is_deleted bool NOT NULL DEFAULT false` (tombstone-
     делеція довідника: pull мусить віддавати `op=delete`, а не мовчазне
     зникнення рядка) + тригер + рядок у `sync_meta` + entity.

  C5 `system_settings`:
     `is_deleted bool NOT NULL DEFAULT false` — без цієї колонки хендлер
     `sync.rs::query_settings` був ЗМУШЕНИЙ видавати лише `op=upsert`.

  C6 `stock_norms`:
     `DELETE FROM sync_meta WHERE entity='stock_norms'` — таблиці в
     серверній схемі НЕ ІСНУЄ (Alembic 0012:13-18), дельта завжди була
     порожньою, а кожен цикл pull вузла робив марний запит → 400.

  C4 `print_templates` — НЕ синхронізується (свідомо, не забуто):
     # C4: локальний оверрайд — рішення Творця відсутнє (Б?)
     Доказ структури: (а) `schema.sql` дає таблиці `store_id` (шаблон точки);
     (б) backend має 9 ендпоінтів CRUD + `set-default`
     (`app/api/v1/print_templates.py`), тобто точка редагує шаблони сама;
     (в) `uq_print_templates_default_per_type` — UNIQUE (type) WHERE
     is_default, тобто ОДИН default на тип ГЛОБАЛЬНО, що несумісно з
     per-store дефолтами (потрібне рішення: індекс по (store_id, type) чи
     каталог хаба); (г) ADR §5 рядок 17 — власник «Х (каталог) → В
     (локальний оверрайд)». Роздача каталогу хаба затерла б шаблони точки.
     Тому: ні `server_version`, ні тригера, ні entity для `print_templates` —
     доки Творець не вирішить природу таблиці.

Механізм версій — наявний (0011/0012): BEFORE-тригер `bump_sync_version()`
інкрементує `sync_meta.version` і проставляє `NEW.server_version` у тій самій
транзакції. Backfill існуючих рядків — як у 0012 (унікальні монотонні версії
1..count, `sync_meta.version += count`): рівні версії на різних рядках могли б
«загубити» частину сторінки при пагінації `WHERE server_version > since`.

Revision ID: 0023_directory_pull_versions
Revises: 0022_catalog_change_requests
Create Date: 2026-09-12
"""

from typing import Sequence, Union

from alembic import op

# revision identifiers, used by Alembic.
revision: str = "0023_directory_pull_versions"
down_revision: Union[str, Sequence[str], None] = "0022_catalog_change_requests"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# C1/C2: (таблиця, sync_meta entity) — server_version + BEFORE-тригер bump.
VERSION_TABLES: list[tuple[str, str]] = [
    ("barcodes", "barcodes"),  # C1
    ("product_images", "product_images"),  # C1
    ("write_off_reasons", "write_off_reasons"),  # C2
]

# C6: рядок, що видаляється (таблиці stock_norms у серверній схемі немає).
REMOVED_ENTITY = "stock_norms"

# C5: таблиця, що отримує лише soft-delete колонку (server_version уже є).
SOFT_DELETE_TABLES: list[str] = ["system_settings"]


def upgrade() -> None:
    # ── C1/C2: колонка server_version на довідниках ───────────────────────
    for table, _entity in VERSION_TABLES:
        op.execute(
            f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS "
            "server_version bigint NOT NULL DEFAULT 0"
        )

    # ── C2: tombstone-делеція довідника причин списання ───────────────────
    op.execute(
        "ALTER TABLE write_off_reasons ADD COLUMN IF NOT EXISTS "
        "is_deleted boolean NOT NULL DEFAULT false"
    )

    # ── C5: system_settings.is_deleted (op=delete замість завжди upsert) ──
    for table in SOFT_DELETE_TABLES:
        op.execute(
            f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS "
            "is_deleted boolean NOT NULL DEFAULT false"
        )

    # ── C1/C2: рядки sync_meta (ідемпотентно) ─────────────────────────────
    for _table, entity in VERSION_TABLES:
        op.execute(f"INSERT INTO sync_meta (entity) VALUES ('{entity}') ON CONFLICT (entity) DO NOTHING")

    # ── C1/C2: тригери bump (функція bump_sync_version створена в 0012) ───
    for table, entity in VERSION_TABLES:
        op.execute(f"DROP TRIGGER IF EXISTS trg_{table}_bump ON {table}")
        op.execute(
            f"CREATE TRIGGER trg_{table}_bump "
            f"BEFORE INSERT OR UPDATE OR DELETE ON {table} "
            f"FOR EACH ROW EXECUTE FUNCTION bump_sync_version('{entity}')"
        )

    # ── C1/C2: backfill існуючих рядків (як у 0012) ───────────────────────
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

    # ── C6: stock_norms — таблиці немає, рядок версій зайвий ──────────────
    op.execute(f"DELETE FROM sync_meta WHERE entity = '{REMOVED_ENTITY}'")


def downgrade() -> None:
    """Відкат: повернути рядок stock_norms, зняти тригери й колонки.

    Версії в sync_meta назад не відмотуються (як у 0012) — backfill не
    зворотний; знімаються лише тригери й колонки, а рядки sync_meta нових
    сутностей лишаються (0 або накопичене значення) — це безпечно: дельта
    без entity у коді не запитується.
    """
    for table, _entity in VERSION_TABLES:
        op.execute(f"DROP TRIGGER IF EXISTS trg_{table}_bump ON {table}")
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS server_version")
    op.execute("ALTER TABLE write_off_reasons DROP COLUMN IF EXISTS is_deleted")
    for table in SOFT_DELETE_TABLES:
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS is_deleted")
    op.execute(
        f"INSERT INTO sync_meta (entity) VALUES ('{REMOVED_ENTITY}') "
        "ON CONFLICT (entity) DO NOTHING"
    )
