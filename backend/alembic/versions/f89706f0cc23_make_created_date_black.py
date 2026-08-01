"""Зробити дату створення на ціннику та етикетці ЧОРНОГО кольору

КОНТЕКСТ: Шаблони друку зберігаються в БД (таблиця print_templates, колонка
content — HTML з inline CSS). У шаблонах цінника та етикетки рядок дати
створення (created_date, показується при show_created_date) зараз СІРИЙ:
- type='label' name='Етикетка 58×40 мм' (активний): font-size: 6px; color: #666;
  margin-bottom: 0.5mm; text-align: left; → дата
- type='price_tag' name='Цінник 40×43 мм' (активний): font-size: 6px; color: #666;
  margin-bottom: 1mm; text-align: left; → дата

ПЕРЕВІРЕНО: у цих двох шаблонах #666 зустрічається ТІЛЬКИ в рядку дати
(назва/ціна/арт вже #000) — заміна безпечна. Інші шаблони (чеки, custom
'Цінник стандартний' — там вже #000) НЕ чіпаються.

РЕАЛІЗАЦІЯ: ТОЧЕЧНИЙ REPLACE (тільки рядок дати 6px, зберегти користувацькі
правки в інших місцях):
  'font-size: 6px; color: #666' -> 'font-size: 6px; color: #000'
для записів type IN ('label','price_tag'), що містять цей патерн.

downgrade() — зворотний REPLACE ('font-size: 6px; color: #000' ->
'font-size: 6px; color: #666') з тими самими умовами WHERE.

НЕ змінюємо попередні міграції (історія недоторканна).

Revision ID: f89706f0cc23
Revises: f89706f0cc22
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc23'
down_revision: Union[str, None] = 'f89706f0cc22'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Патерн, який присутній у рядку дати обох шаблонів (підтверджено перевіркою)
OLD_PATTERN = 'font-size: 6px; color: #666'
NEW_PATTERN = 'font-size: 6px; color: #000'


def upgrade() -> None:
    """Зробити дату створення чорною (#666 -> #000)."""
    op.execute(
        f"""
        UPDATE print_templates
        SET content = REPLACE(content, '{OLD_PATTERN}', '{NEW_PATTERN}')
        WHERE type IN ('label','price_tag')
          AND content LIKE '%{OLD_PATTERN}%';
        """
    )


def downgrade() -> None:
    """Повернути дату створення сірою (#000 -> #666)."""
    op.execute(
        f"""
        UPDATE print_templates
        SET content = REPLACE(content, '{NEW_PATTERN}', '{OLD_PATTERN}')
        WHERE type IN ('label','price_tag')
          AND content LIKE '%{NEW_PATTERN}%';
        """
    )
