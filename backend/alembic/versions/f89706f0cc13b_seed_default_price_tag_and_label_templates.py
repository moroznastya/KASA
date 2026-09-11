"""seed default price_tag and label templates + print settings

Ревізія: f89706f0cc13b
down:    09288dbfd383 (таблиця print_templates + шаблон receipt_58mm)

ЧОМУ: ланцюг міграцій був несамодостатнім — шаблони price_tag/label і
налаштування друку НІКОЛИ не створювались міграціями (тільки UPDATE в
f89706f0cc15+ / f89706f0cc21+). `alembic upgrade head` з чистої БД падав:
- f89706f0cc16: "Шаблон не знайдено: type=price_tag ..."
- f89706f0cc21: "Налаштування не знайдено: key=price_tag_width"

Ця міграція сіє дефолтні шаблони (з мінімальним валідним контентом — наступні
міграції перезапишуть content) і відсутні налаштування друку.
Ідемпотентна: вставляє лише якщо рядка ще немає.
"""

from typing import Sequence, Union

from alembic import op
from sqlalchemy import text

# revision identifiers, used by Alembic.
revision: str = 'f89706f0cc13b'
down_revision: Union[str, None] = '09288dbfd383'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

MINIMAL_CONTENT = """<html>
<body style="font-family: Arial, sans-serif; width: 100%; height: 100%; padding: 2mm; margin: 0; box-sizing: border-box; background: white;">
    <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; text-align: center;">
        <div style="font-size: 14px; font-weight: bold;">{{name}}</div>
        <div style="font-size: 20px; font-weight: bold;">{{price}} грн</div>
    </div>
</body>
</html>"""

TEMPLATES = [
    ("a0000000-0000-0000-0000-000000000010", "Цінник за замовчуванням (40×25 мм)", "price_tag"),
    ("a0000000-0000-0000-0000-000000000011", "Етикетка 60×40 (Code128 + QR)", "label"),
]

# Ключі, які очікують наступні міграції (f89706f0cc21), але не сіються ними.
SETTINGS = [
    ("price_tag_width", "58", "number", "Ширина цінника (мм)"),
    ("price_tag_height", "42", "number", "Висота цінника (мм)"),
    ("label_width", "40", "number", "Ширина етикетки (мм)"),
    ("label_height", "42", "number", "Висота етикетки (мм)"),
    ("price_tag_fields", '["name","price","barcode"]', "string", "Поля цінника"),
    ("label_fields", '["name","price","brand","barcode"]', "string", "Поля етикетки"),
]


def upgrade() -> None:
    """Створити дефолтні шаблони price_tag/label і налаштування, якщо їх немає."""
    bind = op.get_bind()
    for tid, name, ttype in TEMPLATES:
        bind.execute(
            text(
                f"""
                INSERT INTO print_templates
                    (id, name, type, content, variables, is_default, is_active,
                     created_at, updated_at)
                SELECT CAST(:tid AS uuid), :name, '{ttype}', :content, NULL, TRUE,
                       TRUE, now(), now()
                WHERE NOT EXISTS (
                    SELECT 1 FROM print_templates
                    WHERE type = '{ttype}' AND is_default = TRUE
                )
                """
            ),
            {"tid": tid, "name": name, "content": MINIMAL_CONTENT},
        )
    for key, value, vtype, label in SETTINGS:
        bind.execute(
            text(
                f"""
                INSERT INTO system_settings
                    (id, module, key, value, value_type, label, description,
                     options, is_active, created_at, updated_at)
                SELECT gen_random_uuid(), 'printing', '{key}', '{value}',
                       '{vtype}', '{label}', '{label}', NULL, TRUE, now(), now()
                WHERE NOT EXISTS (
                    SELECT 1 FROM system_settings WHERE key = '{key}'
                )
                """
            ),
        )


def downgrade() -> None:
    """Видалити створені seed-шаблони/налаштування (тільки якщо не змінювались)."""
    bind = op.get_bind()
    for tid, name, ttype in TEMPLATES:
        bind.execute(
            text(
                """
                DELETE FROM print_templates
                WHERE id = CAST(:tid AS uuid) AND content = :content
                """
            ),
            {"tid": tid, "content": MINIMAL_CONTENT},
        )
    for key, value, vtype, label in SETTINGS:
        bind.execute(
            text(
                """
                DELETE FROM system_settings
                WHERE key = :key AND value = :value
                """
            ),
            {"key": key, "value": value},
        )
