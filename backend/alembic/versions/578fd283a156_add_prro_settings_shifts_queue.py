"""add prro settings shifts queue

Додає таблиці ПРРО для Фази 1.1:
  - prro_settings     — ключ-значення налаштувань ПРРО
  - prro_shifts       — зміни ПРРО (аналог касових змін)
  - prro_queue_items  — офлайн-черга фіскальних документів

Створює enum-типи:
  - prro_shift_status ('open', 'closed')
  - prro_queue_status ('pending', 'sent', 'failed')

Revision ID: 578fd283a156
Revises: f89706f0cc26
Create Date: 2026-08-01 12:07:46.522003+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

# revision identifiers, used by Alembic.
revision: str = '578fd283a156'
down_revision: Union[str, None] = 'f89706f0cc26'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # ── prro_settings ───────────────────────────
    op.create_table(
        'prro_settings',
        sa.Column('id', sa.Integer(), autoincrement=True, nullable=False, comment='Унікальний ідентифікатор налаштування'),
        sa.Column('key_name', sa.String(length=100), nullable=False, comment='Ключ налаштування (key_file, key_password_encrypted, key_format, prro_fn, prro_tn, prro_zn, mode, url, last_shift_number, ...)'),
        sa.Column('value', sa.Text(), nullable=True, comment='Значення налаштування (зберігається як текст)'),
        sa.Column('updated_at', sa.DateTime(), nullable=False, comment='Дата останнього оновлення'),
        sa.PrimaryKeyConstraint('id'),
    )
    op.create_index(
        op.f('ix_prro_settings_key_name'),
        'prro_settings',
        ['key_name'],
        unique=True,
    )

    # ── prro_shifts ─────────────────────────────
    op.create_table(
        'prro_shifts',
        sa.Column('id', sa.UUID(), nullable=False, comment='Унікальний ідентифікатор зміни'),
        sa.Column('shift_number', sa.Integer(), nullable=False, comment='Номер зміни'),
        sa.Column('opened_at', sa.DateTime(), nullable=False, comment='Дата/час відкриття зміни'),
        sa.Column('closed_at', sa.DateTime(), nullable=True, comment='Дата/час закриття зміни'),
        sa.Column('signer_serial', sa.String(length=255), nullable=True, comment='Серійний номер КЕП підписанта'),
        sa.Column('signer_name', sa.String(length=255), nullable=True, comment='ПІБ підписанта'),
        sa.Column('closed_by', sa.String(length=255), nullable=True, comment='Хто закрив зміну (касир/старший касир)'),
        sa.Column('zreport_number', sa.String(length=50), nullable=True, comment='Номер Z-звіту'),
        sa.Column('status', sa.Enum('open', 'closed', name='prro_shift_status', create_constraint=True), nullable=False, comment='Статус зміни: open / closed'),
        sa.Column('receipt_count', sa.Integer(), nullable=False, comment='Кількість фіскальних чеків за зміну'),
        sa.Column('total_amount', sa.Numeric(precision=12, scale=2), nullable=False, comment='Обіг за зміну (грн)'),
        sa.Column('last_local_number', sa.Integer(), nullable=False, comment='Останній локальний номер чеку (для контролю послідовності)'),
        sa.Column('last_mac', sa.Text(), nullable=True, comment='MAC/хеш останнього переданого <DAT> (для хеш-ланцюжка)'),
        sa.PrimaryKeyConstraint('id'),
    )
    op.create_index(
        op.f('ix_prro_shifts_shift_number'),
        'prro_shifts',
        ['shift_number'],
        unique=False,
    )

    # ── prro_queue_items ────────────────────────
    op.create_table(
        'prro_queue_items',
        sa.Column('id', sa.UUID(), nullable=False, comment='Унікальний ідентифікатор запису черги'),
        sa.Column('receipt_id', sa.UUID(), nullable=True, comment="Зв'язок з чеком"),
        sa.Column('shift_id', sa.UUID(), nullable=True, comment='Зміна ПРРО, до якої належить документ'),
        sa.Column('local_number', sa.Integer(), nullable=False, comment='Локальний номер документа в межах зміни'),
        sa.Column('check_type', sa.String(length=10), nullable=False, comment='Тип фіскального документа: CHK / ZREPORT / SERVICECHK'),
        sa.Column('xml_body', sa.Text(), nullable=False, comment='Канонічний XML <DAT> (підписаний check_sign)'),
        sa.Column('mac', sa.Text(), nullable=True, comment='Значення MAC (хеш-ланцюжок)'),
        sa.Column('status', sa.Enum('pending', 'sent', 'failed', name='prro_queue_status', create_constraint=True), nullable=False, comment='Статус передачі: pending / sent / failed'),
        sa.Column('error', sa.Text(), nullable=True, comment='Текст помилки при передачі'),
        sa.Column('created_at', sa.DateTime(), nullable=False, comment='Дата створення запису'),
        sa.Column('sent_at', sa.DateTime(), nullable=True, comment='Дата/час успішної передачі'),
        sa.ForeignKeyConstraint(['receipt_id'], ['receipts.id'], ondelete='SET NULL'),
        sa.ForeignKeyConstraint(['shift_id'], ['prro_shifts.id'], ondelete='SET NULL'),
        sa.PrimaryKeyConstraint('id'),
    )
    op.create_index(
        op.f('ix_prro_queue_items_receipt_id'),
        'prro_queue_items',
        ['receipt_id'],
        unique=False,
    )
    op.create_index(
        op.f('ix_prro_queue_items_shift_id'),
        'prro_queue_items',
        ['shift_id'],
        unique=False,
    )
    op.create_index(
        op.f('ix_prro_queue_items_status'),
        'prro_queue_items',
        ['status'],
        unique=False,
    )


def downgrade() -> None:
    """Відкотити міграцію (зворотний порядок)."""
    # ── prro_queue_items ────────────────────────
    op.drop_index(op.f('ix_prro_queue_items_status'), table_name='prro_queue_items')
    op.drop_index(op.f('ix_prro_queue_items_shift_id'), table_name='prro_queue_items')
    op.drop_index(op.f('ix_prro_queue_items_receipt_id'), table_name='prro_queue_items')
    op.drop_table('prro_queue_items')

    # ── prro_shifts ─────────────────────────────
    op.drop_index(op.f('ix_prro_shifts_shift_number'), table_name='prro_shifts')
    op.drop_table('prro_shifts')

    # ── prro_settings ───────────────────────────
    op.drop_index(op.f('ix_prro_settings_key_name'), table_name='prro_settings')
    op.drop_table('prro_settings')

    # ── enum-типи ───────────────────────────────
    op.execute('DROP TYPE IF EXISTS prro_shift_status')
    op.execute('DROP TYPE IF EXISTS prro_queue_status')
