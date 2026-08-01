"""Оновити шаблони цінників/етикеток (40×43 / 58×40) + settings.

ПРОБЛЕМА: цінники друкувались у розмірі 58×42 мм (ширші за потрібне),
етикетки — 40×42 мм (замалі). За новими вимогами:
  - Цінник: 40×43 мм (новий дизайн: дата ВГОРІ дрібним, ціна НАЙБІЛЬШИМ шрифтом,
    штрих-код/QR з підписом);
  - Етикетка: 58×40 мм (та сама структура, компактніша);
  - Дублікат 'Етикетка 60×40 (Code128 + QR) (копія)' — видаляється;
  - system_settings (розміри, template_id, поля) — оновлюються.

ІДЕМПОТЕНТНІСТЬ: кожна зміна перевіряє поточне значення; якщо цільове вже
присутнє — пропускається (повторне застосування безпечне).

downgrade(): повертає попередні значення (name/content/settings)
та відновлює видалений дублікат.

НЕ чіпає шаблони чеків (receipt_58mm, receipt_80mm, fiscal,
return_receipt_58mm, custom).

Revision ID: f89706f0cc21
Revises: f89706f0cc20
Create Date: 2026-07-31
"""
from datetime import datetime
from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc21'
down_revision: Union[str, None] = 'f89706f0cc20'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# ─── Ідентифікатори ─────────────────────────────────────────────────────────
PRICE_TAG_ID = "a0000000-0000-0000-0000-000000000010"
LABEL_ID = "a0000000-0000-0000-0000-000000000011"
DUP_LABEL_ID = "e3f3d129-c4bc-4617-aef9-46586cf2897b"

# ─── Шаблон Цінник ──────────────────────────────────────────────────────────
OLD_PRICE_TAG_NAME = "Цінник за замовчуванням (40×25 мм)"
NEW_PRICE_TAG_NAME = "Цінник 40×43 мм"

OLD_PRICE_TAG_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 1.5mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 15px; font-weight: bold; line-height: 1.15; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 1px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 22px; font-weight: bold; color: #000; margin-bottom: 0.5px; text-align: right; padding-right: 3px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 1.5mm; width: 60%; max-height: 30mm; display: flex; justify-content: center; overflow: hidden;">
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

NEW_PRICE_TAG_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 1.5mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        {{#if show_created_date}}
        <div style="width: 100%; font-size: 6px; color: #666; margin-bottom: 1mm; text-align: left;">{{created_date}}</div>
        {{/if}}
        <div style="width: 100%; font-size: 15px; font-weight: bold; line-height: 1.15; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 1mm; color: #000;">{{name}}</div>
        <div style="width: 100%; font-size: 30px; font-weight: bold; color: #000; margin-bottom: 1mm; text-align: center;">{{price}} грн</div>
        {{#if show_barcode}}
        <div style="margin-top: 1mm; width: 70%; max-height: 12mm; display: flex; justify-content: center; overflow: hidden;">{{barcode_image}}</div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 7px; color: #000; margin-top: 0.5mm;">Арт: {{article}}</div>
        {{/if}}
    </div>
</body>
</html>"""

# ─── Шаблон Етикетка ────────────────────────────────────────────────────────
OLD_LABEL_NAME = "Етикетка 60×40 (Code128 + QR)"
NEW_LABEL_NAME = "Етикетка 58×40 мм"

OLD_LABEL_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 2mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 14px; font-weight: bold; line-height: 1.2; min-height: 34px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 2px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 18px; font-weight: bold; color: #000; margin-bottom: 1px; text-align: right; padding-right: 4px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 1px; width: 45%; max-height: 20mm; display: flex; justify-content: center; overflow: hidden;">
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

NEW_LABEL_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 1.5mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        {{#if show_created_date}}
        <div style="width: 100%; font-size: 6px; color: #666; margin-bottom: 0.5mm; text-align: left;">{{created_date}}</div>
        {{/if}}
        <div style="width: 100%; font-size: 13px; font-weight: bold; line-height: 1.15; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 0.5mm; color: #000;">{{name}}</div>
        <div style="width: 100%; font-size: 24px; font-weight: bold; color: #000; margin-bottom: 0.5mm; text-align: center;">{{price}} грн</div>
        {{#if show_barcode}}
        <div style="margin-top: 0.5mm; width: 60%; max-height: 11mm; display: flex; justify-content: center; overflow: hidden;">{{barcode_image}}</div>
        {{/if}}
        {{#if show_article}}
        <div style="font-size: 6px; color: #000; margin-top: 0.5mm;">Арт: {{article}}</div>
        {{/if}}
    </div>
</body>
</html>"""

# ─── Дублікат (видаляється в upgrade, відновлюється в downgrade) ────────────
DUP_LABEL_NAME = "Етикетка 60×40 (Code128 + QR) (копія)"
DUP_LABEL_CREATED_AT = datetime.fromisoformat("2026-07-31 18:14:02.707055+03:00")

DUP_LABEL_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 2mm 2mm 1mm 2mm; margin: 0; box-sizing: border-box; background: white; border: 1px solid #000;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: flex-start; height: 100%; text-align: center;">
        <div style="width: 100%; font-size: 14px; font-weight: bold; line-height: 1.2; min-height: 34px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: ellipsis; word-break: break-word; margin-bottom: 2px; color: #000;">
            {{name}}
        </div>
        <div style="width: 100%; font-size: 18px; font-weight: bold; color: #000; margin-bottom: 1px; text-align: right; padding-right: 4px;">
            {{price}} грн
        </div>
        {{#if show_barcode}}
        <div style="margin-top: 1px; width: 60%; max-height: 20mm; display: flex; justify-content: center; overflow: hidden;">
            {{barcode_image}}
        </div>
        {{#if show_created_date}}
        <div style="font-size: 7px; color: #000; margin-top: 1px;">
            {{created_date}}
        </div>
        {{/if}}
    </div>
</body>
</html>"""

# ─── system_settings: (old, new) ─────────────────────────────────────────────
SETTINGS_UPDATE = {
    "price_tag_width": ("58", "40"),
    "price_tag_height": ("42", "43"),
    "label_width": ("40", "58"),
    "label_height": ("42", "40"),
    "label_template_id": (
        "e3f3d129-c4bc-4617-aef9-46586cf2897b",
        "a0000000-0000-0000-0000-000000000011",
    ),
    "price_tag_fields": (
        '["name","price","barcode"]',
        '["name","price","barcode","created_date"]',
    ),
    "label_fields": (
        '["name","price","brand","barcode"]',
        '["name","price","barcode","created_date"]',
    ),
}


# ─── Допоміжні функції (ідемпотентні) ───────────────────────────────────────
def _update_template(bind, template_id: str, name: str, content: str) -> None:
    """Оновлює name/content шаблону, якщо content ще не дорівнює цільовому."""
    row = bind.execute(
        text("SELECT content FROM print_templates WHERE id = :id"),
        {"id": template_id},
    ).fetchone()
    if row is None:
        raise RuntimeError(f"Шаблон не знайдено: id={template_id}")
    if row.content == content:
        return  # вже застосовано
    bind.execute(
        text(
            "UPDATE print_templates SET name = :name, content = :content, "
            "updated_at = now() WHERE id = :id"
        ),
        {"name": name, "content": content, "id": template_id},
    )


def _update_setting(bind, key: str, value: str) -> None:
    """Оновлює значення system_settings, якщо воно ще не цільове."""
    row = bind.execute(
        text("SELECT value FROM system_settings WHERE key = :key"),
        {"key": key},
    ).fetchone()
    if row is None:
        raise RuntimeError(f"Налаштування не знайдено: key={key}")
    if row.value == value:
        return  # вже застосовано
    bind.execute(
        text(
            "UPDATE system_settings SET value = :value, updated_at = now() "
            "WHERE key = :key"
        ),
        {"value": value, "key": key},
    )


def upgrade() -> None:
    """Нові шаблони (40×43 / 58×40), видалення дубліката, оновлення settings."""
    bind = op.get_bind()

    # 1) Цінник: name + content
    _update_template(bind, PRICE_TAG_ID, NEW_PRICE_TAG_NAME, NEW_PRICE_TAG_CONTENT)

    # 2) Етикетка: name + content
    _update_template(bind, LABEL_ID, NEW_LABEL_NAME, NEW_LABEL_CONTENT)

    # 3) Видалити дублікат (ідемпотентно: якщо вже немає — нічого)
    bind.execute(
        text("DELETE FROM print_templates WHERE id = :id"),
        {"id": DUP_LABEL_ID},
    )

    # 4) system_settings (7 ключів)
    for key, (_old, new) in SETTINGS_UPDATE.items():
        _update_setting(bind, key, new)


def downgrade() -> None:
    """Повертає попередні шаблони/назви/settings та відновлює дублікат."""
    bind = op.get_bind()

    # 1) Цінник: попередні name + content
    _update_template(bind, PRICE_TAG_ID, OLD_PRICE_TAG_NAME, OLD_PRICE_TAG_CONTENT)

    # 2) Етикетка: попередні name + content
    _update_template(bind, LABEL_ID, OLD_LABEL_NAME, OLD_LABEL_CONTENT)

    # 3) Відновити дублікат (якщо ще не існує)
    row = bind.execute(
        text("SELECT 1 FROM print_templates WHERE id = :id"),
        {"id": DUP_LABEL_ID},
    ).fetchone()
    if row is None:
        bind.execute(
            text(
                "INSERT INTO print_templates "
                "(id, name, type, content, variables, is_default, is_active, "
                "created_at, updated_at) "
                "VALUES (:id, :name, 'label', :content, NULL, false, true, "
                ":created_at, now())"
            ),
            {
                "id": DUP_LABEL_ID,
                "name": DUP_LABEL_NAME,
                "content": DUP_LABEL_CONTENT,
                "created_at": DUP_LABEL_CREATED_AT,
            },
        )

    # 4) system_settings: попередні значення
    for key, (old, _new) in SETTINGS_UPDATE.items():
        _update_setting(bind, key, old)
