"""Оптимізація шаблонів цінників: вертикальний профіль під 25мм висоту

ПРОБЛЕМА (підтверджена): Цінник 40×25мм — контент (назва 2 рядки + ціна +
штрих-код + арт + дата) НЕ вміщувався у 25мм висоту → обрізався низ.
Розрахунок: padding 2мм+1мм + border 1px + назва min-height 22px (≈5.8мм!)
+ ціна 14px (3.7мм) + штрих-код до 7мм + арт/дата по ~1.7мм + відступи
≈ 26-28мм > 25мм.

ЗМІНИ (обидва шаблони цінників):
1. 'a0000000-0000-0000-0000-000000000010' «Цінник за замовчуванням (40×25 мм)»:
   - body padding: 1.5mm 2mm 1mm 2mm (було 2mm 2mm 1mm 2mm)
   - name: прибрано min-height: 22px (1 рядок ~11px), line-clamp 2 збережено
   - name font-size: 9px, line-height: 1.15
   - price: font-size 14px, margin-bottom: 0.5px
   - barcode: max-height: 7mm, margin-top: 0.5px
   - article/date: font-size 6px, margin-top: 0.5px
   - border: 1px solid #000 (рамка) та всі {{змінні}} і {{#if}} блоки збережено

2. '9d9b552d-ccdf-44d7-b9bb-d3cc2240117a' «Цінник стандартний»:
   - додано border: 1px solid #000 на body (раніше рамки не було)
   - контент по центру, відступи зменшено (padding 1.5mm 2mm 1mm 2mm)
   - width/height: 100% (заповнює комірку, рамка по периметру)
   - назва з line-clamp 2, штрих-код width 55% max-height 7mm

Змінюються ТІЛЬКИ 2 записи за id. Чеки 58/80мм, повернення — НЕ чіпаються.
is_active записів НЕ змінюється (custom залишається is_active=False).

Revision ID: f89706f0cc18
Revises: f89706f0cc17
Create Date: 2026-07-31
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc18'
down_revision: Union[str, None] = 'f89706f0cc17'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

PRICE_TAG_ID = "a0000000-0000-0000-0000-000000000010"
CUSTOM_ID = "9d9b552d-ccdf-44d7-b9bb-d3cc2240117a"

# ── Новий content: Цінник за замовчуванням (оптимізований під 25мм) ─────────
PRICE_TAG_CONTENT_NEW = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 1.5mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 9px; font-weight: bold; line-height: 1.15; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 1px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 14px; font-weight: bold; color: #000; margin-bottom: 0.5px; text-align: right; padding-right: 3px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 0.5px; width: 55%; max-height: 7mm; display: flex; justify-content: center; overflow: hidden;">
            {{barcode_image}}
        </div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 6px; color: #000; margin-top: 0.5px;">
            Арт: {{article}}
        </div>
        {{/if}}
        {{#if show_created_date}}
        <div style="font-size: 6px; color: #000; margin-top: 0.5px;">
            {{created_date}}
        </div>
        {{/if}}
    </div>
</body>
</html>"""

# ── Новий content: Цінник стандартний (рамка + центрування + менші відступи) ─
CUSTOM_CONTENT_NEW = """<html>
<body style="font-family: Arial; font-size: 8pt; margin: 0; padding: 1.5mm 2mm 1mm 2mm; background: white; width: 100%; height: 100%; box-sizing: border-box; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; text-align: center;">
        <div style="font-size: 7pt; color: #000; font-weight: bold; line-height: 1.15; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 1px;">
            {{name}}
        </div>
        <div style="font-size: 6pt; color: #000; margin-top: 1px; width: 55%; max-height: 7mm; display: flex; justify-content: center; overflow: hidden;">
            {{barcode_image}}
        </div>
        <div style="font-size: 16pt; color: #000; font-weight: bold; margin-top: 1px;">
            {{price}}
        </div>
    </div>
</body>
</html>"""

# ── Попередній content (для downgrade) ───────────────────────────────────────
PRICE_TAG_CONTENT_OLD = """<html>
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

CUSTOM_CONTENT_OLD = """<html>
<body style="font-family: Arial; font-size:8pt; margin:0; padding:2mm; background:white; width:{{width}}mm; height:{{height}}mm; box-sizing:border-box;">
<div style="display:flex; flex-direction:column; align-items:center; justify-content:center; height:100%; text-align:center;">
<div style="font-size:7pt;color:#000;font-weight:bold;">{{name}}</div>
<div style="font-size:6pt;color:#000;margin-top:2pt;">{{barcode_image}}</div>
<div style="font-size:16pt;color:#000;font-weight:bold;margin-top:2pt;">{{price}}</div>
</div>
</body>
</html>"""


def upgrade() -> None:
    """Оновити content обох шаблонів цінників (за id)."""
    bind = op.get_bind()
    bind.execute(
        text("UPDATE print_templates SET content = :content, updated_at = now() WHERE id = :id"),
        {"content": PRICE_TAG_CONTENT_NEW, "id": PRICE_TAG_ID},
    )
    bind.execute(
        text("UPDATE print_templates SET content = :content, updated_at = now() WHERE id = :id"),
        {"content": CUSTOM_CONTENT_NEW, "id": CUSTOM_ID},
    )


def downgrade() -> None:
    """Повернути попередній content."""
    bind = op.get_bind()
    bind.execute(
        text("UPDATE print_templates SET content = :content, updated_at = now() WHERE id = :id"),
        {"content": PRICE_TAG_CONTENT_OLD, "id": PRICE_TAG_ID},
    )
    bind.execute(
        text("UPDATE print_templates SET content = :content, updated_at = now() WHERE id = :id"),
        {"content": CUSTOM_CONTENT_OLD, "id": CUSTOM_ID},
    )
