"""add_payment_method_to_receipts

Revision ID: e3186c88fc28
Revises: a84eefa802e4
Create Date: 2026-07-25 22:02:06.476078+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'e3186c88fc28'
down_revision: Union[str, None] = 'a84eefa802e4'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


# Назва enum типу в PostgreSQL
ENUM_NAME = "receipt_payment_method"
ENUM_VALUES = ["cash", "card", "mixed"]


def upgrade() -> None:
    """Застосувати міграцію."""

    # ── 1. Створюємо enum тип в PostgreSQL ────────
    op.execute(f"CREATE TYPE {ENUM_NAME} AS ENUM ({', '.join(repr(v) for v in ENUM_VALUES)})")

    # ── 2. Додаємо колонку з цим типом ────────────
    op.add_column(
        'receipts',
        sa.Column(
            'payment_method',
            sa.Enum(ENUM_VALUES[0], ENUM_VALUES[1], ENUM_VALUES[2],
                    name=ENUM_NAME, create_constraint=True),
            nullable=True,
            comment='Спосіб оплати: cash, card, mixed',
        ),
    )


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column('receipts', 'payment_method')
    op.execute(f"DROP TYPE IF EXISTS {ENUM_NAME}")
