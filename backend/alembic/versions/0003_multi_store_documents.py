"""Мультиточковість: store_id на документні таблиці.

Додає store_id (NULLABLE + backfill «Основна точка») до всіх бізнес-таблиць.
SET NOT NULL — окремою міграцією після Етапу 3 (коли Rust-фасад почне
передавати store_id у кожному запиті).

Revision ID: 0003_multi_store_documents
Revises: 0002_multi_store_core
Create Date: 2026-08-20
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql
from sqlalchemy import inspect

# revision identifiers, used by Alembic.
revision: str = "0003_multi_store_documents"
down_revision: Union[str, Sequence[str], None] = "0002_multi_store_core"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Таблиці, які отримують store_id
TABLES = [
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
    inspector = inspect(bind)
    conn = bind
    try:
        main_store_id = conn.execute(
            sa.text("SELECT id FROM stores ORDER BY created_at LIMIT 1")
        ).scalar()
        if not main_store_id:
            raise RuntimeError("Основна точка не знайдена — спершу виконай 0002_multi_store_core")

        for table in TABLES:
            if table not in inspector.get_table_names():
                continue
            cols = {c["name"] for c in inspector.get_columns(table)}
            if "store_id" in cols:
                continue
            # 1) колонка (NULLABLE — зворотна сумісність зі старим кодом)
            op.add_column(
                table,
                sa.Column("store_id", postgresql.UUID(as_uuid=True), nullable=True),
            )
            # 2) backfill основною точкою
            conn.execute(
                sa.text(f"UPDATE {table} SET store_id = :sid WHERE store_id IS NULL"),
                {"sid": main_store_id},
            )
            # 3) FK
            op.create_foreign_key(
                f"fk_{table}_store", table, "stores", ["store_id"], ["id"],
                ondelete="CASCADE",
            )
            # 4) індекс (store_id, created_at) — де created_at є
            cols = {c["name"] for c in inspector.get_columns(table)}
            if "created_at" in cols:
                op.create_index(
                    f"ix_{table}_store_created", table, ["store_id", "created_at"],
                )
            else:
                op.create_index(f"ix_{table}_store", table, ["store_id"])
        # alembic комітить сам (transaction_per_migration=True)
    finally:
        pass


def downgrade() -> None:
    bind = op.get_bind()
    inspector = inspect(bind)
    for table in TABLES:
        if table not in inspector.get_table_names():
            continue
        cols = {c["name"] for c in inspector.get_columns(table)}
        if "store_id" not in cols:
            continue
        op.drop_constraint(f"fk_{table}_store", table, type_="foreignkey")
        op.drop_column(table, "store_id")
