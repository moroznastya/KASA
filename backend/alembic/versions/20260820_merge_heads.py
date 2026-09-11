"""merge heads: зведення двох ліній міграцій в одну (мультиточковість).

Зводить:
  - 43fbd6dbc463 (лінія реалізації: write_offs, receipts, prro, ...)
  - f89706f0cc12 (лінія print_templates)

Revision ID: 20260820_merge_heads
Revises: 43fbd6dbc463, f89706f0cc12
Create Date: 2026-08-20
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = "20260820_merge_heads"
down_revision: Union[str, Sequence[str], None] = (
    "43fbd6dbc463",
    "f89706f0cc12",
)
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Немає змін даних — лише злиття гілок."""
    pass


def downgrade() -> None:
    """Немає змін даних — лише злиття гілок."""
    pass
