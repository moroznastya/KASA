"""Початкова міграція: створення всіх таблиць.

Створює повну схему БД для Torgashka POS:
  - Довідники: users, categories, suppliers
  - Товари: products, barcodes, product_images
  - Документи: invoices, transfers, write_offs, return_invoices
  - Продажі: receipts
  - Взаєморозрахунки: supplier_ledger

Revision ID: 0001
Revises:
Create Date: 2025-01-01 00:00:00.000000
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

# revision identifiers, used by Alembic.
revision: str = "0001"
down_revision: Union[str, None] = None
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Створення всіх таблиць."""

    # ── Розширення UUID та pg_trgm ──
    op.execute('CREATE EXTENSION IF NOT EXISTS "uuid-ossp"')
    op.execute('CREATE EXTENSION IF NOT EXISTS "pg_trgm"')

    # ══════════════════════════════════════════════
    # 1. ДОВІДНИКИ
    # ══════════════════════════════════════════════

    # ── Таблиця: categories ──────────────────────
    op.create_table(
        "categories",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("name", sa.String(255), nullable=False, index=True),
        sa.Column("description", sa.Text(), nullable=True),
        sa.Column("parent_id", postgresql.UUID(as_uuid=True), nullable=True,
                  index=True),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["parent_id"], ["categories.id"],
                                ondelete="SET NULL"),
    )

    # ── Таблиця: suppliers ───────────────────────
    op.create_table(
        "suppliers",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("name", sa.String(255), nullable=False, index=True),
        sa.Column("edrpou", sa.String(10), nullable=True),
        sa.Column("phone", sa.String(20), nullable=True),
        sa.Column("email", sa.String(255), nullable=True),
        sa.Column("address", sa.Text(), nullable=True),
        sa.Column("notes", sa.Text(), nullable=True),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
    )

    # ── Таблиця: users ───────────────────────────
    op.create_table(
        "users",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("name", sa.String(255), nullable=False),
        sa.Column("login", sa.String(100), unique=True, nullable=False,
                  index=True),
        sa.Column("password_hash", sa.String(255), nullable=False),
        sa.Column("pin_code", sa.String(255), nullable=True),
        sa.Column("role", sa.Enum("admin", "cashier", name="user_role"),
                  nullable=False, server_default="cashier"),
        sa.Column("is_active", sa.Boolean(), nullable=False,
                  server_default=sa.text("true")),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
    )

    # ══════════════════════════════════════════════
    # 2. ТОВАРИ
    # ══════════════════════════════════════════════

    # ── Таблиця: products ────────────────────────
    op.create_table(
        "products",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("barcode", sa.String(50), unique=True, nullable=True,
                  index=True),
        sa.Column("sku", sa.String(100), unique=True, nullable=True,
                  index=True),
        sa.Column("title", sa.String(255), nullable=False, index=True),
        sa.Column("description", sa.Text(), nullable=True),
        sa.Column("price", sa.Numeric(10, 2), nullable=True,
                  server_default=sa.text("0.00")),
        sa.Column("cost_price", sa.Numeric(10, 2), nullable=True,
                  server_default=sa.text("0.00")),
        sa.Column("stock", sa.Numeric(10, 3), nullable=True,
                  server_default=sa.text("0.000")),
        sa.Column("uktzed", sa.String(10), nullable=True),
        sa.Column("scan_excise", sa.Boolean(), nullable=False,
                  server_default=sa.text("false")),
        sa.Column("tax_rate", sa.Numeric(5, 2), nullable=True,
                  server_default=sa.text("20.00")),
        sa.Column("tax_group", sa.String(2), nullable=True,
                  server_default="А"),
        sa.Column("is_weight", sa.Boolean(), nullable=False,
                  server_default=sa.text("false")),
        sa.Column("unit", sa.String(10), nullable=True,
                  server_default="шт"),
        sa.Column("category_id", postgresql.UUID(as_uuid=True), nullable=True,
                  index=True),
        sa.Column("supplier_id", postgresql.UUID(as_uuid=True), nullable=True,
                  index=True),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["category_id"], ["categories.id"],
                                ondelete="SET NULL"),
        sa.ForeignKeyConstraint(["supplier_id"], ["suppliers.id"],
                                ondelete="SET NULL"),
    )

    # ── Таблиця: barcodes ────────────────────────
    op.create_table(
        "barcodes",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("barcode", sa.String(50), unique=True, nullable=False,
                  index=True),
        sa.Column("is_primary", sa.Boolean(), nullable=False,
                  server_default=sa.text("false")),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                ondelete="CASCADE"),
    )

    # ── Таблиця: product_images ──────────────────
    op.create_table(
        "product_images",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("url", sa.String(1024), nullable=False),
        sa.Column("is_main", sa.Boolean(), nullable=False,
                  server_default=sa.text("false")),
        sa.Column("sort_order", sa.Integer(), nullable=False,
                  server_default=sa.text("0")),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                ondelete="CASCADE"),
    )

    # ══════════════════════════════════════════════
    # 3. ДОКУМЕНТИ
    # ══════════════════════════════════════════════

    # ── Таблиця: invoices (прибуткові накладні) ──
    op.create_table(
        "invoices",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("number", sa.String(50), nullable=False, index=True),
        sa.Column("supplier_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("invoice_date", sa.DateTime(), nullable=False),
        sa.Column("status",
                  sa.Enum("draft", "confirmed", "cancelled",
                          name="invoice_status"),
                  nullable=False, server_default="draft"),
        sa.Column("notes", sa.Text(), nullable=True),
        sa.Column("total_amount", sa.Numeric(12, 2), nullable=True,
                  server_default=sa.text("0.00")),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["supplier_id"], ["suppliers.id"],
                                ondelete="RESTRICT"),
    )

    # ── Таблиця: invoice_items ───────────────────
    op.create_table(
        "invoice_items",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("invoice_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("quantity", sa.Numeric(10, 3), nullable=False),
        sa.Column("price", sa.Numeric(10, 2), nullable=False),
        sa.Column("total", sa.Numeric(12, 2), nullable=False),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["invoice_id"], ["invoices.id"],
                                ondelete="CASCADE"),
        sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                ondelete="RESTRICT"),
    )

    # ── Таблиця: transfers (переміщення) ─────────
    op.create_table(
        "transfers",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("number", sa.String(50), nullable=False, index=True),
        sa.Column("from_location", sa.String(255), nullable=False),
        sa.Column("to_location", sa.String(255), nullable=False),
        sa.Column("transfer_date", sa.DateTime(), nullable=False),
        sa.Column("status",
                  sa.Enum("draft", "confirmed", "cancelled",
                          name="transfer_status"),
                  nullable=False, server_default="draft"),
        sa.Column("notes", sa.Text(), nullable=True),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
    )

    # ── Таблиця: transfer_items ──────────────────
    op.create_table(
        "transfer_items",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("transfer_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("quantity", sa.Numeric(10, 3), nullable=False),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["transfer_id"], ["transfers.id"],
                                ondelete="CASCADE"),
        sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                ondelete="RESTRICT"),
    )

    # ── Таблиця: write_offs (списання) ───────────
    op.create_table(
        "write_offs",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("number", sa.String(50), nullable=False, index=True),
        sa.Column("reason",
                  sa.Enum("expired", "damaged", "defect", "theft",
                          "inventory", "other", name="write_off_reason"),
                  nullable=False),
        sa.Column("write_off_date", sa.DateTime(), nullable=False),
        sa.Column("notes", sa.Text(), nullable=True),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
    )

    # ── Таблиця: write_off_items ─────────────────
    op.create_table(
        "write_off_items",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("write_off_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("quantity", sa.Numeric(10, 3), nullable=False),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["write_off_id"], ["write_offs.id"],
                                ondelete="CASCADE"),
        sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                ondelete="RESTRICT"),
    )

    # ── Таблиця: return_invoices (повернення постачальнику) ──
    op.create_table(
        "return_invoices",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("number", sa.String(50), nullable=False, index=True),
        sa.Column("supplier_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("return_date", sa.DateTime(), nullable=False),
        sa.Column("status",
                  sa.Enum("draft", "confirmed", "cancelled",
                          name="return_invoice_status"),
                  nullable=False, server_default="draft"),
        sa.Column("notes", sa.Text(), nullable=True),
        sa.Column("total_amount", sa.Numeric(12, 2), nullable=True,
                  server_default=sa.text("0.00")),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.Column("updated_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["supplier_id"], ["suppliers.id"],
                                ondelete="RESTRICT"),
    )

    # ── Таблиця: return_invoice_items ────────────
    op.create_table(
        "return_invoice_items",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("return_invoice_id", postgresql.UUID(as_uuid=True),
                  nullable=False, index=True),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("quantity", sa.Numeric(10, 3), nullable=False),
        sa.Column("price", sa.Numeric(10, 2), nullable=False),
        sa.Column("total", sa.Numeric(12, 2), nullable=False),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["return_invoice_id"], ["return_invoices.id"],
                                ondelete="CASCADE"),
        sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                ondelete="RESTRICT"),
    )

    # ══════════════════════════════════════════════
    # 4. ПРОДАЖІ
    # ══════════════════════════════════════════════

    # ── Таблиця: receipts (чеки) ─────────────────
    op.create_table(
        "receipts",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("receipt_number", sa.String(50), nullable=False, index=True),
        sa.Column("receipt_type",
                  sa.Enum("sale", "return", name="receipt_type"),
                  nullable=False, server_default="sale"),
        sa.Column("cashier_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("total_amount", sa.Numeric(12, 2), nullable=False),
        sa.Column("is_return", sa.Boolean(), nullable=False,
                  server_default=sa.text("false")),
        sa.Column("notes", sa.Text(), nullable=True),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["cashier_id"], ["users.id"],
                                ondelete="RESTRICT"),
    )

    # ── Таблиця: receipt_items ───────────────────
    op.create_table(
        "receipt_items",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("receipt_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("quantity", sa.Numeric(10, 3), nullable=False),
        sa.Column("price", sa.Numeric(10, 2), nullable=False),
        sa.Column("total", sa.Numeric(12, 2), nullable=False),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["receipt_id"], ["receipts.id"],
                                ondelete="CASCADE"),
        sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                ondelete="RESTRICT"),
    )

    # ══════════════════════════════════════════════
    # 5. ВЗАЄМОРОЗРАХУНКИ
    # ══════════════════════════════════════════════

    # ── Таблиця: supplier_ledger ─────────────────
    op.create_table(
        "supplier_ledger",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                  server_default=sa.text("uuid_generate_v4()")),
        sa.Column("supplier_id", postgresql.UUID(as_uuid=True), nullable=False,
                  index=True),
        sa.Column("operation_type",
                  sa.Enum("invoice", "payment", "return", "correction",
                          name="ledger_operation_type"),
                  nullable=False),
        sa.Column("document_id", postgresql.UUID(as_uuid=True), nullable=True),
        sa.Column("document_number", sa.String(50), nullable=True),
        sa.Column("amount", sa.Numeric(12, 2), nullable=False),
        sa.Column("balance_after", sa.Numeric(12, 2), nullable=False),
        sa.Column("operation_date", sa.DateTime(), nullable=False),
        sa.Column("notes", sa.Text(), nullable=True),
        sa.Column("created_at", sa.DateTime(), nullable=False,
                  server_default=sa.func.now()),
        sa.ForeignKeyConstraint(["supplier_id"], ["suppliers.id"],
                                ondelete="RESTRICT"),
    )

    # ══════════════════════════════════════════════
    # ІНДЕКСИ
    # ══════════════════════════════════════════════

    # GIN trigram index для нечіткого пошуку товарів
    op.create_index("ix_products_title_trgm", "products", ["title"],
                    postgresql_using="gin",
                    postgresql_ops={"title": "gin_trgm_ops"})


def downgrade() -> None:
    """Видалення всіх таблиць (відкат міграції)."""

    # Видаляємо в зворотному порядку (спочатку дочірні)
    op.drop_table("supplier_ledger")
    op.drop_table("receipt_items")
    op.drop_table("receipts")
    op.drop_table("return_invoice_items")
    op.drop_table("return_invoices")
    op.drop_table("write_off_items")
    op.drop_table("write_offs")
    op.drop_table("transfer_items")
    op.drop_table("transfers")
    op.drop_table("invoice_items")
    op.drop_table("invoices")
    op.drop_table("product_images")
    op.drop_table("barcodes")
    op.drop_table("products")
    op.drop_table("users")
    op.drop_table("suppliers")
    op.drop_table("categories")

    # Видаляємо кастомні типи
    op.execute('DROP TYPE IF EXISTS "user_role"')
    op.execute('DROP TYPE IF EXISTS "invoice_status"')
    op.execute('DROP TYPE IF EXISTS "transfer_status"')
    op.execute('DROP TYPE IF EXISTS "write_off_reason"')
    op.execute('DROP TYPE IF EXISTS "return_invoice_status"')
    op.execute('DROP TYPE IF EXISTS "receipt_type"')
    op.execute('DROP TYPE IF EXISTS "ledger_operation_type"')
