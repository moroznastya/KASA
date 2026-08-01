"""Збільшити запас контейнера штрих-коду: max-height 10mm -> 12mm

ПРОБЛЕМА: barcode контейнер мав max-height: 10mm; overflow: hidden.
QR 7мм + підпис цифр ~2.5мм = 9.5мм — ВПРИТУЛ до 10мм. Потрібен запас,
щоб підпис гарантовано не обрізався (різні шрифти/масштаби).

ЗМІНА (лише barcode-блок шаблону
'Цінник за замовчуванням (40×25 мм)' id='a0000000-0000-0000-0000-000000000010'):
- max-height: 10mm -> max-height: 12mm (QR 7 + підпис 2.5 = 9.5мм << 12мм)

НІЧОГО іншого не змінюється: margin-top: 1.5mm, width: 55%, overflow: hidden,
рамка body, name, price, {{#if}} блоки — все лишається.

ІДЕМПОТЕНТНІСТЬ: якщо max-height: 12mm вже присутній — міграція завершується
успішно без змін (захист від повторного застосування).

Revision ID: f89706f0cc20
Revises: f89706f0cc19
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc20'
down_revision: Union[str, None] = 'f89706f0cc19'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

PRICE_TAG_ID = "a0000000-0000-0000-0000-000000000010"

OLD_FRAGMENT = 'margin-top: 1.5mm; width: 55%; max-height: 10mm; display: flex; justify-content: center; overflow: hidden;'
NEW_FRAGMENT = 'margin-top: 1.5mm; width: 55%; max-height: 12mm; display: flex; justify-content: center; overflow: hidden;'


def upgrade() -> None:
    """Точкова заміна max-height у barcode-блоці (ідемпотентно)."""
    bind = op.get_bind()
    row = bind.execute(
        text('SELECT content FROM print_templates WHERE id = :id'), {"id": PRICE_TAG_ID}
    ).fetchone()
    if row is None:
        raise RuntimeError(f'Шаблон не знайдено: id={PRICE_TAG_ID}')
    content = row.content
    if NEW_FRAGMENT in content:
        return  # вже застосовано
    new_content = content.replace(OLD_FRAGMENT, NEW_FRAGMENT)
    if new_content == content:
        raise RuntimeError('Фрагмент barcode-блоку не знайдено — зупинено, щоб не пошкодити content')
    bind.execute(
        text('UPDATE print_templates SET content = :content, updated_at = now() WHERE id = :id'),
        {"content": new_content, "id": PRICE_TAG_ID},
    )


def downgrade() -> None:
    """Повернути max-height: 10mm (ідемпотентно)."""
    bind = op.get_bind()
    row = bind.execute(
        text('SELECT content FROM print_templates WHERE id = :id'), {"id": PRICE_TAG_ID}
    ).fetchone()
    if row is None:
        return
    content = row.content
    if OLD_FRAGMENT in content:
        return
    new_content = content.replace(NEW_FRAGMENT, OLD_FRAGMENT)
    bind.execute(
        text('UPDATE print_templates SET content = :content, updated_at = now() WHERE id = :id'),
        {"content": new_content, "id": PRICE_TAG_ID},
    )
