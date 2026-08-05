"""add cash_card amounts to receipts

Revision ID: 257f5e9e1d2a
Revises: af5fba990920
Create Date: 2026-08-05 12:40:56.963622+00:00

Додає колонки cash_amount / card_amount (numeric(12,2), nullable) до таблиці receipts
для збереження розбивки оплати по способах (готівка/картка) у змішаних чеках.

Backfill історичних чеків:
  - payment_method='cash'  -> cash_amount = paid_amount
  - payment_method='card'  -> card_amount = paid_amount
  - payment_method='mixed' -> лишається NULL (розбивка історично не збережена)
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

# revision identifiers, used by Alembic.
revision: str = '257f5e9e1d2a'
down_revision: Union[str, None] = 'af5fba990920'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # ── Нові колонки розбивки оплати ──
    op.add_column(
        'receipts',
        sa.Column(
            'cash_amount',
            sa.Numeric(precision=12, scale=2),
            nullable=True,
            comment='Сума оплати готівкою (грн). Заповнюється для cash/mixed чеків',
        ),
    )
    op.add_column(
        'receipts',
        sa.Column(
            'card_amount',
            sa.Numeric(precision=12, scale=2),
            nullable=True,
            comment='Сума оплати карткою (грн). Заповнюється для card/mixed чеків',
        ),
    )

    # ── Backfill існуючих чеків (історичні дані) ──
    op.execute(
        "UPDATE receipts SET cash_amount = paid_amount "
        "WHERE payment_method = 'cash'"
    )
    op.execute(
        "UPDATE receipts SET card_amount = paid_amount "
        "WHERE payment_method = 'card'"
    )
    # mixed-чеки лишаються NULL: історична розбивка не збережена


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column('receipts', 'card_amount')
    op.drop_column('receipts', 'cash_amount')
