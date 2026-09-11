"""add onboarding_completed to users

Revision ID: 0005_onboarding_completed
Revises: 0004_rls
Create Date: 2026-08-21 01:10:00.000000+00:00

Прапорець завершення онбордингу для користувачів (серверний стан UI-флоу).
Існуючі користувачі отримують TRUE (server_default) — без регресії.
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = "0005_onboarding_completed"
down_revision: Union[str, Sequence[str], None] = "0004_rls"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    op.add_column(
        "users",
        sa.Column(
            "onboarding_completed",
            sa.Boolean(),
            server_default=sa.text("true"),
            nullable=False,
            comment="Онбординг завершено (прапорець клієнтського UI). За замовчуванням true — існуючі користувачі без регресії",
        ),
    )


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column("users", "onboarding_completed")
