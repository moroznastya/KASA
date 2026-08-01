"""Оновити barcode-блок цінника: відступ від ціни 1.5mm + не обрізати підпис QR

ПРОБЛЕМА (підтверджена користувачем):
1. QR-code/штрих-код занадто близько до ціни — потрібен більший відступ
2. Цифри під QR-кодом (підпис) обрізаються по висоті

ЗМІНИ (лише barcode-блок шаблону
'Цінник за замовчуванням (40×25 мм)' id='a0000000-0000-0000-0000-000000000010'):
- margin-top: 0.5px -> margin-top: 1.5mm (штрих-код трішки нижче від ціни)
- max-height: 7mm -> max-height: 10mm (QR 7×7мм + підпис цифр ~2мм = ~9мм вміщуються)
- overflow: hidden залишено як захист (тепер не ріже підпис)

Інші частини шаблону (name, price, рамка body, {{#if}} блоки, article/date)
НЕ змінюються. Інші шаблони НЕ чіпаються.

ІДЕМПОТЕНТНІСТЬ: якщо новий фрагмент вже присутній у content (наприклад, зміна
вже застосована раніше прямим UPDATE), міграція не падає, а завершується успішно.

Revision ID: f89706f0cc19
Revises: f89706f0cc18
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc19'
down_revision: Union[str, None] = 'f89706f0cc18'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

PRICE_TAG_ID = "a0000000-0000-0000-0000-000000000010"

OLD_FRAGMENT = '<div style="margin-top: 0.5px; width: 55%; max-height: 7mm; display: flex; justify-content: center; overflow: hidden;">'
NEW_FRAGMENT = '<div style="margin-top: 1.5mm; width: 55%; max-height: 10mm; display: flex; justify-content: center; overflow: hidden;">'


def upgrade() -> None:
    """Точкова заміна фрагмента barcode-блоку (ідемпотентно)."""
    bind = op.get_bind()
    row = bind.execute(
        text('SELECT content FROM print_templates WHERE id = :id'), {"id": PRICE_TAG_ID}
    ).fetchone()
    if row is None:
        raise RuntimeError(f'Шаблон не знайдено: id={PRICE_TAG_ID}')
    content = row.content
    if NEW_FRAGMENT in content:
        # Зміна вже застосована (напр. прямим UPDATE) — просто підтверджуємо
        return
    new_content = content.replace(OLD_FRAGMENT, NEW_FRAGMENT)
    if new_content == content:
        raise RuntimeError('Фрагмент barcode-блоку не знайдено — зупинено, щоб не пошкодити content')
    bind.execute(
        text('UPDATE print_templates SET content = :content, updated_at = now() WHERE id = :id'),
        {"content": new_content, "id": PRICE_TAG_ID},
    )


def downgrade() -> None:
    """Повернути попередній фрагмент (ідемпотентно)."""
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
