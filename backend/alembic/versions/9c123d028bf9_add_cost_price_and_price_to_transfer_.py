"""add_cost_price_and_price_to_transfer_and_write_off_items

Revision ID: 9c123d028bf9
Revises: a1b2c3d4e5f6
Create Date: 2026-08-02 21:10:26.495858+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = '9c123d028bf9'
down_revision: Union[str, None] = 'a1b2c3d4e5f6'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Додати вартісні колонки (собівартість і ціна продажу) у позиції
    документів «Переміщення» (transfer_items) та «Списання» (write_off_items).

    server_default='0' обов'язковий: у таблицях можуть бути існуючі рядки —
    вони отримають 0, а не NULL (колонки nullable=False).
    """
    op.add_column(
        'transfer_items',
        sa.Column(
            'cost_price',
            sa.Numeric(precision=12, scale=2),
            nullable=False,
            server_default='0',
            comment='Собівартість одиниці товару',
        ),
    )
    op.add_column(
        'transfer_items',
        sa.Column(
            'price',
            sa.Numeric(precision=12, scale=2),
            nullable=False,
            server_default='0',
            comment='Ціна продажу одиниці товару',
        ),
    )
    op.add_column(
        'write_off_items',
        sa.Column(
            'cost_price',
            sa.Numeric(precision=12, scale=2),
            nullable=False,
            server_default='0',
            comment='Собівартість одиниці товару',
        ),
    )
    op.add_column(
        'write_off_items',
        sa.Column(
            'price',
            sa.Numeric(precision=12, scale=2),
            nullable=False,
            server_default='0',
            comment='Ціна продажу одиниці товару',
        ),
    )


def downgrade() -> None:
    """Відкотити міграцію — видалити вартісні колонки."""
    op.drop_column('write_off_items', 'price')
    op.drop_column('write_off_items', 'cost_price')
    op.drop_column('transfer_items', 'price')
    op.drop_column('transfer_items', 'cost_price')
