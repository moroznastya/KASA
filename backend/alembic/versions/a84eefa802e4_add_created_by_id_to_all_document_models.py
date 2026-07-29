"""add created_by_id to all document models

Revision ID: a84eefa802e4
Revises: f89706f0cc13
Create Date: 2026-07-25 20:50:02.591297+00:00

Примітка: спочатку додаємо колонку як nullable,
потім заповнюємо ID першого адміна для існуючих рядків,
і лише потім встановлюємо NOT NULL.
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'a84eefa802e4'
down_revision: Union[str, None] = 'f89706f0cc13'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""

    # ── 1. Додаємо колонки як nullable ─────────────
    for table in ['inventories', 'invoices', 'purchase_orders',
                  'return_invoices', 'transfers', 'write_offs']:
        op.add_column(
            table,
            sa.Column('created_by_id', sa.UUID(), nullable=True,
                      comment='Ідентифікатор користувача, який створив документ'),
        )

    # ── 2. Отримуємо ID першого адміна ──────────────
    # Якщо є користувачі, беремо першого; інакше вказуємо uuid нулями
    conn = op.get_bind()
    result = conn.execute(
        sa.text("SELECT id FROM users ORDER BY created_at LIMIT 1")
    )
    row = result.fetchone()
    if row:
        default_user_id = str(row[0])
    else:
        # Якщо користувачів немає — використовуємо нульовий UUID
        # (на практиці має бути хоча б адмін)
        default_user_id = '00000000-0000-0000-0000-000000000000'

    # ── 3. Заповнюємо існуючі рядки ─────────────────
    for table in ['inventories', 'invoices', 'purchase_orders',
                  'return_invoices', 'transfers', 'write_offs']:
        conn.execute(
            sa.text(
                f"UPDATE {table} SET created_by_id = :default_id "
                f"WHERE created_by_id IS NULL"
            ),
            {"default_id": default_user_id},
        )

    # ── 4. Встановлюємо NOT NULL та FK ──────────────
    op.alter_column('inventories', 'created_by_id', nullable=False)
    op.create_foreign_key(
        'fk_inventories_created_by_id', 'inventories', 'users',
        ['created_by_id'], ['id'], ondelete='RESTRICT',
    )

    op.alter_column('invoices', 'created_by_id', nullable=False)
    op.create_foreign_key(
        'fk_invoices_created_by_id', 'invoices', 'users',
        ['created_by_id'], ['id'], ondelete='RESTRICT',
    )

    op.alter_column('purchase_orders', 'created_by_id', nullable=False)
    op.create_foreign_key(
        'fk_purchase_orders_created_by_id', 'purchase_orders', 'users',
        ['created_by_id'], ['id'], ondelete='RESTRICT',
    )

    op.alter_column('return_invoices', 'created_by_id', nullable=False)
    op.create_foreign_key(
        'fk_return_invoices_created_by_id', 'return_invoices', 'users',
        ['created_by_id'], ['id'], ondelete='RESTRICT',
    )

    op.alter_column('transfers', 'created_by_id', nullable=False)
    op.create_foreign_key(
        'fk_transfers_created_by_id', 'transfers', 'users',
        ['created_by_id'], ['id'], ondelete='RESTRICT',
    )

    op.alter_column('write_offs', 'created_by_id', nullable=False)
    op.create_foreign_key(
        'fk_write_offs_created_by_id', 'write_offs', 'users',
        ['created_by_id'], ['id'], ondelete='RESTRICT',
    )


def downgrade() -> None:
    """Відкотити міграцію."""
    for table in ['write_offs', 'transfers', 'return_invoices',
                  'purchase_orders', 'invoices', 'inventories']:
        op.drop_constraint(f'fk_{table}_created_by_id', table, type_='foreignkey')
        op.drop_column(table, 'created_by_id')
