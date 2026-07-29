"""add_print_templates_table

Revision ID: 09288dbfd383
Revises: d08dc43dd496
Create Date: 2026-07-26 00:06:04.319714+00:00
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

# revision identifiers, used by Alembic.
revision: str = '09288dbfd383'
down_revision: Union[str, None] = 'd08dc43dd496'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Застосувати міграцію."""
    # ── Створення таблиці ──────────────────────
    op.create_table('print_templates',
        sa.Column('id', sa.UUID(), nullable=False,
                  comment='Унікальний ідентифікатор шаблону'),
        sa.Column('name', sa.String(length=255), nullable=False,
                  comment="Назва шаблону (наприклад 'Стандартний 58мм')"),
        sa.Column('type', sa.String(length=20), nullable=False,
                  comment='Тип шаблону: receipt_58mm, receipt_80mm, fiscal, custom'),
        sa.Column('content', sa.Text(), nullable=False,
                  comment='HTML-вміст шаблону з {{змінними}} для підстановки'),
        sa.Column('variables', postgresql.JSONB(astext_type=sa.Text()), nullable=True,
                  comment="JSON з описом змінних: { 'name': 'shop_name', 'label': "
                          "'Назва магазину', 'type': 'string' }"),
        sa.Column('is_default', sa.Boolean(), nullable=False,
                  comment='Чи є шаблоном за замовчуванням для свого type'),
        sa.Column('is_active', sa.Boolean(), nullable=False,
                  comment='Чи активний шаблон (може використовуватись для друку)'),
        sa.Column('created_at', sa.DateTime(timezone=True), nullable=False,
                  comment='Дата створення'),
        sa.Column('updated_at', sa.DateTime(timezone=True), nullable=False,
                  comment='Дата останнього оновлення'),
        sa.PrimaryKeyConstraint('id')
    )
    op.create_index(op.f('ix_print_templates_type'), 'print_templates', ['type'], unique=False)
    op.create_index('uq_print_templates_default_per_type', 'print_templates', ['type'],
                    unique=True, postgresql_where=sa.text('is_default = TRUE'))

    # ── Seed: Стандартний шаблон 58мм ──────────
    op.execute(
        """
        INSERT INTO print_templates (id, name, type, content, variables, is_default, is_active, created_at, updated_at)
        VALUES (
            'a0000000-0000-0000-0000-000000000001',
            'Стандартний 58мм',
            'receipt_58mm',
            '<html>
<body style="font-family: ''DejaVu Sans'', ''Noto Sans'', sans-serif; font-size: 10px; width: 48mm; margin: 0; padding: 0; color: #000; line-height: 1.1; overflow: hidden;">
    <div style="padding: 1px 3px;">

        <!-- Шапка: магазин -->
        <div style="text-align: center; margin-bottom: 2px;">
            <div style="font-size: 22px; font-weight: bold; text-transform: uppercase; margin-bottom: 2px;">{{shop_name}}</div>
            <div style="font-size: 10px;">{{shop_address}}</div>
        </div>

        <div style="border-top: 1px dashed #000; margin: 1px 0;"></div>

        <!-- Номер чеку -->
        <div style="text-align: center; font-size: 15px; font-weight: bold; margin: 2px 0;">
            ЧЕК № {{receipt_number}}
        </div>

        <!-- Дата/час + касир (flex, nowrap) -->
        <div style="display: flex; justify-content: space-between; font-size: 10px; white-space: nowrap; margin: 1px 0;">
            <span>{{date}}</span>
            <span>{{time}}</span>
        </div>
        <div style="font-size: 10px; margin-bottom: 2px;">
            Касир: {{cashier}}
        </div>

        <div style="border-top: 1px dashed #000; margin: 1px 0;"></div>

        <!-- Товари -->
        <div style="width: 100%; margin: 1px 0;">
            {{items}}
        </div>

        <div style="border-top: 1px dashed #000; margin: 1px 0;"></div>

        <!-- Підсумок -->
        <table style="width: 100%; border-collapse: collapse; margin-top: 2px;">
            <tr>
                <td style="font-size: 14px; font-weight: bold; text-align: left;">ДО СПЛАТИ</td>
                <td style="font-size: 14px; font-weight: bold; text-align: right;">{{total}} грн</td>
            </tr>
        </table>

        <!-- Оплата -->
        <table style="width: 100%; border-collapse: collapse; font-size: 10px; margin-top: 2px;">
            <tr>
                <td style="text-align: left;">{{payment_method}}</td>
                <td style="text-align: right;">{{paid}} грн</td>
            </tr>
            <tr>
                <td style="text-align: left;">Решта</td>
                <td style="text-align: right;">{{change}} грн</td>
            </tr>
        </table>

        <div style="border-top: 1px dashed #000; margin: 2px 0 2px 0;"></div>

        <!-- Підвал -->
        <div style="text-align: center; font-size: 11px; font-weight: bold; margin-top: 1px;">
            Дякуємо за покупку!
        </div>
        <div style="text-align: center; font-size: 9px; margin-top: 1px; font-style: italic;">
            Ми цінуємо ваш вибір і сподіваємося побачити вас знову.
        </div>

    </div>
</body>
</html>',
            '[
                {"name":"shop_name","label":"Назва магазину","type":"string"},
                {"name":"shop_address","label":"Адреса магазину","type":"string"},
                {"name":"receipt_number","label":"Номер чека","type":"string"},
                {"name":"date","label":"Дата","type":"string"},
                {"name":"time","label":"Час","type":"string"},
                {"name":"cashier","label":"Касир","type":"string"},
                {"name":"items","label":"Рядки товарів (HTML)","type":"html"},
                {"name":"total","label":"Загальна сума","type":"string"},
                {"name":"payment_method","label":"Спосіб оплати","type":"string"},
                {"name":"paid","label":"Сплачено","type":"string"},
                {"name":"change","label":"Решта","type":"string"},
                {"name":"footer","label":"Нижній колонтитул","type":"string"}
            ]'::jsonb,
            TRUE,
            TRUE,
            NOW(),
            NOW()
        );
        """
    )


def downgrade() -> None:
    """Відкотити міграцію."""
    op.drop_index('uq_print_templates_default_per_type', table_name='print_templates',
                  postgresql_where=sa.text('is_default = TRUE'))
    op.drop_index(op.f('ix_print_templates_type'), table_name='print_templates')
    op.drop_table('print_templates')
