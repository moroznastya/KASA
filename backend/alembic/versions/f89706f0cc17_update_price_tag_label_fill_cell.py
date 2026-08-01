"""Оновлення шаблонів цінника/етикетки: рамка по краях, обрізання, відступ 2мм

ПРОБЛЕМА (підтверджена): body шаблону мав width: {{width}}mm; height: {{height}}mm,
але вставляється ВСЕРЕДИНІ .tag-cell (яку сервіс оновлює: padding:0, border:none).
Також контент центрувався (justify-content:center) -> відступ зверху; контент не
вміщувався по висоті -> обрізались 2-й рядок назви та цифри штрих-коду.

ЗМІНИ (обидва шаблони):
- body: width:100%; height:100% замість {{width}}mm/{{height}}mm -> заповнює
  комірку, рамка border:1px solid #000 видима по периметру
- padding: 2mm 2mm 1mm 2mm -> назва починається з відступом 2мм зверху
- justify-content: flex-start замість center -> контент зверху, без центрування
- Назва: font зменшено (9px/14px) + min-height для 2 рядків + line-clamp:2 +
  ellipsis + word-break
- Ціна: праворуч (text-align:right)
- Штрих-код: width 55%/45%, max-height 9mm/12mm (штрихи + цифри вміщуються)
- ВСІ змінні ({{name}}, {{price}}, {{barcode_image}}, {{article}}, {{created_date}})
  та {{#if show_*}} блоки збережено

Змінюються ТІЛЬКИ 2 записи:
  1. type='price_tag', name='Цінник за замовчуванням (40×25 мм)'
  2. type='label', name='Етикетка 60×40 (Code128 + QR)'

Чеки 58/80мм, повернення, custom — НЕ чіпаються.

Revision ID: f89706f0cc17
Revises: f89706f0cc16
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc17'
down_revision: Union[str, None] = 'f89706f0cc16'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# ── НОВИЙ content: Цінник 40×25 ──────────────────────────────────────────────
PRICE_TAG_CONTENT_NEW = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 2mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 9px; font-weight: bold; line-height: 1.2; min-height: 22px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 1px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 14px; font-weight: bold; color: #000; margin-bottom: 1px; text-align: right; padding-right: 3px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 1px; width: 55%; max-height: 9mm; display: flex; justify-content: center; overflow: hidden;">
            {{barcode_image}}
        </div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 6.5px; color: #000; margin-top: 1px;">
            Арт: {{article}}
        </div>
        {{/if}}
        {{#if show_created_date}}
        <div style="font-size: 6.5px; color: #000; margin-top: 1px;">
            {{created_date}}
        </div>
        {{/if}}
    </div>
</body>
</html>"""

# ── НОВИЙ content: Етикетка 60×40 ────────────────────────────────────────────
LABEL_CONTENT_NEW = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 2mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 14px; font-weight: bold; line-height: 1.2; min-height: 34px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 2px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 18px; font-weight: bold; color: #000; margin-bottom: 1px; text-align: right; padding-right: 4px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 1px; width: 45%; max-height: 12mm; display: flex; justify-content: center; overflow: hidden;">
            {{barcode_image}}
        </div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 7px; color: #000; margin-top: 1px;">
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

# ── Попередній content: Цінник (для downgrade, стан після f89706f0cc16) ─────
PRICE_TAG_CONTENT_OLD = """<html>
<body style="font-family: Arial, sans-serif; width: {{width}}mm; height: {{height}}mm; padding: 4px; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
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

# ── Попередній content: Етикетка (для downgrade, стан після f89706f0cc16) ───
LABEL_CONTENT_OLD = """<html>
<body style="font-family: Arial, sans-serif; width: {{width}}mm; height: {{height}}mm; padding: 4px; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
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

TARGETS = [
    ('price_tag', 'Цінник за замовчуванням (40×25 мм)'),
    ('label', 'Етикетка 60×40 (Code128 + QR)'),
]


def upgrade() -> None:
    """Замінити content двох шаблонів на новий (width:100%/height:100%, 2мм)."""
    bind = op.get_bind()
    contents = {
        'price_tag': PRICE_TAG_CONTENT_NEW,
        'label': LABEL_CONTENT_NEW,
    }
    for t, name in TARGETS:
        bind.execute(
            text("""
                UPDATE print_templates
                SET content = :content, updated_at = now()
                WHERE type = :t AND name = :n
            """),
            {"content": contents[t], "t": t, "n": name},
        )


def downgrade() -> None:
    """Повернути попередній content (з {{width}}mm/{{height}}mm та padding 4px)."""
    bind = op.get_bind()
    contents = {
        'price_tag': PRICE_TAG_CONTENT_OLD,
        'label': LABEL_CONTENT_OLD,
    }
    for t, name in TARGETS:
        bind.execute(
            text("""
                UPDATE print_templates
                SET content = :content, updated_at = now()
                WHERE type = :t AND name = :n
            """),
            {"content": contents[t], "t": t, "n": name},
        )
