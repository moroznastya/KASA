"""ЕТАП 4 (offline-first): ідемпотентні приймачі push — client_uuid + UNIQUE.

Реалізує розділ 3.2 і 8.2 дизайну sync-schema-design.md: сервер приймає
транзакції каси ідемпотентно через UNIQUE(client_uuid) на таблицях-приймачах.

Що робить міграція:

  1. client_uuid uuid (nullable) + partial UNIQUE-індекс
     (WHERE client_uuid IS NOT NULL) на головних документах-приймачах:
       receipts, return_invoices, purchase_orders, inventories, transfers,
       write_offs, debtor_payments, work_sessions
     — усі 8 реально існують у SQLAlchemy-моделях і в БД (звірено
     з __tablename__). Каса генерує client_uuid (UUIDv4) один раз на
     транзакцію; повторний push того самого uuid → сервер відповідає
     already_exists (UNIQUE-індекс ловить дублікат атомарно).

     Створення індексу — DROP IF EXISTS + CREATE (без IF NOT EXISTS за
     ім'ям): виявлено рудиментарний індекс uq_receipts_client_uuid на
     колонці receipts.client_receipt_uuid у dev-БД, створеній зі
     schema.sql (kasa-спадок; колонка/індекс НЕ згадуються в жодному
     коді backend/rust і НЕ входять в alembic-ланцюг). IF NOT EXISTS за
     ім'ям мовчки пропускав створення індексу на client_uuid. DROP+CREATE
     гарантує однаковий кінцевий стан на всіх БД: uq_{table}_client_uuid
     завжди на колонці client_uuid. Сам рудиментарний стовпець
     client_receipt_uuid НЕ видаляється (поза межами міграції — окреме
     очищення dev-БД).

  2. sync_log — ВЖЕ створена міграцією 0011_sync_server_schema (точний
     зріз дизайну 8.2: id bigserial, store_id FK→stores ON DELETE CASCADE,
     direction CHECK pull|push, entity varchar(32), client_uuid uuid,
     status CHECK ok|error|already_exists, payload_hash, error,
     created_at timestamptz + індекси ix_sync_log_store/ix_sync_log_status).
     У коді backend/rust sync_log поки не використовується (перевірено
     grep по backend/app та torgashka-infrastructure) — дублювання НЕ
     виконується, ланцюг міграцій лінійний (0011 передує цій).

  3. is_deleted / updated_at на довідниках — ВЖЕ присутні:
       is_deleted  — 0011 на products, categories, suppliers, users;
       updated_at  — існує з 0001 (NOT NULL DEFAULT now()) на всіх чотирьох.
     Додавати нічого.

  Аномалії, зафіксовані за каналом зворотного зв'язку:
    - cash_operations (дизайн 8.2) НЕ існує в SQLAlchemy-моделях і в
      Alembic-ланцюзі — це Rust-таблиця (створюється DDL torgashka-
      infrastructure). Роль «cash-операції каси» в Python-моделях виконує
      work_sessions — вона отримує client_uuid. Для cash_operations
      client_uuid додасть Rust-шар ЕТАП 4.
    - invoices / invoice_items у дизайні 8.2 серед приймачів не згадані
      (це документи закупівель/рахунків постачальника, не транзакції каси
      push) — не чіпаються.
    - dev-БД pos_system_fresh(_test) містять мертву колонку
      receipts.client_receipt_uuid + індекс uq_receipts_client_uuid
      (kasa-спадок schema.sql) — індекс перестворюється на client_uuid,
      колонка лишається (рекомендація: окреме очищення).

Revision ID: 0013_sync_push_idempotency
Revises: 0012_server_version_columns
Create Date: 2026-09-02
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0013_sync_push_idempotency"
down_revision: Union[str, Sequence[str], None] = "0012_server_version_columns"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Таблиці-приймачі транзакцій каси (звірені з __tablename__ моделей).
# client_uuid + partial UNIQUE (WHERE client_uuid IS NOT NULL) — дизайн 8.2.
RECEIVER_TABLES: list[str] = [
    "receipts",
    "return_invoices",
    "purchase_orders",
    "inventories",
    "transfers",
    "write_offs",
    "debtor_payments",
    "work_sessions",
]


def upgrade() -> None:
    """client_uuid + partial UNIQUE на приймачах транзакцій каси."""
    for table in RECEIVER_TABLES:
        op.execute(
            f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS client_uuid uuid"
        )
        # DROP+CREATE: захист від рудиментарного індексу з тим самим ім'ям
        # на іншій колонці (див. docstring — receipts.client_receipt_uuid).
        op.execute(f"DROP INDEX IF EXISTS uq_{table}_client_uuid")
        op.execute(
            f"CREATE UNIQUE INDEX uq_{table}_client_uuid "
            f"ON {table} (client_uuid) WHERE client_uuid IS NOT NULL"
        )


def downgrade() -> None:
    """Прибрати client_uuid та його UNIQUE-індекси з приймачів."""
    for table in RECEIVER_TABLES:
        op.execute(f"DROP INDEX IF EXISTS uq_{table}_client_uuid")
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS client_uuid")
