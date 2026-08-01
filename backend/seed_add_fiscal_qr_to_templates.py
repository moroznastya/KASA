"""
Seed: Додати плейсхолдер {{fiscal_block}} у шаблони чеків (Фаза 3.8 — QR ДПС).

Що робить:
  - Для КОЖНОГО активного шаблону чеків (receipt_58mm, receipt_80mm,
    return_receipt_58mm, return_receipt_80mm, fiscal та інших типів, що
    починаються з "receipt"/"return_receipt") додає плейсхолдер {{fiscal_block}}
    у нижню частину чека (після футера "Дякуємо за покупку!" / "Повернення оформлено").
  - {{fiscal_block}} — це HTML-блок з фіскальними реквізитами (ФН, № фіскального
    чека, дата/час) та QR-кодом для перевірки чеку в ДПС. Блок формується на
    фронтенді (frontend/src/hooks/useReceiptPrinter.ts → receiptToRenderData)
    і підставляється серверним рендером PrintTemplateService.render_template.
  - Для НЕфіскальних чеків значення fiscal_block порожнє → шаблон друкується
    БЕЗ QR (звичайний друк не змінюється).
  - Ідемпотентний: якщо плейсхолдер уже є — шаблон не чіпається.

Запускати: python seed_add_fiscal_qr_to_templates.py
"""

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine

from app.infrastructure.persistence.models.print_template import PrintTemplate

# Типи шаблонів, у які додаємо фіскальний блок з QR
RECEIPT_TYPE_PREFIXES = ("receipt", "return_receipt", "fiscal")

# Тексти футерів, ПІСЛЯ яких вставляємо фіскальний блок
FOOTER_MARKERS = ("Дякуємо за покупку!", "Повернення оформлено")


def add_fiscal_block_placeholder(content: str) -> str | None:
    """
    Повертає оновлений контент шаблону з плейсхолдером {{fiscal_block}},
    або None, якщо плейсхолдер уже присутній (ідемпотентність).

    Позиція: ПІСЛЯ футера ("Дякуємо за покупку!" / "Повернення оформлено") —
    QR-код ДПС має бути в нижній частині чека. Якщо футер не знайдено —
    вставка перед </body>.
    """
    if "{{fiscal_block}}" in content:
        return None

    # 1) Вставка ПІСЛЯ div-контейнера футера (блок стає сусідом футера,
    #    а не його частиною — не наслідує font-weight/font-size футера)
    for marker in FOOTER_MARKERS:
        idx = content.find(marker)
        if idx != -1:
            end = idx + len(marker)
            close = content.find("</div>", end)
            if close != -1:
                after_close = close + len("</div>")
                return (
                    content[:after_close]
                    + "\n        {{fiscal_block}}\n"
                    + content[after_close:]
                )
            return content[:end] + "\n        {{fiscal_block}}\n" + content[end:]

    # 2) Fallback: перед закриттям body
    body_close = content.rfind("</body>")
    if body_close != -1:
        return content[:body_close] + "\n        {{fiscal_block}}\n" + content[body_close:]

    return content + "\n        {{fiscal_block}}\n"


async def main():
    db_url = os.getenv(
        "DATABASE_URL",
        "postgresql+asyncpg://postgres:VgxWd7MBJ10X@localhost:5432/pos_system"
    )
    engine = create_async_engine(db_url, echo=False)

    async with AsyncSession(engine) as session:
        # Всі активні шаблони чеків (типи: receipt*, return_receipt*, fiscal)
        result = await session.execute(
            select(PrintTemplate)
            .where(PrintTemplate.is_active == True)
            .order_by(PrintTemplate.type, PrintTemplate.name)
        )
        templates = result.scalars().all()

        updated = 0
        skipped = 0
        for template in templates:
            if not template.type.startswith(RECEIPT_TYPE_PREFIXES):
                continue

            new_content = add_fiscal_block_placeholder(template.content or "")
            if new_content is None:
                skipped += 1
                print(
                    f"⏭️  '{template.name}' ({template.type}) — плейсхолдер уже є, пропущено"
                )
                continue

            template.content = new_content
            updated += 1
            print(f"✅ '{template.name}' ({template.type}) — додано {{{{fiscal_block}}}}")

        await session.commit()
        print(f"\n📊 Підсумок: оновлено {updated}, пропущено {skipped}")

    await engine.dispose()
    print("✅ Готово!")


if __name__ == "__main__":
    asyncio.run(main())
