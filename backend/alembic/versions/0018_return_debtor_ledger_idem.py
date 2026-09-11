"""ФАЗА 3.3b (offline-first): ідемпотентні приймачі повернень постачальнику,
оплат боргу й ручних записів книги постачальника — client_uuid + partial UNIQUE.

Каса створює ці документи ОФЛАЙН (standby-вузол, клас LocalOutbox, ADR-0007
§11.7.9.7). Повторна відправка після обриву зв'язку НЕ має задвоїти ефект
(товар/борг/книгу), тому кожна таблиця-приймач отримує:

  * ``return_invoices.client_uuid``  + partial UNIQUE ``uq_return_invoices_client_uuid``
    — повернення ПОСТАЧАЛЬНИКУ (НЕ чек покупця: той приймається в ``receipts``);
  * ``debtor_payments.client_uuid``  + partial UNIQUE ``uq_debtor_payments_client_uuid``
    — оплата боргу покупця (разом з ``UPDATE debtors.total_debt`` в одній
    транзакції приймача);
  * ``supplier_ledger.client_uuid``  + partial UNIQUE ``uq_supplier_ledger_client_uuid``
    — ручний запис у книгу постачальника (``POST /api/v1/ledger``,
    ``POST /api/v2/ledger/entries``).

Патерн DROP IF EXISTS + CREATE — точно як 0013/0016/0017 (захист від
рудиментарного індексу з тим самим ім'ям на іншій колонці).

Ім'я ревізії — 29 символів: ``alembic_version.version_num`` це ``varchar(32)``
(див. docstring 0017).

Revision ID: 0018_return_debtor_ledger_idem
Revises: 0017_cash_operation_idempotency
Create Date: 2026-09-12
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0018_return_debtor_ledger_idem"
down_revision: Union[str, Sequence[str], None] = "0017_cash_operation_idempotency"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# (таблиця, ім'я UNIQUE-індексу) — однакова форма для всіх трьох приймачів.
TARGETS: tuple[tuple[str, str], ...] = (
    ("return_invoices", "uq_return_invoices_client_uuid"),
    ("debtor_payments", "uq_debtor_payments_client_uuid"),
    ("supplier_ledger", "uq_supplier_ledger_client_uuid"),
)


def upgrade() -> None:
    """client_uuid + partial UNIQUE на трьох приймачах Фази 3.3b."""
    for table, index in TARGETS:
        op.execute(f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS client_uuid uuid")
        op.execute(f"DROP INDEX IF EXISTS {index}")
        op.execute(
            f"CREATE UNIQUE INDEX {index} ON {table} (client_uuid) "
            "WHERE client_uuid IS NOT NULL"
        )


def downgrade() -> None:
    """Прибрати client_uuid та UNIQUE-індекси з трьох приймачів."""
    for table, index in TARGETS:
        op.execute(f"DROP INDEX IF EXISTS {index}")
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS client_uuid")
