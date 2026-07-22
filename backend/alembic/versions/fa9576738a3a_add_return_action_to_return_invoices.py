"""add_return_action_to_return_invoices

Revision ID: fa9576738a3a
Revises: ae9c51eb874e
Create Date: 2026-07-21 22:50:06.333648+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'fa9576738a3a'
down_revision: Union[str, None] = 'ae9c51eb874e'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # Створюємо ENUM тип
    op.execute("CREATE TYPE return_action_type AS ENUM ('deduct_from_debt', 'add_to_cash', 'exchange')")
    
    # Додаємо колонку з цим типом
    op.add_column('return_invoices', sa.Column(
        'return_action',
        sa.Enum('deduct_from_debt', 'add_to_cash', 'exchange', name='return_action_type', create_constraint=True, create_type=False),
        nullable=False,
        server_default='deduct_from_debt',
        comment='Дія при підтвердженні: списати з боргу / в касу / на обмін',
    ))
    op.alter_column('return_invoices', 'return_action', server_default=None)


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column('return_invoices', 'return_action')
    op.execute("DROP TYPE IF EXISTS return_action_type")
