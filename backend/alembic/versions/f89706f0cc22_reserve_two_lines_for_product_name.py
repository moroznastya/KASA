"""Зарезервувати ДВА рядки для назви товару на цінниках та етикетках

КОНТЕКСТ: Шаблони друку зберігаються в БД (таблиця print_templates, колонка
content — HTML з inline CSS). У шаблонах цінників/етикеток контейнер назви
{{name}} ВЖЕ має обрізання на 2 рядки:
  display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical;
  overflow: hidden; text-overflow: ellipsis;
АЛЕ контейнер НЕ має фіксованої висоти: якщо назва коротка (1 рядок) — наступні
елементи (ціна, штрих-код) піднімаються вище, і макет роз'їжджається між
цінниками.

ВИМОГА: контейнер назви має ЗАВЖДИ займати рівно 2 рядки:
- назва довша за 2 рядки → обрізається (вже працює через line-clamp + overflow: hidden)
- назва коротша → другий рядок порожній (фіксована висота контейнера = 2 × line-height)

РІШЕННЯ: додати height: 2.3em у inline style контейнера назви
(2 рядки × line-height 1.15 = 2.3em; em масштабується від font-size).

ШАБЛОНИ ДЛЯ ОНОВЛЕННЯ (3 записи print_templates, усі мають line-clamp: 2):
- type='label' name='Етикетка 58×40 мм' (активний)
- type='price_tag' name='Цінник 40×43 мм' (активний)
- type='custom' name='Цінник стандартний' (неактивний, але має ту саму структуру)

РЕАЛІЗАЦІЯ: ТОЧЕЧНИЙ REPLACE (НЕ перезаписуємо content повністю — зберігаємо
будь-які користувацькі правки). Патерн
'display: -webkit-box; -webkit-line-clamp: 2;' замінюється на
'display: -webkit-box; height: 2.3em; -webkit-line-clamp: 2;'
лише для записів, які містять line-clamp: 2 і ще НЕ мають height: 2.3em.

downgrade() — зворотний REPLACE з тими самими умовами WHERE.

НЕ змінюємо попередні міграції (історія недоторканна). Інші типи шаблонів
(чеки, повернення) НЕ чіпаються.

Revision ID: f89706f0cc22
Revises: f89706f0cc21
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc22'
down_revision: Union[str, None] = 'f89706f0cc21'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Патерн, який точно присутній у поточних шаблонах цінників/етикеток
OLD_PATTERN = 'display: -webkit-box; -webkit-line-clamp: 2;'
NEW_PATTERN = 'display: -webkit-box; height: 2.3em; -webkit-line-clamp: 2;'


def upgrade() -> None:
    """Додати фіксовану висоту 2.3em контейнеру назви (2 рядки)."""
    op.execute(
        f"""
        UPDATE print_templates
        SET content = REPLACE(content, '{OLD_PATTERN}', '{NEW_PATTERN}')
        WHERE type IN ('label','price_tag','custom')
          AND content LIKE '%line-clamp: 2%'
          AND content NOT LIKE '%height: 2.3em%';
        """
    )


def downgrade() -> None:
    """Прибрати фіксовану висоту (зворотний REPLACE)."""
    op.execute(
        f"""
        UPDATE print_templates
        SET content = REPLACE(content, '{NEW_PATTERN}', '{OLD_PATTERN}')
        WHERE type IN ('label','price_tag','custom')
          AND content LIKE '%height: 2.3em%';
        """
    )
