"""add payment_method to invoices

Revision ID: ae9c51eb874e
Revises: 7f7208ec9b81
Create Date: 2026-07-21 21:37:16.453289+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'ae9c51eb874e'
down_revision: Union[str, None] = '7f7208ec9b81'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # Спочатку створюємо тип ENUM
    op.execute("CREATE TYPE payment_method AS ENUM ('credit', 'bank_transfer', 'cash', 'other')")
    # Потім додаємо колонку
    op.add_column('invoices', sa.Column(
        'payment_method',
        sa.Enum('credit', 'bank_transfer', 'cash', 'other', name='payment_method', create_constraint=True),
        nullable=True,
        comment='Спосіб оплати з постачальником'
    ))


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column('invoices', 'payment_method')
    op.execute("DROP TYPE IF EXISTS payment_method")
