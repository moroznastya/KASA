"""add last_login_at to users

Revision ID: a1b2c3d4e5f6
Revises: 578fd283a156
Create Date: 2026-08-01 17:30:00.000000
"""

from typing import Sequence, Union

import sqlalchemy as sa
from alembic import op

# revision identifiers, used by Alembic.
revision: str = "a1b2c3d4e5f6"
down_revision: Union[str, None] = "578fd283a156"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Додає стовпець last_login_at до таблиці users."""
    op.add_column(
        "users",
        sa.Column("last_login_at", sa.DateTime(), nullable=True, comment="Дата/час останнього входу"),
    )


def downgrade() -> None:
    """Видаляє стовпець last_login_at з таблиці users."""
    op.drop_column("users", "last_login_at")
