"""
Seed: Оновлення дефолтного шаблону receipt_58mm — новий стиль 2026-07-30.

Зміни:
  - font-size: 10px для body (було 12px) — вміщається більше
  - border-top: 2px dashed (було 1px) — товстіші лінії
  - margin: 6px 0 для пунктирів — більше відступів
  - padding: 1mm 1.5mm (було 2px 4px) — менші відступи
  - box-sizing: border-box — щоб padding входив в ширину
  - Courier New monospace (було Arial) — термопринтерний шрифт

Запускати: python seed_update_receipt_58mm_template.py
"""

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine

from app.infrastructure.persistence.models.print_template import PrintTemplate

DEFAULT_TEMPLATE_ID = "a0000000-0000-0000-0000-000000000001"

NEW_CONTENT = """<html>
<body style="font-family: 'Courier New', Courier, monospace; font-size: 10px; width: 48mm; margin: 0; padding: 0; color: #000; line-height: 1.2; box-sizing: border-box;">
    <div style="padding: 1mm 1.5mm;">

        <!-- Шапка -->
        <div style="text-align: center; margin-bottom: 4px;">
            <div style="font-size: 10px; font-weight: bold; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 1px;">{{shop_name}}</div>
            <div style="font-size: 8px; color: #333;">{{shop_address}}</div>
        </div>

        <div style="border-top: 2px dashed #000; margin: 6px 0;"></div>

        <!-- Інформація про чек -->
        <div style="text-align: center; font-size: 11px; font-weight: bold; margin: 3px 0;">
            ЧЕК № {{receipt_number}}
        </div>
        <div style="font-size: 9px; margin-bottom: 3px;">
            <table style="width: 100%; border-collapse: collapse; font-size: 9px;">
                <tr>
                    <td style="text-align: left;">{{date}}</td>
                    <td style="text-align: right;">{{time}}</td>
                </tr>
                <tr>
                    <td style="text-align: left;" colspan="2">Касир: {{cashier}}</td>
                </tr>
            </table>
        </div>

        <div style="border-top: 2px dashed #000; margin: 6px 0;"></div>

        <!-- Список товарів (CSS Grid — назва ліворуч, сума праворуч) -->
        <div style="font-size: 10px; width: 100%; margin: 3px 0;">
            {{items}}
        </div>

        <div style="border-top: 2px dashed #000; margin: 6px 0;"></div>

        <!-- Підсумок -->
        <table style="width: 100%; border-collapse: collapse; margin-top: 3px;">
            <tr>
                <td style="font-size: 12px; font-weight: bold; text-align: left;">ДО СПЛАТИ</td>
                <td style="font-size: 12px; font-weight: bold; text-align: right;">{{total}} грн</td>
            </tr>
        </table>

        <table style="width: 100%; border-collapse: collapse; font-size: 10px; margin-top: 3px;">
            <tr>
                <td style="text-align: left;">{{payment_method}}</td>
                <td style="text-align: right;">{{paid}} грн</td>
            </tr>
            <tr>
                <td style="text-align: left;">Решта</td>
                <td style="text-align: right;">{{change}} грн</td>
            </tr>
        </table>

        <div style="border-top: 2px dashed #000; margin: 8px 0 6px 0;"></div>

        <div style="text-align: center; font-size: 10px; font-weight: bold; margin-top: 3px;">
            Дякуємо за покупку!
        </div>

    </div>
</body>
</html>"""


async def main():
    # Підключаємось до БД
    db_url = os.getenv(
        "DATABASE_URL",
        "postgresql+asyncpg://postgres:VgxWd7MBJ10X@localhost:5432/pos_system"
    )
    engine = create_async_engine(db_url, echo=False)  # echo=False — менше шуму

    async with AsyncSession(engine) as session:
        # Отримуємо шаблон
        result = await session.execute(
            select(PrintTemplate).where(PrintTemplate.id == DEFAULT_TEMPLATE_ID)
        )
        template = result.scalar_one_or_none()

        if not template:
            print(f"❌ Шаблон з ID {DEFAULT_TEMPLATE_ID} не знайдено!")
            return

        # Зберігаємо значення ДО commit (після commit async сесія недоступна)
        name = template.name
        type_ = template.type

        # Оновлюємо content
        template.content = NEW_CONTENT
        await session.commit()
        print(f"✅ Шаблон '{name}' ({type_}) оновлено!")

    await engine.dispose()
    print("✅ Готово!")


if __name__ == "__main__":
    asyncio.run(main())
