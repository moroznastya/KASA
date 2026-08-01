"""Додати fiscal_quantity до позицій чеків (часткова фіскалізація)

КОНТЕКСТ: Чек Kasa може містити ФІСКАЛЬНІ позиції (товар з фіскальної
накладної, Product.is_fiscal=True) та НЕФІСКАЛЬНІ (решту товару).
Для кожної позиції чека зберігаємо, яка кількість фіскалізується
(передається у ПРРО/податкову): fiscal_quantity.

ЗМІНА:
- receipt_items.fiscal_quantity NUMERIC(10,3) NOT NULL DEFAULT 0
  (0 = нефіскальна; 0<fiscal_quantity<quantity = часткова фіскалізація;
  =quantity = повністю фіскальна)

downgrade: DROP COLUMN.

Revision ID: f89706f0cc26
Revises: f89706f0cc25
Create Date: 2026-08-01
"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc26'
down_revision: Union[str, None] = 'f89706f0cc25'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    op.add_column(
        'receipt_items',
        sa.Column(
            'fiscal_quantity',
            sa.Numeric(precision=10, scale=3),
            server_default=sa.text('0'),
            nullable=False,
            comment='Фіскалізована кількість позиції (0 = нефіскальна; 0<fiscal_quantity<quantity = часткова фіскалізація; =quantity = повністю фіскальна)',
        ),
    )


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column('receipt_items', 'fiscal_quantity')
