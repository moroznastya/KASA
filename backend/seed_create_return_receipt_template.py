"""
Seed: Створення шаблону друку return_receipt_58mm для чеків повернення.

ID шаблону: a0000000-0000-0000-0000-000000000002
Логіка:
  - Шаблон з ID a0000000-0000-0000-0000-000000000002 існує → оновлюємо content та name
  - Шаблон не існує → створюємо новий

Запускати: python seed_create_return_receipt_template.py
"""

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine

from app.infrastructure.persistence.models.print_template import PrintTemplate

RETURN_TEMPLATE_ID = "a0000000-0000-0000-0000-000000000002"

RETURN_CONTENT = """<html>
<body style="font-family: 'Courier New', Courier, monospace; font-size: 10px; width: 48mm; margin: 0; padding: 0; color: #000; line-height: 1.2; box-sizing: border-box;">
    <div style="padding: 1mm 1.5mm;">

        <div style="text-align: center; font-size: 14px; font-weight: bold; margin-bottom: 6px; letter-spacing: 1px;">ПОВЕРНЕННЯ</div>

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
                <td style="font-size: 12px; font-weight: bold; text-align: left;">ВИДАЧА КОШТІВ</td>
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

    </div>
</body>
</html>"""


async def main():
    # Підключаємось до БД
    db_url = os.getenv(
        "DATABASE_URL",
        "postgresql+asyncpg://postgres:VgxWd7MBJ10X@localhost:5432/pos_system"
    )
    engine = create_async_engine(db_url, echo=False)

    async with AsyncSession(engine) as session:
        # Шукаємо шаблон за ID
        result = await session.execute(
            select(PrintTemplate).where(PrintTemplate.id == RETURN_TEMPLATE_ID)
        )
        template = result.scalar_one_or_none()

        if template:
            # Оновлюємо існуючий шаблон
            old_name = template.name
            template.name = "Повернення 58мм"
            template.content = RETURN_CONTENT
            template.is_default = True
            template.is_active = True
            await session.commit()
            print(f"✅ Шаблон '{old_name}' оновлено на 'Повернення 58мм' (return_receipt_58mm)!")
        else:
            # Створюємо новий шаблон
            from uuid import UUID
            new_template = PrintTemplate(
                id=UUID(RETURN_TEMPLATE_ID),
                name="Повернення 58мм",
                type="return_receipt_58mm",
                content=RETURN_CONTENT,
                is_default=True,
                is_active=True,
            )
            session.add(new_template)
            await session.commit()
            print("✅ Шаблон 'Повернення 58мм' (return_receipt_58mm) створено!")

    await engine.dispose()
    print("✅ Готово!")


if __name__ == "__main__":
    asyncio.run(main())
