"""
Seed: Додати дефолтний шаблон для чеку повернення (return_receipt_58mm).

Створює шаблон з типом return_receipt_58mm, якщо його ще не існує.
Візуальні відмінності від звичайного чеку:
  - Великий напис "ПОВЕРНЕННЯ"
  - Номер оригінального чеку (якщо є)
  - Причина повернення (якщо є)
  - Сума зі знаком мінус / червоним кольором
  - Футер: "Повернення оформлено"
"""

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine, async_sessionmaker
from app.infrastructure.persistence.models.print_template import PrintTemplate

RETURN_TEMPLATE_ID = "a0000000-0000-0000-0000-000000000050"

RETURN_TEMPLATE_CONTENT = """<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
  @page { margin: 0; size: 58mm auto; }
  body {
    font-family: 'Courier New', monospace;
    font-size: 12px;
    width: 58mm;
    padding: 2mm;
    margin: 0;
    text-align: center;
  }
  .header { font-size: 18px; font-weight: bold; color: #dc2626; margin-bottom: 4px; }
  .receipt-number { font-size: 14px; font-weight: bold; margin-bottom: 8px; }
  .shop-name { font-size: 14px; font-weight: bold; margin-bottom: 2px; }
  .separator { border-top: 1px dashed #000; margin: 6px 0; }
  .item { text-align: left; margin-bottom: 2px; font-size: 11px; }
  .item-name { font-weight: bold; }
  .item-detail { color: #333; }
  .total { font-size: 16px; font-weight: bold; color: #dc2626; margin: 8px 0; }
  .footer { font-size: 11px; color: #666; margin-top: 6px; }
  .original-receipt { font-size: 10px; color: #555; margin: 4px 0; }
  .return-reason { font-size: 10px; color: #dc2626; margin: 4px 0; }
</style>
</head>
<body>
  <div class="shop-name">{{shop_name}}</div>
  <div>{{shop_address}}</div>
  <div>ЄДРПОУ: {{tax_id}}</div>
  <div class="separator"></div>
  <div class="header">ПОВЕРНЕННЯ</div>
  <div class="receipt-number">№ {{receipt_number}}</div>
  {% if original_receipt_number %}
  <div class="original-receipt">Оригінальний чек: № {{original_receipt_number}}</div>
  {% endif %}
  {% if return_reason %}
  <div class="return-reason">Причина: {{return_reason}}</div>
  {% endif %}
  <div>{{date}} {{time}}</div>
  <div>Касир: {{cashier}}</div>
  <div class="separator"></div>
  {{items}}
  <div class="separator"></div>
  <div class="total">СУМА ПОВЕРНЕННЯ: -{{total}} грн</div>
  <div class="separator"></div>
  <div class="footer">Повернення оформлено</div>
</body>
</html>"""


async def main():
    db_url = os.getenv(
        "DATABASE_URL",
        "postgresql+asyncpg://postgres:VgxWd7MBJ10X@localhost:5432/pos_system"
    )
    engine = create_async_engine(db_url, echo=False)

    async with AsyncSession(engine) as session:
        # Перевіряємо, чи вже існує шаблон повернення
        result = await session.execute(
            select(PrintTemplate).where(PrintTemplate.type == "return_receipt_58mm")
        )
        existing = result.scalar_one_or_none()

        if existing:
            print(f"⏭️  Шаблон повернення вже існує: '{existing.name}' (ID: {existing.id})")
            return

        # Створюємо новий шаблон
        template = PrintTemplate(
            id=RETURN_TEMPLATE_ID,
            name="Чек повернення 58мм",
            type="return_receipt_58mm",
            content=RETURN_TEMPLATE_CONTENT,
            is_default=True,
            is_active=True,
        )
        session.add(template)
        await session.commit()
        print(f"✅ Створено шаблон повернення: '{template.name}' ({template.type})")

    await engine.dispose()
    print("✅ Готово!")


if __name__ == "__main__":
    asyncio.run(main())
