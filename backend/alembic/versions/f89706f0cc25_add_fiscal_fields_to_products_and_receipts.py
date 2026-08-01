"""Додати фіскальний облік товарів та чеків (правило «товар з фіскальної накладної»)

КОНТЕКСТ: Програма веде відстеження, чи товар надійшов з фіскальної накладної
(Invoice.is_fiscal / ReturnInvoice.is_fiscal вже існують), формує окремі
фіскальні чеки тільки з фіскальними товарами та відправляє їх у податкову.

ЗМІНИ:
1. products:
   - is_fiscal    BOOLEAN      NOT NULL DEFAULT false  — ознака: товар надходив з фіскальної накладної
   - fiscal_stock NUMERIC(10,3) NOT NULL DEFAULT 0     — кількість у поточному залишку з фіскальних накладних
2. receipts:
   - новий enum-тип fiscal_status ('none','pending','sent','failed')
   - is_fiscal        BOOLEAN  NOT NULL DEFAULT false
   - fiscal_status    fiscal_status NOT NULL DEFAULT 'none'
   - fiscal_number    VARCHAR(50)  NULL
   - fiscal_serial    VARCHAR(50)  NULL
   - fiscal_sent_at   TIMESTAMP    NULL
   - fiscal_error     TEXT         NULL
   - split_group_id   UUID NULL, FK receipts.id ON DELETE SET NULL + index
     (ID пов'язаного чеку при розділенні фіскальних/нефіскальних позицій)

downgrade: видаляє колонки у зворотному порядку, потім drop enum-типу.

Revision ID: f89706f0cc25
Revises: f89706f0cc24
Create Date: 2026-08-01
"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc25'
down_revision: Union[str, None] = 'f89706f0cc24'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # ── products ─────────────────────────────────
    op.add_column(
        'products',
        sa.Column(
            'is_fiscal',
            sa.Boolean(),
            server_default=sa.text('false'),
            nullable=False,
            comment='Ознака: товар надходив з фіскальної накладної',
        ),
    )
    op.add_column(
        'products',
        sa.Column(
            'fiscal_stock',
            sa.Numeric(precision=10, scale=3),
            server_default=sa.text('0'),
            nullable=False,
            comment='Кількість у поточному залишку, що надійшла з фіскальних накладних',
        ),
    )

    # ── тип enum fiscal_status ──────────────────
    op.execute(
        "CREATE TYPE fiscal_status AS ENUM ('none', 'pending', 'sent', 'failed')"
    )

    # ── receipts ─────────────────────────────────
    op.add_column(
        'receipts',
        sa.Column(
            'is_fiscal',
            sa.Boolean(),
            server_default=sa.text('false'),
            nullable=False,
            comment='Чек є фіскальним (містить лише товари з фіскальних накладних)',
        ),
    )
    op.add_column(
        'receipts',
        sa.Column(
            'fiscal_status',
            sa.Enum(
                'none', 'pending', 'sent', 'failed',
                name='fiscal_status',
                create_type=False,
            ),
            server_default=sa.text("'none'"),
            nullable=False,
            comment='Статус відправки фіскального чеку у податкову',
        ),
    )
    op.add_column(
        'receipts',
        sa.Column(
            'fiscal_number',
            sa.String(length=50),
            nullable=True,
            comment='Фіскальний номер чеку, присвоєний податковою',
        ),
    )
    op.add_column(
        'receipts',
        sa.Column(
            'fiscal_serial',
            sa.String(length=50),
            nullable=True,
            comment='Фіскальний серійний номер',
        ),
    )
    op.add_column(
        'receipts',
        sa.Column(
            'fiscal_sent_at',
            sa.DateTime(),
            nullable=True,
            comment='Дата/час успішної відправки у податкову',
        ),
    )
    op.add_column(
        'receipts',
        sa.Column(
            'fiscal_error',
            sa.Text(),
            nullable=True,
            comment='Текст помилки при відправці у податкову',
        ),
    )
    op.add_column(
        'receipts',
        sa.Column(
            'split_group_id',
            sa.UUID(as_uuid=True),
            sa.ForeignKey('receipts.id', ondelete='SET NULL'),
            nullable=True,
            comment="ID пов'язаного чеку при розділенні фіскальних/нефіскальних позицій (обидва чеки однієї продажі)",
        ),
    )
    op.create_index(
        'ix_receipts_split_group_id',
        'receipts',
        ['split_group_id'],
    )


def downgrade() -> None:
    """Відкотити міграцію (зворотний порядок)."""
    # ── receipts ─────────────────────────────────
    op.drop_index('ix_receipts_split_group_id', table_name='receipts')
    op.drop_column('receipts', 'split_group_id')
    op.drop_column('receipts', 'fiscal_error')
    op.drop_column('receipts', 'fiscal_sent_at')
    op.drop_column('receipts', 'fiscal_serial')
    op.drop_column('receipts', 'fiscal_number')
    op.drop_column('receipts', 'fiscal_status')
    op.drop_column('receipts', 'is_fiscal')

    # ── тип enum fiscal_status ──────────────────
    op.execute('DROP TYPE IF EXISTS fiscal_status')

    # ── products ─────────────────────────────────
    op.drop_column('products', 'fiscal_stock')
    op.drop_column('products', 'is_fiscal')
