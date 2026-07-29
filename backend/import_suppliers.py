"""
Імпорт постачальників з CSV файлу `kasa/Categories/Постачальники.csv`
в таблицю `suppliers`.

Запуск:
    cd kasa/backend && python import_suppliers.py
"""

import asyncio
import csv
from pathlib import Path

from sqlalchemy import text
from sqlalchemy.ext.asyncio import (
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)

from app.config import settings

# ──────────────────────────────────────────────
# Конфігурація
# ──────────────────────────────────────────────
CSV_PATH = (
    Path(__file__).resolve().parent.parent
    / "Categories"
    / "Постачальники.csv"
)


def build_notes(row: dict) -> str | None:
    """
    Формує поле `notes` з колонок:
      - Реквізити
      - Відстрочка платежу
      - Макс. сума боргу
    """
    parts = []

    rekviz = row.get("Реквізити", "").strip()
    if rekviz:
        parts.append(f"Реквізити: {rekviz}")

    vidstrochka = row.get("Відстрочка платежу", "").strip()
    if vidstrochka and vidstrochka not in ("0", "", "0.00"):
        parts.append(f"Відстрочка платежу: {vidstrochka}")

    max_borg = row.get("Макс. сума боргу", "").strip()
    if max_borg and max_borg not in ("0", "", "0.00"):
        parts.append(f"Макс. сума боргу: {max_borg}")

    return "; ".join(parts) if parts else None


async def import_suppliers():
    """
    Головна функція:
    1. Читає CSV
    2. Для кожного рядка перевіряє дублікат за `name`
    3. Додає нових постачальників
    4. Фіксує транзакцію
    """
    engine = create_async_engine(settings.DATABASE_URL)
    async_session = async_sessionmaker(
        engine, class_=AsyncSession, expire_on_commit=False
    )

    async with async_session() as session:
        try:
            # ── Завантажуємо існуючі назви постачальників ──
            result = await session.execute(
                text("SELECT name FROM suppliers")
            )
            existing_names = {row.name for row in result.fetchall()}
            print(
                f"📂 Існуючих постачальників в БД: {len(existing_names)}"
            )

            # ── Читаємо CSV ──
            with open(CSV_PATH, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)
                rows = list(reader)

            print(f"📄 Знайдено рядків у CSV: {len(rows)}")

            added = 0
            skipped_names = []
            skipped_empty = 0

            for row in rows:
                name = row.get("Найменування", "").strip()

                # Пропускаємо пусті назви
                if not name:
                    skipped_empty += 1
                    continue

                # Перевірка на дублікат
                if name in existing_names:
                    skipped_names.append(name)
                    continue

                # Формуємо notes
                notes = build_notes(row)

                # Вставка нового постачальника
                await session.execute(
                    text(
                        """
                        INSERT INTO suppliers (name, notes)
                        VALUES (:name, :notes)
                        """
                    ),
                    {"name": name, "notes": notes},
                )
                added += 1
                existing_names.add(name)  # для перевірки наступних дублікатів

            # ── Фіксуємо транзакцію ──
            await session.commit()

            # ── Виводимо статистику ──
            print("\n" + "=" * 60)
            print("📊 СТАТИСТИКА ІМПОРТУ")
            print("=" * 60)
            print(f"   ✅ Додано нових постачальників:   {added}")
            print(f"   ⏭️  Пропущено (дублікати):         {len(skipped_names)}")
            print(f"   ⏭️  Пропущено (пуста назва):       {skipped_empty}")

            if skipped_names:
                print("\n⏭️  Пропущені дублікати:")
                for name in skipped_names:
                    print(f"   - {name}")

            # Перевірка після імпорту
            result = await session.execute(
                text("SELECT COUNT(*) FROM suppliers")
            )
            total = result.scalar()
            print(f"\n📈 Всього постачальників в БД: {total}")

        except Exception as e:
            await session.rollback()
            print(f"\n❌ ПОМИЛКА: {e}")
            raise
        finally:
            await engine.dispose()


if __name__ == "__main__":
    asyncio.run(import_suppliers())
