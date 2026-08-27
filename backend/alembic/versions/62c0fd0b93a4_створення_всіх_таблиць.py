"""Створення всіх таблиць

Revision ID: 62c0fd0b93a4
Revises: 0001
Create Date: 2026-07-20 14:50:13.508879+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = '62c0fd0b93a4'
down_revision: Union[str, None] = '0001'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію.

    NO-OP: повний дубль 0001_initial_full_schema (ті самі 17 таблиць + enum).
    Усі об'єкти вже створені базовою міграцією 0001. Дубль лишається в
    ланцюгу лише для зворотної сумісності (alembic_version на старих БД).
    Фікс 2026-08-27: `alembic upgrade head` з чистої БД падав на
    "relation categories already exists".
    """
    pass

def downgrade() -> None:
    """Відкат — NO-OP (об'єкти створює/видаляє 0001)."""
    pass
