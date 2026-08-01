"""Оновити шаблон етикетки до актуального стану (QR 29mm, дата 9px)

КОНТЕКСТ: Жива БД (PostgreSQL pos_system) на alembic_version = f89706f0cc23,
але шаблон етикетки print_templates.id='a0000000-0000-0000-0000-000000000011'
(type='label', name='Етикетка 58×40 мм') було оновлено напряму (psql UPDATE) —
міграцій для цього немає. Ця міграція (cc24) фіксує актуальний стан, щоб
`alembic upgrade head` на свіжій БД відтворював його.

ЗМІНИ (порівняно зі станом після cc23):
- body: прибрано padding (1.5mm 2mm 1mm 2mm);
- дата створення: font-size 6px -> 9px, додано font-weight: bold (колір #000 лишається);
- назва: прибрано height: 2.3em (лишився line-clamp: 2);
- ціна: прибрано margin-bottom: 0.5mm;
- barcode: width 60% -> 85%, max-height 11mm -> 29mm, додано flex-обгортку
  <div style="display: flex; flex-direction: column; align-items: center;">{{barcode_image}}</div>.

ІДЕМПОТЕНТНІСТЬ: UPDATE виконується лише якщо content ще НЕ містить
'max-height: 29mm' (повторне застосування безпечне — 0 рядків).

downgrade(): повертає ПОПЕРЕДНІЙ стан (стан після cc23: дата 6px чорна #000,
max-height 17mm, без flex-обгортки barcode_image, ціна з margin-bottom: 0.5mm,
назва з height: 2.3em, body з padding).

НЕ змінюємо попередні міграції (історія недоторканна).

Revision ID: f89706f0cc24
Revises: f89706f0cc23
Create Date: 2026-08-01
"""
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc24'
down_revision: Union[str, None] = 'f89706f0cc23'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

LABEL_ID = "a0000000-0000-0000-0000-000000000011"

# ─── Актуальний стан (вже в живій БД — бери саме його) ─────────────────────
NEW_LABEL_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        {{#if show_created_date}}
        <div style="width: 100%; font-size: 9px; font-weight: bold; color: #000; margin-bottom: 0.5mm; text-align: left;">{{created_date}}</div>
        {{/if}}
        <div style="width: 100%; font-size: 13px; font-weight: bold; line-height: 1.15; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 0.5mm; color: #000;">{{name}}</div>
        <div style="width: 100%; font-size: 24px; font-weight: bold; color: #000; text-align: center;">{{price}} грн</div>
        {{#if show_barcode}}
        <div style="margin-top: 0.5mm; width: 85%; max-height: 29mm; display: flex; justify-content: center; overflow: hidden;"><div style="display: flex; flex-direction: column; align-items: center;">{{barcode_image}}</div></div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 6px; color: #000; margin-top: 0.5mm;">Арт: {{article}}</div>
        {{/if}}
    </div>
</body>
</html>"""

# ─── Попередній стан (до сьогоднішньої зміни): cc23 + без ручних правок ────
OLD_LABEL_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 1.5mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        {{#if show_created_date}}
        <div style="width: 100%; font-size: 6px; color: #000; margin-bottom: 0.5mm; text-align: left;">{{created_date}}</div>
        {{/if}}
        <div style="width: 100%; font-size: 13px; font-weight: bold; line-height: 1.15; display: -webkit-box; height: 2.3em; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 0.5mm; color: #000;">{{name}}</div>
        <div style="width: 100%; font-size: 24px; font-weight: bold; color: #000; margin-bottom: 0.5mm; text-align: center;">{{price}} грн</div>
        {{#if show_barcode}}
        <div style="margin-top: 0.5mm; width: 60%; max-height: 17mm; display: flex; justify-content: center; overflow: hidden;">{{barcode_image}}</div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 6px; color: #000; margin-top: 0.5mm;">Арт: {{article}}</div>
        {{/if}}
    </div>
</body>
</html>"""


def upgrade() -> None:
    """Привести шаблон етикетки до актуального стану (ідемпотентно)."""
    bind = op.get_bind()
    bind.execute(
        text(
            "UPDATE print_templates SET content = :content, updated_at = now() "
            "WHERE id = :id AND content NOT LIKE '%max-height: 29mm%'"
        ),
        {"content": NEW_LABEL_CONTENT, "id": LABEL_ID},
    )


def downgrade() -> None:
    """Повернути попередній стан шаблону етикетки."""
    bind = op.get_bind()
    bind.execute(
        text(
            "UPDATE print_templates SET content = :content, updated_at = now() "
            "WHERE id = :id"
        ),
        {"content": OLD_LABEL_CONTENT, "id": LABEL_ID},
    )
