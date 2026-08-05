"""backfill_cost_price_and_price_for_transfer_and_write_off_items

Revision ID: af5fba990920
Revises: 9c123d028bf9
Create Date: 2026-08-02 21:17:52.856602+00:00

Data migration: заповнює cost_price / price у позиціях «Переміщення» та
«Списання» з карток товарів (products). Колонки вже додані міграцією
9c123d028bf9 (numeric(12,2) NOT NULL DEFAULT 0), тому тут лише backfill.
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'af5fba990920'
down_revision: Union[str, None] = '9c123d028bf9'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Заповнити вартісні колонки з карток товарів (COALESCE — безпечно для NULL)."""
    op.execute(
        """
        UPDATE transfer_items ti
        SET cost_price = COALESCE(p.cost_price, 0),
            price      = COALESCE(p.price, 0)
        FROM products p
        WHERE ti.product_id = p.id
        """
    )
    op.execute(
        """
        UPDATE write_off_items woi
        SET cost_price = COALESCE(p.cost_price, 0),
            price      = COALESCE(p.price, 0)
        FROM products p
        WHERE woi.product_id = p.id
        """
    )


def downgrade() -> None:
    """Відкотити неможливо: попередні значення не зберігаються.
    Колонки залишаються (їх видалення — у downgrade міграції 9c123d028bf9).
    """
    pass
