"""Додає обрамлення (рамку) цінника та етикетки в шаблонах БД

Додає в style тега <body> обох дефолтних шаблонів властивість
'border: 1px solid #000;' та збільшує внутрішній відступ padding 3px -> 4px
(щоб контент не прилягав до рамки).

ВАЖЛИВО: box-sizing: border-box вже присутній — рамка враховується
в розмірах {{width}}mm × {{height}}mm без виходу за межі.

Змінюються ТІЛЬКИ 2 записи:
  1. type='price_tag', name='Цінник за замовчуванням (40×25 мм)'
  2. type='label', name='Етикетка 60×40 (Code128 + QR)'

Всі інші стилі, змінні ({{name}}, {{price}}, {{barcode_image}}, {{article}},
{{created_date}}, {{width}}, {{height}}) та умовні блоки ({{#if show_*}})
залишаються БЕЗ змін. Інші шаблони (чеки 58/80мм, повернення, custom)
НЕ чіпаються.

Revision ID: f89706f0cc16
Revises: f89706f0cc15
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc16'
down_revision: Union[str, None] = 'f89706f0cc15'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Фрагмент body-стилю ДО зміни
OLD_FRAGMENT = 'padding: 3px; margin: 0; box-sizing: border-box; background: white;">'
# Фрагмент body-стилю ПІСЛЯ зміни (рамка + більший відступ)
NEW_FRAGMENT = 'padding: 4px; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">'

TARGETS = [
    ('price_tag', 'Цінник за замовчуванням (40×25 мм)'),
    ('label', 'Етикетка 60×40 (Code128 + QR)'),
]


def upgrade() -> None:
    """Додати рамку до body обох шаблонів."""
    bind = op.get_bind()
    for t, name in TARGETS:
        row = bind.execute(
            text('SELECT content FROM print_templates WHERE type = :t AND name = :n'),
            {"t": t, "n": name},
        ).fetchone()
        if row is None:
            raise RuntimeError(f'Шаблон не знайдено: type={t}, name={name}')
        new_content = row.content.replace(OLD_FRAGMENT, NEW_FRAGMENT)
        if new_content == row.content:
            raise RuntimeError(
                f'Фрагмент body-стилю не знайдено в шаблоні: {name} — '
                f'міграцію зупинено, щоб не пошкодити content'
            )
        bind.execute(
            text("""
                UPDATE print_templates
                SET content = :content, updated_at = now()
                WHERE type = :t AND name = :n
            """),
            {"content": new_content, "t": t, "n": name},
        )


def downgrade() -> None:
    """Прибрати рамку та повернути padding 3px."""
    bind = op.get_bind()
    for t, name in TARGETS:
        row = bind.execute(
            text('SELECT content FROM print_templates WHERE type = :t AND name = :n'),
            {"t": t, "n": name},
        ).fetchone()
        if row is None:
            continue
        new_content = row.content.replace(NEW_FRAGMENT, OLD_FRAGMENT)
        bind.execute(
            text("""
                UPDATE print_templates
                SET content = :content, updated_at = now()
                WHERE type = :t AND name = :n
            """),
            {"content": new_content, "t": t, "n": name},
        )
