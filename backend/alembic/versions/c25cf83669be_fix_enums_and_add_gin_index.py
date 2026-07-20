"""Виправлення ENUM регістру та додавання GIN trigram index.

Виконує:
  1. Додає GIN trigram index ix_products_title_trgm (якщо відсутній)
  2. Виправляє ENUM регістр: UPPERCASE -> lowercase для всіх ENUM типів
  3. Додає колонки status та total_amount до write_offs (якщо відсутні)

Revision ID: c25cf83669be
Revises: 62c0fd0b93a4
Create Date: 2026-07-20 20:16:42.500260+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

# revision identifiers, used by Alembic.
revision: str = 'c25cf83669be'
down_revision: Union[str, None] = '62c0fd0b93a4'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


# ── Маппінг ENUM: старе (UPPERCASE) -> нове (lowercase) ──
ENUM_FIXES = {
    "invoice_status": {
        "old_values": ["DRAFT", "CONFIRMED", "CANCELLED"],
        "new_values": ["draft", "confirmed", "cancelled"],
        "columns": [("invoices", "status")],
    },
    "transfer_status": {
        "old_values": ["DRAFT", "CONFIRMED", "CANCELLED"],
        "new_values": ["draft", "confirmed", "cancelled"],
        "columns": [("transfers", "status")],
    },
    "return_invoice_status": {
        "old_values": ["DRAFT", "CONFIRMED", "CANCELLED"],
        "new_values": ["draft", "confirmed", "cancelled"],
        "columns": [("return_invoices", "status")],
    },
    "receipt_type": {
        "old_values": ["SALE", "RETURN"],
        "new_values": ["sale", "return"],
        "columns": [("receipts", "receipt_type")],
    },
    "write_off_reason": {
        "old_values": ["EXPIRED", "DAMAGED", "DEFECT", "THEFT", "INVENTORY", "OTHER"],
        "new_values": ["expired", "damaged", "defect", "theft", "inventory", "other"],
        "columns": [("write_offs", "reason")],
    },
    "ledger_operation_type": {
        "old_values": ["INVOICE", "PAYMENT", "RETURN", "CORRECTION"],
        "new_values": ["invoice", "payment", "return", "correction"],
        "columns": [("supplier_ledger", "operation_type")],
    },
}


def upgrade() -> None:
    """Застосувати виправлення."""

    # ══════════════════════════════════════════════
    # 1. Додати GIN trigram index (якщо відсутній)
    # ══════════════════════════════════════════════
    op.execute("""
        CREATE INDEX IF NOT EXISTS ix_products_title_trgm
        ON products USING gin (title gin_trgm_ops)
    """)

    # ══════════════════════════════════════════════
    # 2. Додати колонки до write_offs (якщо відсутні)
    # ══════════════════════════════════════════════
    conn = op.get_bind()
    inspector = sa.inspect(conn)
    write_off_columns = [col["name"] for col in inspector.get_columns("write_offs")]

    if "status" not in write_off_columns:
        op.add_column(
            "write_offs",
            sa.Column(
                "status",
                sa.String(20),
                nullable=False,
                server_default="confirmed",
                comment="Статус списання (confirmed за замовчуванням)",
            ),
        )

    if "total_amount" not in write_off_columns:
        op.add_column(
            "write_offs",
            sa.Column(
                "total_amount",
                sa.Numeric(12, 2),
                nullable=True,
                server_default=sa.text("0.00"),
                comment="Загальна сума списання (грн)",
            ),
        )

    # ══════════════════════════════════════════════
    # 3. Виправити ENUM регістр (UPPERCASE -> lowercase)
    # ══════════════════════════════════════════════
    for enum_name, fix in ENUM_FIXES.items():
        old_values = fix["old_values"]
        new_values = fix["new_values"]
        columns = fix["columns"]

        # Перевіряємо, чи існує ENUM тип
        result = conn.execute(
            sa.text(
                "SELECT EXISTS (SELECT 1 FROM pg_type WHERE typname = :enum_name)"
            ),
            {"enum_name": enum_name},
        ).scalar()

        if not result:
            continue

        # Перевіряємо поточні значення ENUM
        current_values = [
            row[0]
            for row in conn.execute(
                sa.text(
                    "SELECT enumlabel FROM pg_enum WHERE enumtypid = "
                    "(SELECT oid FROM pg_type WHERE typname = :enum_name) "
                    "ORDER BY enumsortorder"
                ),
                {"enum_name": enum_name},
            ).fetchall()
        ]

        # Якщо вже lowercase — пропускаємо
        if all(v.islower() for v in current_values):
            continue

        # Створюємо новий ENUM тип з lowercase значеннями
        new_enum_name = f"{enum_name}_new"
        new_values_sql = ", ".join(f"'{v}'" for v in new_values)
        conn.execute(sa.text(f"CREATE TYPE {new_enum_name} AS ENUM ({new_values_sql})"))

        # Оновлюємо кожну колонку, що використовує цей ENUM
        for table, column in columns:
            # Перевіряємо, чи існує колонка
            table_columns = [c["name"] for c in inspector.get_columns(table)]
            if column not in table_columns:
                continue

            # Оновлюємо значення: UPPERCASE -> lowercase
            for old_val, new_val in zip(old_values, new_values):
                conn.execute(
                    sa.text(
                        f"UPDATE {table} SET {column} = :new_val "
                        f"WHERE {column} = :old_val"
                    ),
                    {"new_val": new_val, "old_val": old_val},
                )

            # Змінюємо тип колонки на новий ENUM
            conn.execute(
                sa.text(
                    f"ALTER TABLE {table} ALTER COLUMN {column} "
                    f"TYPE {new_enum_name} USING {column}::text::{new_enum_name}"
                )
            )

        # Видаляємо старий ENUM тип
        conn.execute(sa.text(f"DROP TYPE {enum_name}"))

        # Перейменовуємо новий ENUM на стару назву
        conn.execute(sa.text(f"ALTER TYPE {new_enum_name} RENAME TO {enum_name}"))


def downgrade() -> None:
    """Відкотити виправлення (повернути UPPERCASE ENUM)."""

    conn = op.get_bind()
    inspector = sa.inspect(conn)

    # ══════════════════════════════════════════════
    # 1. Видалити GIN trigram index
    # ══════════════════════════════════════════════
    op.execute("DROP INDEX IF EXISTS ix_products_title_trgm")

    # ══════════════════════════════════════════════
    # 2. Видалити колонки з write_offs
    # ══════════════════════════════════════════════
    write_off_columns = [col["name"] for col in inspector.get_columns("write_offs")]
    if "total_amount" in write_off_columns:
        op.drop_column("write_offs", "total_amount")
    if "status" in write_off_columns:
        op.drop_column("write_offs", "status")

    # ══════════════════════════════════════════════
    # 3. Повернути ENUM до UPPERCASE
    # ══════════════════════════════════════════════
    for enum_name, fix in reversed(list(ENUM_FIXES.items())):
        old_values = fix["old_values"]  # UPPERCASE (оригінал)
        new_values = fix["new_values"]  # lowercase (поточний)
        columns = fix["columns"]

        # Перевіряємо, чи існує ENUM тип
        result = conn.execute(
            sa.text(
                "SELECT EXISTS (SELECT 1 FROM pg_type WHERE typname = :enum_name)"
            ),
            {"enum_name": enum_name},
        ).scalar()

        if not result:
            continue

        # Створюємо старий ENUM тип з UPPERCASE
        old_enum_name = f"{enum_name}_old"
        old_values_sql = ", ".join(f"'{v}'" for v in old_values)
        conn.execute(sa.text(f"CREATE TYPE {old_enum_name} AS ENUM ({old_values_sql})"))

        # Оновлюємо колонки
        for table, column in columns:
            table_columns = [c["name"] for c in inspector.get_columns(table)]
            if column not in table_columns:
                continue

            # Повертаємо UPPERCASE значення
            for new_val, old_val in zip(new_values, old_values):
                conn.execute(
                    sa.text(
                        f"UPDATE {table} SET {column} = :old_val "
                        f"WHERE {column} = :new_val"
                    ),
                    {"old_val": old_val, "new_val": new_val},
                )

            # Змінюємо тип колонки
            conn.execute(
                sa.text(
                    f"ALTER TABLE {table} ALTER COLUMN {column} "
                    f"TYPE {old_enum_name} USING {column}::text::{old_enum_name}"
                )
            )

        # Видаляємо поточний ENUM
        conn.execute(sa.text(f"DROP TYPE {enum_name}"))

        # Перейменовуємо
        conn.execute(sa.text(f"ALTER TYPE {old_enum_name} RENAME TO {enum_name}"))
