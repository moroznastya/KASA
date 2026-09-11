"""ЕТАП 4 (offline-first): ідемпотентний приймач касових операцій — cash_operations.client_uuid + partial UNIQUE.

Закриває прогалину, свідомо залишену міграцією 0013_sync_push_idempotency
(там cash_operations виключена: «дизайн 8.2 — таблиця не існує в SQLAlchemy-
моделях», рядки 43–46 docstring'а). Це свідомий перегляд того рішення: касові
операції (внесення/інкасація) створюються касою офлайн і надходять у черзі
push (ADR-0007 §11.6, клас LocalOutbox), тож повторна відправка після обриву
зв'язку НЕ має задвоїти рух грошового ящика.

Що робить міграція:
  * cash_operations.client_uuid uuid (nullable) — UUIDv4 операції, генерується
    один раз на вузлі-джерелі (агрегат `cash_ledger` + outbox-запис);
  * partial UNIQUE-індекс uq_cash_operations_client_uuid
    (WHERE client_uuid IS NOT NULL) — атомарний захист від дубля: повторний
    push того самого uuid → порушення UNIQUE → сервер відповідає
    already_exists. Патерн DROP IF EXISTS + CREATE — точно як у 0013/0016
    (захист від рудиментарного індексу з тим самим ім'ям на іншій колонці).

Ім'я ревізії — 31 символ: колонка `alembic_version.version_num` це
`varchar(32)`; довше ім'я валило `upgrade head` на UPDATE версії
(StringDataRightTruncationError) ПІСЛЯ успішного DDL (перевірено на
`pos_system_fresh` 2026-09-11).

Джерело істини для балансу каси лишається таблиця cash_operations (окремої
таблиці балансу на primary немає — balance рахується запитом у
`repositories/pos.rs::list_cash_operations`). Локальний баланс каси
(`cash_balance`, offline-міграція 0011) — оцінка вузла, не істина.

Revision ID: 0017_cash_operation_idempotency
Revises: 0016_invoice_push_idempotency
Create Date: 2026-09-11
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0017_cash_operation_idempotency"
down_revision: Union[str, Sequence[str], None] = "0016_invoice_push_idempotency"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """client_uuid + partial UNIQUE на приймачі касових операцій."""
    op.execute(
        "ALTER TABLE cash_operations ADD COLUMN IF NOT EXISTS client_uuid uuid"
    )
    # DROP+CREATE: захист від рудиментарного індексу з тим самим ім'ям
    # на іншій колонці (див. docstring 0013 — receipts.client_receipt_uuid).
    op.execute("DROP INDEX IF EXISTS uq_cash_operations_client_uuid")
    op.execute(
        "CREATE UNIQUE INDEX uq_cash_operations_client_uuid "
        "ON cash_operations (client_uuid) WHERE client_uuid IS NOT NULL"
    )


def downgrade() -> None:
    """Прибрати client_uuid та його UNIQUE-індекс з cash_operations."""
    op.execute("DROP INDEX IF EXISTS uq_cash_operations_client_uuid")
    op.execute("ALTER TABLE cash_operations DROP COLUMN IF EXISTS client_uuid")
