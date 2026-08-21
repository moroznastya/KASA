"""Додати роль owner в ENUM user_role.

Окрема міграція: нове значення enum НЕ можна використати в тій самій
транзакції, де воно додається (PostgreSQL: unsafe use of new value).
Тому ADD VALUE виконується тут, а використання 'owner' — у наступній
міграції 0002_multi_store_core.

Revision ID: 0002a_add_owner_role
Revises: 20260820_merge_heads
Create Date: 2026-08-20
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

# revision identifiers, used by Alembic.
revision: str = "0002a_add_owner_role"
down_revision: Union[str, Sequence[str], None] = "20260820_merge_heads"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    op.execute(
        "ALTER TYPE user_role ADD VALUE IF NOT EXISTS 'owner'"
    )


def downgrade() -> None:
    # PostgreSQL не підтримує видалення значення enum без пересоздання типу.
    # Залишаємо 'owner' у типі (зворотна сумісність не порушується).
    pass
