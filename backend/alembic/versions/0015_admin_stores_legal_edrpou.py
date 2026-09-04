"""admin-панель власника (Етап 1): stores.legal_name/edrpou + роль store_manager

Етап 1 ТЗ «Адмін-панель власника мережі» (розділи 5.1–5.3):
  * stores.legal_name (varchar 255)  — юрособа/ФОП для ПРРО-вкладки (nullable);
  * stores.edrpou    (varchar 20)    — код ЄДРПОУ/ІПН для ПРРО-вкладки (nullable);
  * user_role додано 'store_manager' — керуючий мережею (роль адмін-панелі,
    поруч із owner; каса: admin/cashier).

Revision ID: 0015_admin_stores_legal_edrpou
Revises: 0014_drop_receipts_client_receipt_uuid
Create Date: 2026-08-02
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = "0015_admin_stores_legal_edrpou"
down_revision: Union[str, Sequence[str], None] = "0014_drop_receipts_client_receipt_uuid"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    op.add_column("stores", sa.Column("legal_name", sa.String(length=255), nullable=True))
    op.add_column("stores", sa.Column("edrpou", sa.String(length=20), nullable=True))
    # ALTER TYPE ... ADD VALUE IF NOT EXISTS — PostgreSQL 12+.
    op.execute(
        "ALTER TYPE public.user_role ADD VALUE IF NOT EXISTS 'store_manager'"
    )


def downgrade() -> None:
    op.drop_column("stores", "edrpou")
    op.drop_column("stores", "legal_name")
    # Значення enum не видаляємо (PostgreSQL не підтримує REMOVE VALUE) —
    # лишається невикористаним, що безпечно.
