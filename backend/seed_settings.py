"""
Seed-дані для системних налаштувань Torgashka POS.
Заповнює таблицю system_settings початковими значеннями.
"""
import uuid
from datetime import UTC, datetime

from sqlalchemy import create_engine, text
from sqlalchemy.orm import Session

from app.config import Settings

settings = Settings()

DEFAULT_SETTINGS = [
    # ── 1. Загальні (General) ──────────────────────────
    {
        "module": "general",
        "key": "company_name",
        "value": "Мій магазин",
        "value_type": "string",
        "label": "Назва магазину",
        "description": "Виводиться в чеках, заголовках",
        "options": None,
    },
    {
        "module": "general",
        "key": "company_address",
        "value": "",
        "value_type": "string",
        "label": "Адреса",
        "description": "Для фіскальних чеків",
        "options": None,
    },
    {
        "module": "general",
        "key": "company_phone",
        "value": "",
        "value_type": "string",
        "label": "Телефон",
        "description": "Для контактів",
        "options": None,
    },
    {
        "module": "general",
        "key": "company_edrpou",
        "value": "",
        "value_type": "string",
        "label": "ЄДРПОУ",
        "description": "Для фіскальних документів",
        "options": None,
    },
    # ── 2. Каса (POS) ────────────────────────────
    {
        "module": "pos",
        "key": "auto_print_receipt",
        "value": "true",
        "value_type": "boolean",
        "label": "Автоматичний друк чека",
        "description": "Друкувати чек одразу після завершення продажу",
        "options": None,
    },
    {
        "module": "pos",
        "key": "show_product_card_on_scan",
        "value": "false",
        "value_type": "boolean",
        "label": "Показувати картку товару при скануванні",
        "description": "Відкривати картку товару після сканування штрих-коду",
        "options": None,
    },
    {
        "module": "pos",
        "key": "allow_negative_stock",
        "value": "false",
        "value_type": "boolean",
        "label": "Торгівля в мінус",
        "description": "Дозволити продаж товарів при нульовому залишку",
        "options": None,
    },
    {
        "module": "pos",
        "key": "price_rounding",
        "value": "1",
        "value_type": "select",
        "label": "Заокруглення ціни до",
        "description": "До якого номіналу заокруглювати ціну в чеку",
        "options": '["1","10","50"]',
    },
    # ── 3. Друк (Printing) ────────────────────────────
    {
        "module": "printing",
        "key": "printer_name",
        "value": "",
        "value_type": "string",
        "label": "Принтер за замовчуванням",
        "description": "Назва принтера в системі (порожньо = системний за замовчуванням)",
        "options": None,
    },
    {
        "module": "printing",
        "key": "default_template_type",
        "value": "receipt_58mm",
        "value_type": "select",
        "label": "Тип чеку за замовчуванням",
        "description": "Який тип шаблону використовувати для друку чеків",
        "options": '["receipt_58mm","receipt_80mm","return_receipt_58mm","fiscal","custom","price_tag","label"]',
    },
    {
        "module": "printing",
        "key": "print_copies",
        "value": "1",
        "value_type": "number",
        "label": "Кількість копій",
        "description": "Скільки примірників друкувати за замовчуванням",
        "options": None,
    },
    {
        "module": "printing",
        "key": "auto_cut_paper",
        "value": "true",
        "value_type": "boolean",
        "label": "Автоматичне обрізання паперу",
        "description": "Обрізати папір після друку (якщо підтримується принтером)",
        "options": None,
    },
    {
        "module": "printing",
        "key": "show_logo",
        "value": "true",
        "value_type": "boolean",
        "label": "Показувати логотип",
        "description": "Виводити логотип магазину в чеку",
        "options": None,
    },
    {
        "module": "printing",
        "key": "return_receipt_template_type",
        "value": "return_receipt_58mm",
        "value_type": "select",
        "label": "Тип чеку повернення",
        "description": "Який тип шаблону використовувати для друку чеків повернення",
        "options": '["receipt_58mm","receipt_80mm","return_receipt_58mm","fiscal","custom"]',
    },
    {
        "module": "printing",
        "key": "receipt_print_copies",
        "value": "1",
        "value_type": "number",
        "label": "Копії для чеків",
        "description": "Скільки примірників друкувати для звичайних чеків",
        "options": None,
    },
    {
        "module": "printing",
        "key": "report_print_copies",
        "value": "1",
        "value_type": "number",
        "label": "Копії для звітів",
        "description": "Скільки примірників друкувати для звітів (X-звіт, Z-звіт)",
        "options": None,
    },
    # ── 4. Налаштування цінників та етикеток ──────────
    {
        "module": "printing",
        "key": "price_tag_fields",
        "value": '["name","price","barcode","created_date"]',
        "value_type": "string",
        "label": "Поля на ціннику",
        "description": "Які поля показувати на ціннику",
        "options": '["name","price","barcode","article","category","created_date"]',
    },
    {
        "module": "printing",
        "key": "price_tag_width",
        "value": "40",
        "value_type": "number",
        "label": "Ширина цінника (мм)",
        "description": "Ширина цінника у міліметрах",
        "options": None,
    },
    {
        "module": "printing",
        "key": "price_tag_height",
        "value": "25",
        "value_type": "number",
        "label": "Висота цінника (мм)",
        "description": "Висота цінника у міліметрах",
        "options": None,
    },
    {
        "module": "printing",
        "key": "label_fields",
        "value": '["name","price","barcode","created_date"]',
        "value_type": "string",
        "label": "Поля на етикетці",
        "description": "Які поля показувати на етикетці",
        "options": '["name","price","barcode","article","category","created_date"]',
    },
    {
        "module": "printing",
        "key": "label_width",
        "value": "60",
        "value_type": "number",
        "label": "Ширина етикетки (мм)",
        "description": "Ширина етикетки у міліметрах",
        "options": None,
    },
    {
        "module": "printing",
        "key": "label_height",
        "value": "40",
        "value_type": "number",
        "label": "Висота етикетки (мм)",
        "description": "Висота етикетки у міліметрах",
        "options": None,
    },
    # ── 5. Друк цінників та етикеток: додаткові параметри ──
    {
        "module": "printing",
        "key": "price_tag_gap",
        "value": "3",
        "value_type": "number",
        "label": "Відступ між цінниками (мм)",
        "description": "Відступ між цінниками у міліметрах",
        "options": None,
    },
    {
        "module": "printing",
        "key": "label_gap",
        "value": "3",
        "value_type": "number",
        "label": "Відступ між етикетками (мм)",
        "description": "Відступ між етикетками у міліметрах",
        "options": None,
    },
    {
        "module": "printing",
        "key": "price_tag_margin",
        "value": "10",
        "value_type": "number",
        "label": "Поле сторінки для цінників (мм)",
        "description": "Поле сторінки для цінників у міліметрах",
        "options": None,
    },
    {
        "module": "printing",
        "key": "barcode_type",
        "value": "code128",
        "value_type": "select",
        "label": "Тип штрих-коду",
        "description": "Тип штрих-коду за замовчуванням",
        "options": '["code128","ean13","ean8","upc_a","qr"]',
    },
    {
        "module": "printing",
        "key": "price_tag_template_id",
        "value": "",
        "value_type": "string",
        "label": "Шаблон цінника за замовчуванням",
        "description": "ID шаблону цінника (порожньо — використати is_default)",
        "options": None,
    },
    {
        "module": "printing",
        "key": "label_template_id",
        "value": "",
        "value_type": "string",
        "label": "Шаблон етикетки за замовчуванням",
        "description": "ID шаблону етикетки (порожньо — використати is_default)",
        "options": None,
    },
]


def seed_settings(database_url: str | None = None):
    """Заповнює таблицю system_settings початковими даними."""
    url = database_url or settings.DATABASE_URL_SYNC
    engine = create_engine(url)

    with Session(engine) as session:
        for setting in DEFAULT_SETTINGS:
            # Перевіряємо, чи вже існує
            existing = session.execute(
                text("SELECT id FROM system_settings WHERE key = :key"),
                {"key": setting["key"]},
            ).fetchone()

            if not existing:
                now = datetime.now(UTC)
                session.execute(
                    text("""
                        INSERT INTO system_settings (id, module, key, value, value_type, label, description, options, is_active, created_at, updated_at)
                        VALUES (:id, :module, :key, :value, :value_type, :label, :description, :options, true, :created_at, :updated_at)
                    """),
                    {
                        "id": str(uuid.uuid4()),
                        "module": setting["module"],
                        "key": setting["key"],
                        "value": setting["value"],
                        "value_type": setting["value_type"],
                        "label": setting["label"],
                        "description": setting["description"],
                        "options": setting["options"],
                        "created_at": now,
                        "updated_at": now,
                    },
                )
                print(f"  ✅ Додано: {setting['module']}.{setting['key']} = {setting['value']}")
            else:
                print(f"  ⏭️  Вже існує: {setting['module']}.{setting['key']}")

        session.commit()
        print(f"\n✅ Seed налаштувань завершено! Додано/оновлено {len(DEFAULT_SETTINGS)} налаштувань.")


if __name__ == "__main__":
    seed_settings()
