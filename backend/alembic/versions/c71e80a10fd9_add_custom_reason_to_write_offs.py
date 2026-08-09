"""add custom_reason to write_offs

Revision ID: c71e80a10fd9
Revises: 43441ff89b4f
Create Date: 2026-08-09 20:31:11.650396+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

# revision identifiers, used by Alembic.
revision: str = 'c71e80a10fd9'
down_revision: Union[str, None] = '43441ff89b4f'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Додає колонку custom_reason до write_offs (довільна причина списання)."""
    op.add_column(
        'write_offs',
        sa.Column(
            'custom_reason',
            sa.Text(),
            nullable=True,
            comment='Довільна причина списання (коли reason=other)',
        ),
    )


def downgrade() -> None:
    """Прибирає колонку custom_reason."""
    op.drop_column('write_offs', 'custom_reason')
