"""Мультиточковість: Row-Level Security (другий контур захисту).

УВАГА: застосовувати ТІЛЬКИ разом з Rust-middleware (Етап 3), який
проставляє current_setting('app.user_id'/'app.store_id') на кожен запит.
Без middleware всі запити почнуть повертати 0 рядків.

Політики:
  - за замовчуванням: рядок видно, якщо store_id = поточна точка;
  - owner: бачить усі свої точки (через user_stores) — потрібно для
    міжточкового пошуку наявності та зведених звітів.

ВАЖЛИВО (quirk PostgreSQL): reset_config у Rust скидає контекст через
`RESET app.store_id` — але для custom-параметрів (з крапкою) це лишає
`current_setting(..., true) = ''`, а не NULL. Тому ВСІ касти в політиках
обгорнуті NULLIF(..., '') — '' трактується як «контекст не встановлений»,
рівно як свіже з'єднання (NULL). Без NULLIF політика падає
`invalid input syntax for type uuid` на будь-якому запиті non-superuser-ролі
після того, як з'єднання обслужило store-запит.

Revision ID: 0004_rls
Revises: 0003_multi_store_documents
Create Date: 2026-08-20
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy import inspect

# revision identifiers, used by Alembic.
revision: str = "0004_rls"
down_revision: Union[str, Sequence[str], None] = "0003_multi_store_documents"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Таблиці зі store_id — бізнес-дані
STORE_TABLES = [
    "stock",
    "receipts", "receipt_items",
    "invoices", "invoice_items",
    "transfers", "transfer_items",
    "write_offs", "write_off_items",
    "return_invoices", "return_invoice_items",
    "purchase_orders", "purchase_order_items",
    "inventories", "inventory_items",
    "work_sessions",
    "debtors", "debtor_payments",
    "supplier_ledger",
    "categories", "barcodes", "product_images",
    "system_settings",
]


def upgrade() -> None:
    bind = op.get_bind()

    # ── 1. Бізнес-таблиці: політика поточної точки + усі точки користувача
    for table in STORE_TABLES:
        op.execute(f"ALTER TABLE {table} ENABLE ROW LEVEL SECURITY")
        op.execute(f"DROP POLICY IF EXISTS {table}_store_isolation ON {table}")
        op.execute(f"""
            CREATE POLICY {table}_store_isolation ON {table}
            USING (
                store_id = NULLIF(current_setting('app.store_id', true), '')::uuid
                OR store_id IN (
                    SELECT store_id FROM user_stores
                    WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid
                )
            )
            WITH CHECK (
                store_id = NULLIF(current_setting('app.store_id', true), '')::uuid
                OR store_id IN (
                    SELECT store_id FROM user_stores
                    WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid
                )
            )
        """)

    # ── 2. stores: бачиш точки, до яких маєш доступ
    op.execute("ALTER TABLE stores ENABLE ROW LEVEL SECURITY")
    op.execute("DROP POLICY IF EXISTS stores_access ON stores")
    op.execute("""
        CREATE POLICY stores_access ON stores
        USING (
            id IN (
                SELECT store_id FROM user_stores
                WHERE user_id = NULLIF(current_setting('app.user_id', true), '')::uuid
            )
        )
    """)

    # ── 3. user_stores: бачиш власні зв'язки
    op.execute("ALTER TABLE user_stores ENABLE ROW LEVEL SECURITY")
    op.execute("DROP POLICY IF EXISTS user_stores_self ON user_stores")
    op.execute("""
        CREATE POLICY user_stores_self ON user_stores
        USING (user_id = NULLIF(current_setting('app.user_id', true), '')::uuid)
    """)


def downgrade() -> None:
    bind = op.get_bind()
    for table in STORE_TABLES:
        op.execute(f"DROP POLICY IF EXISTS {table}_store_isolation ON {table}")
        op.execute(f"ALTER TABLE {table} DISABLE ROW LEVEL SECURITY")
    op.execute("DROP POLICY IF EXISTS stores_access ON stores")
    op.execute("ALTER TABLE stores DISABLE ROW LEVEL SECURITY")
    op.execute("DROP POLICY IF EXISTS user_stores_self ON user_stores")
    op.execute("ALTER TABLE user_stores DISABLE ROW LEVEL SECURITY")
