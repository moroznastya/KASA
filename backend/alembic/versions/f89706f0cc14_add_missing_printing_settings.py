"""Додає відсутні ключі модуля printing до system_settings

Додає 6 нових налаштувань друку, які використовує frontend:
  - PrintLabelsPriceTagsPage.tsx: price_tag_template_id, price_tag_gap,
    price_tag_margin, barcode_type (+ label_gap, label_template_id)
  - PrintSettingsPanel.tsx: price_tag_gap, label_gap, price_tag_margin

Міграція ідемпотентна: використовує INSERT ... ON CONFLICT (key) DO NOTHING,
безпечно запускати повторно. Існуючі ключі не змінюються.

Revision ID: f89706f0cc14
Revises: 56b4c1696966
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc14'
down_revision: Union[str, None] = '56b4c1696966'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Нові налаштування модуля printing
NEW_SETTINGS = [
    {
        "key": "price_tag_gap",
        "value": "3",
        "value_type": "number",
        "label": "Відступ між цінниками (мм)",
        "description": "Відступ між цінниками у міліметрах",
        "options": None,
    },
    {
        "key": "label_gap",
        "value": "3",
        "value_type": "number",
        "label": "Відступ між етикетками (мм)",
        "description": "Відступ між етикетками у міліметрах",
        "options": None,
    },
    {
        "key": "price_tag_margin",
        "value": "10",
        "value_type": "number",
        "label": "Поле сторінки для цінників (мм)",
        "description": "Поле сторінки для цінників у міліметрах",
        "options": None,
    },
    {
        "key": "barcode_type",
        "value": "code128",
        "value_type": "select",
        "label": "Тип штрих-коду",
        "description": "Тип штрих-коду за замовчуванням",
        "options": '["code128","ean13","ean8","upc_a","qr"]',
    },
    {
        "key": "price_tag_template_id",
        "value": "",
        "value_type": "string",
        "label": "Шаблон цінника за замовчуванням",
        "description": "ID шаблону цінника (порожньо — використати is_default)",
        "options": None,
    },
    {
        "key": "label_template_id",
        "value": "",
        "value_type": "string",
        "label": "Шаблон етикетки за замовчуванням",
        "description": "ID шаблону етикетки (порожньо — використати is_default)",
        "options": None,
    },
]


def upgrade() -> None:
    """Вставити відсутні ключі (ідемпотентно)."""
    bind = op.get_bind()
    for s in NEW_SETTINGS:
        bind.execute(
            text("""
                INSERT INTO system_settings
                    (id, module, key, value, value_type, label, description,
                     options, is_active, created_at, updated_at)
                VALUES
                    (gen_random_uuid(), 'printing', :key, :value, :value_type,
                     :label, :description, :options, true, now(), now())
                ON CONFLICT (key) DO NOTHING
            """),
            {
                "key": s["key"],
                "value": s["value"],
                "value_type": s["value_type"],
                "label": s["label"],
                "description": s["description"],
                "options": s["options"],
            },
        )


def downgrade() -> None:
    """Видалити додані ключі (тільки якщо значення збігаються з seed)."""
    bind = op.get_bind()
    for s in NEW_SETTINGS:
        bind.execute(
            text("""
                DELETE FROM system_settings
                WHERE key = :key AND value = :value
            """),
            {
                "key": s["key"],
                "value": s["value"],
            },
        )
