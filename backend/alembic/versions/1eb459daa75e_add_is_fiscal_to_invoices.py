"""add_is_fiscal_to_invoices

Revision ID: 1eb459daa75e
Revises: e2914ed12716
Create Date: 2026-07-21 10:21:55.462942+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = '1eb459daa75e'
down_revision: Union[str, None] = 'e2914ed12716'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # Додаємо колонки з server_default, щоб заповнити існуючі рядки
    op.add_column('invoices', sa.Column('is_fiscal', sa.Boolean(), server_default=sa.text('false'), nullable=False, comment='Фіскальна накладна (проведена через РРО)'))
    op.add_column('return_invoices', sa.Column('is_fiscal', sa.Boolean(), server_default=sa.text('false'), nullable=False, comment='Фіскальний документ (проведений через РРО)'))
    # Видаляємо server_default після того, як всі рядки заповнені
    op.alter_column('invoices', 'is_fiscal', server_default=None)
    op.alter_column('return_invoices', 'is_fiscal', server_default=None)


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column('return_invoices', 'is_fiscal')
    op.drop_column('invoices', 'is_fiscal')
