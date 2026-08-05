"""add terminal transaction fields to receipts

Revision ID: 43441ff89b4f
Revises: 257f5e9e1d2a
Create Date: 2026-08-05 13:33:17.155251+00:00

Додає поля для збереження даних банківської транзакції карткового терміналу
(ПриватБанк) у таблиці receipts — зв'язок чека каси з транзакцією терміналу.
Усі колонки nullable, безпечні для існуючих даних.
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = '43441ff89b4f'
down_revision: Union[str, None] = '257f5e9e1d2a'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # ── Дані банківської транзакції карткового терміналу (ПриватБанк) ──
    op.add_column('receipts', sa.Column('terminal_rrn', sa.String(length=32), nullable=True, comment='RRN транзакції терміналу (унікальний номер транзакції банку)'))
    op.add_column('receipts', sa.Column('terminal_approval_code', sa.String(length=16), nullable=True, comment='Код авторизації терміналу'))
    op.add_column('receipts', sa.Column('terminal_invoice_number', sa.String(length=32), nullable=True, comment='Номер чека терміналу (може перевищувати 32-bit, зберігається як рядок)'))
    op.add_column('receipts', sa.Column('terminal_transaction_id', sa.String(length=64), nullable=True, comment='Ідентифікатор транзакції в банку-емітенті (rrnExt / rid)'))
    op.add_column('receipts', sa.Column('terminal_response_code', sa.String(length=8), nullable=True, comment='ResponseCode відповіді терміналу ("0000" — успіх тощо)'))
    op.add_column('receipts', sa.Column('terminal_status', sa.String(length=16), nullable=True, comment='Статус транзакції (trnStatus: "1" — успіх; або наш статус approved/declined/partial/cancelled)'))
    op.add_column('receipts', sa.Column('terminal_receipt', sa.Text(), nullable=True, comment='Повний текст чека терміналу (для друку)'))
    op.add_column('receipts', sa.Column('terminal_card_pan', sa.String(length=32), nullable=True, comment='Маскований номер картки (pan)'))
    op.add_column('receipts', sa.Column('terminal_payment_system', sa.String(length=16), nullable=True, comment='Міжнародна платіжна система (VISA/MasterCard)'))
    op.add_column('receipts', sa.Column('terminal_merchant', sa.String(length=32), nullable=True, comment='Номер мерчанта'))
    op.add_column('receipts', sa.Column('terminal_created_at', sa.DateTime(), nullable=True, comment='Дата/час транзакції від терміналу'))


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_column('receipts', 'terminal_created_at')
    op.drop_column('receipts', 'terminal_merchant')
    op.drop_column('receipts', 'terminal_payment_system')
    op.drop_column('receipts', 'terminal_card_pan')
    op.drop_column('receipts', 'terminal_receipt')
    op.drop_column('receipts', 'terminal_status')
    op.drop_column('receipts', 'terminal_response_code')
    op.drop_column('receipts', 'terminal_transaction_id')
    op.drop_column('receipts', 'terminal_invoice_number')
    op.drop_column('receipts', 'terminal_approval_code')
    op.drop_column('receipts', 'terminal_rrn')
