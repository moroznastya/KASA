"""ЕТАП 4 (offline-first): ідемпотентний приймач прибуткових накладних — invoices.client_uuid + partial UNIQUE.

Закриває прогалину, свідомо залишену міграцією 0013_sync_push_idempotency
(там invoices/invoice_items виключені: «не транзакції каси push» — рядок 48
docstring'а). Це свідомий перегляд того рішення: накладні теж синхронізуються
з черги офлайн-вузла, і повторна відправка після обриву зв'язку НЕ має
задвоїти надходження товару на склад.

Що робить міграція:
  * invoices.client_uuid uuid (nullable) — UUIDv4 документа, генерується один
    раз на вузлі-джерелі;
  * partial UNIQUE-індекс uq_invoices_client_uuid (WHERE client_uuid IS NOT NULL)
    — атомарний захист від дубля: повторний push того самого uuid → порушення
    UNIQUE → сервер відповідає already_exists. Патерн DROP IF EXISTS + CREATE —
    точно як у 0013 (захист від рудиментарного індексу з тим самим ім'ям на
    іншій колонці).

invoice_items client_uuid НЕ отримує: ключ ідемпотентності — на головному
документі invoices (як receipts для чеків; позиції перестворюються разом із
документом у межах тієї самої транзакції приймача).

Revision ID: 0016_invoice_push_idempotency
Revises: 0015_admin_stores_legal_edrpou
Create Date: 2026-09-02
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0016_invoice_push_idempotency"
down_revision: Union[str, Sequence[str], None] = "0015_admin_stores_legal_edrpou"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """client_uuid + partial UNIQUE на головному документі invoices."""
    op.execute(
        "ALTER TABLE invoices ADD COLUMN IF NOT EXISTS client_uuid uuid"
    )
    # DROP+CREATE: захист від рудиментарного індексу з тим самим ім'ям
    # на іншій колонці (див. docstring 0013 — receipts.client_receipt_uuid).
    op.execute("DROP INDEX IF EXISTS uq_invoices_client_uuid")
    op.execute(
        "CREATE UNIQUE INDEX uq_invoices_client_uuid "
        "ON invoices (client_uuid) WHERE client_uuid IS NOT NULL"
    )


def downgrade() -> None:
    """Прибрати client_uuid та його UNIQUE-індекс з invoices."""
    op.execute("DROP INDEX IF EXISTS uq_invoices_client_uuid")
    op.execute("ALTER TABLE invoices DROP COLUMN IF EXISTS client_uuid")
