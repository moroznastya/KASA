"""Оновлення дефолтних шаблонів цінника та етикетки (print_templates)

1. Оновлює content шаблону 'Цінник за замовчуванням (40×25 мм)' (type=price_tag):
   - назва товару: clamp до 2 рядків з min-height (line-height 1.2)
   - ціна: зміщена праворуч (text-align:right + padding-right)
   - штрих-код: зменшено до 55% ширини, max-height 12mm, центрування

2. Оновлює content шаблону 'Етикетка 60×40 (Code128 + QR)' (type=label):
   - назва товару: clamp до 2 рядків з min-height (line-height 1.2)
   - ціна: зміщена праворуч (text-align:right + padding-right)
   - штрих-код: зменшено до 45% ширини, max-height 14mm, центрування

3. Деактивує застарілий шаблон 'Цінник стандартний' (type=custom, is_default=True).

Всі змінні та умовні блоки збережено: {{name}}, {{price}}, {{barcode_image}},
{{article}}, {{created_date}}, {{width}}, {{height}} + {{#if show_barcode}},
{{#if show_article}}, {{#if show_created_date}}.

Точкові UPDATE за type+name — інші шаблони (чеки 58/80мм, повернення) не чіпаються.

Revision ID: f89706f0cc15
Revises: f89706f0cc14
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc15'
down_revision: Union[str, None] = 'f89706f0cc14'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# ── Новий content: Цінник 40×25 ──────────────────────────────────────────────
PRICE_TAG_CONTENT_NEW = """<html>
<body style="font-family: Arial, sans-serif; width: {{width}}mm; height: {{height}}mm; padding: 3px; margin: 0; box-sizing: border-box; background: white;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 10px; font-weight: bold; line-height: 1.2; min-height: 24px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 2px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 16px; font-weight: bold; color: #000; margin-bottom: 1px; text-align: right; padding-right: 4px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 2px; width: 55%; max-height: 12mm; display: flex; justify-content: center; overflow: hidden;">
            {{barcode_image}}
        </div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 7px; color: #000;">
            Арт: {{article}}
        </div>
        {{/if}}
        {{#if show_created_date}}
        <div style="font-size: 7px; color: #000; margin-top: 1px;">
            {{created_date}}
        </div>
        {{/if}}
    </div>
</body>
</html>"""

# ── Старий content: Цінник 40×25 (для downgrade) ─────────────────────────────
PRICE_TAG_CONTENT_OLD = """<html>
<body style="font-family: Arial, sans-serif; width: {{width}}mm; height: {{height}}mm; padding: 3px; margin: 0; box-sizing: border-box; background: white;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; text-align: center;">
        <div style="font-size: 10px; font-weight: bold; line-height: 1.2; margin-bottom: 2px; color: #000;">
            {{name}}
        </div>
        <div style="font-size: 16px; font-weight: bold; color: #000; margin-bottom: 1px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 2px; width: 100%; display: flex; justify-content: center;">
            {{barcode_image}}
        </div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 7px; color: #000;">
            Арт: {{article}}
        </div>
        {{/if}}
        {{#if show_created_date}}
        <div style="font-size: 7px; color: #000; margin-top: 1px;">
            {{created_date}}
        </div>
        {{/if}}
    </div>
</body>
</html>"""

# ── Новий content: Етикетка 60×40 ────────────────────────────────────────────
LABEL_CONTENT_NEW = """<html>
<body style="font-family: Arial, sans-serif; width: {{width}}mm; height: {{height}}mm; padding: 3px; margin: 0; box-sizing: border-box; background: white;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 15px; font-weight: bold; line-height: 1.2; min-height: 36px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 2px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 20px; font-weight: bold; color: #000; margin-bottom: 1px; text-align: right; padding-right: 6px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 2px; width: 45%; max-height: 14mm; display: flex; justify-content: center; overflow: hidden;">
            {{barcode_image}}
        </div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 7px; color: #000;">
            Арт: {{article}}
        </div>
        {{/if}}
        {{#if show_created_date}}
        <div style="font-size: 7px; color: #000; margin-top: 1px;">
            {{created_date}}
        </div>
        {{/if}}
    </div>
</body>
</html>"""

# ── Старий content: Етикетка 60×40 (для downgrade) ───────────────────────────
LABEL_CONTENT_OLD = """<html>
<body style="font-family: Arial, sans-serif; width: {{width}}mm; height: {{height}}mm; padding: 3px; margin: 0; box-sizing: border-box; background: white;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; text-align: center;">
        <div style="font-size: 15px; font-weight: bold; line-height: 1.2; margin-bottom: 2px; color: #000;">
            {{name}}
        </div>
        <div style="font-size: 20px; font-weight: bold; color: #000; margin-bottom: 1px; justify-content: right;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 2px; width: 60%; display: flex; justify-content: center;">
            {{barcode_image}}
        </div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 7px; color: #000;">
            Арт: {{article}}
        </div>
        {{/if}}
        {{#if show_created_date}}
        <div style="font-size: 7px; color: #000; margin-top: 1px;">
            {{created_date}}
        </div>
        {{/if}}
    </div>
</body>
</html>"""


def upgrade() -> None:
    """Оновити content двох дефолтних шаблонів + деактивувати custom."""
    bind = op.get_bind()

    # 1. Цінник за замовчуванням (40×25 мм)
    bind.execute(
        text("""
            UPDATE print_templates
            SET content = :content, updated_at = now()
            WHERE type = 'price_tag' AND name = 'Цінник за замовчуванням (40×25 мм)'
        """),
        {"content": PRICE_TAG_CONTENT_NEW},
    )

    # 2. Етикетка 60×40 (Code128 + QR)
    bind.execute(
        text("""
            UPDATE print_templates
            SET content = :content, updated_at = now()
            WHERE type = 'label' AND name = 'Етикетка 60×40 (Code128 + QR)'
        """),
        {"content": LABEL_CONTENT_NEW},
    )

    # 3. Деактивація застарілого 'Цінник стандартний' (custom)
    bind.execute(
        text("""
            UPDATE print_templates
            SET is_active = false, updated_at = now()
            WHERE type = 'custom' AND name = 'Цінник стандартний' AND is_default = true
        """),
    )


def downgrade() -> None:
    """Відновити старі content та реактивувати custom."""
    bind = op.get_bind()

    bind.execute(
        text("""
            UPDATE print_templates
            SET content = :content, updated_at = now()
            WHERE type = 'price_tag' AND name = 'Цінник за замовчуванням (40×25 мм)'
        """),
        {"content": PRICE_TAG_CONTENT_OLD},
    )

    bind.execute(
        text("""
            UPDATE print_templates
            SET content = :content, updated_at = now()
            WHERE type = 'label' AND name = 'Етикетка 60×40 (Code128 + QR)'
        """),
        {"content": LABEL_CONTENT_OLD},
    )

    bind.execute(
        text("""
            UPDATE print_templates
            SET is_active = true, updated_at = now()
            WHERE type = 'custom' AND name = 'Цінник стандартний' AND is_default = true
        """),
    )
