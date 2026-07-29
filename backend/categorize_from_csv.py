"""
Скрипт оновлення категорій товарів з CSV файлів.

Прочитає всі CSV файли з `kasa/Categories/`, знайде відповідні категорії в БД
та оновить `products.category_id` для кожного товару.

Запуск:
    cd kasa/backend && python categorize_from_csv.py
"""

import asyncio
import csv
import sys
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
CATEGORIES_DIR = Path(__file__).resolve().parent.parent / "Categories"

# Мапінг назв CSV файлів, що не збігаються з назвами категорій в БД
NAME_MAPPING = {
    "Інша мясна продукція": "Інша м'ясна продукція",  # апостроф
    "Морепродукти": "Інша риба",
}

# Файли, які потрібно пропустити (не категорії)
SKIP_FILES = {"Постачальники"}


async def categorize_from_csv():
    """
    Головна функція:
    1. Читає всі CSV файли з CATEGORIES_DIR
    2. Для кожного знаходить категорію в БД
    3. Для кожного рядка знаходить товар і оновлює category_id
    """
    engine = create_async_engine(settings.DATABASE_URL)
    async_session = async_sessionmaker(
        engine, class_=AsyncSession, expire_on_commit=False
    )

    async with async_session() as session:
        try:
            # ── Завантажуємо всі категорії з БД у словник ──
            result = await session.execute(
                text("SELECT id, name FROM categories")
            )
            db_categories = {row.name: row.id for row in result.fetchall()}
            print(f"📂 Завантажено категорій з БД: {len(db_categories)}")

            # ── Отримуємо список CSV файлів ──
            csv_files = sorted(CATEGORIES_DIR.glob("*.csv"))
            print(f"📄 Знайдено CSV файлів: {len(csv_files)}")

            stats = {
                "files_processed": 0,
                "files_skipped": 0,
                "categories_not_found": [],
                "products_updated": 0,
                "products_not_found": [],
                "total_rows": 0,
            }

            # ── Проходимо по кожному CSV файлу ──
            for csv_path in csv_files:
                category_name_csv = csv_path.stem  # назва файлу без .csv

                # Пропускаємо файли з SKIP_FILES
                if category_name_csv in SKIP_FILES:
                    print(f"⏭️  Пропущено (не категорія): {category_name_csv}")
                    stats["files_skipped"] += 1
                    continue

                # Маппінг назви, якщо потрібно
                category_name_db = NAME_MAPPING.get(
                    category_name_csv, category_name_csv
                )

                # Перевіряємо, чи існує категорія в БД
                category_id = db_categories.get(category_name_db)
                if category_id is None:
                    print(
                        f"⚠️  Категорію '{category_name_csv}' "
                        f"(-> '{category_name_db}') не знайдено в БД. Пропущено."
                    )
                    stats["categories_not_found"].append(category_name_csv)
                    continue

                # ── Читаємо CSV файл ──
                with open(csv_path, "r", encoding="utf-8") as f:
                    reader = csv.DictReader(f)
                    rows = list(reader)

                stats["files_processed"] += 1
                file_updated = 0
                file_not_found = 0

                for row in rows:
                    barcode = row.get("Код", "").strip()
                    product_name = row.get("Найменування", "").strip()

                    if not barcode:
                        continue

                    # Шукаємо товар за barcode
                    result = await session.execute(
                        text(
                            "SELECT id, title FROM products WHERE barcode = :barcode"
                        ),
                        {"barcode": barcode},
                    )
                    product = result.fetchone()

                    if product is None:
                        stats["products_not_found"].append(
                            f"  barcode={barcode}, name={product_name}, "
                            f"csv={category_name_csv}"
                        )
                        file_not_found += 1
                        continue

                    # Оновлюємо category_id
                    await session.execute(
                        text(
                            "UPDATE products SET category_id = :cat_id "
                            "WHERE id = :pid"
                        ),
                        {"cat_id": category_id, "pid": product.id},
                    )
                    file_updated += 1

                stats["products_updated"] += file_updated
                stats["total_rows"] += len(rows)

                print(
                    f"✅ {category_name_csv}: "
                    f"оновлено {file_updated}, "
                    f"не знайдено {file_not_found} "
                    f"(з {len(rows)} рядків)"
                )

            # ── Фіксуємо транзакцію ──
            await session.commit()

            # ── Виводимо статистику ──
            print("\n" + "=" * 60)
            print("📊 СТАТИСТИКА")
            print("=" * 60)
            print(f"   Оброблено файлів:          {stats['files_processed']}")
            print(f"   Пропущено файлів:          {stats['files_skipped']}")
            print(f"   Всього рядків у CSV:       {stats['total_rows']}")
            print(f"   ✅ Оновлено товарів:        {stats['products_updated']}")
            print(
                f"   ❌ Не знайдено товарів:     "
                f"{len(stats['products_not_found'])}"
            )
            print(
                f"   ⚠️  Категорій не знайдено:   "
                f"{len(stats['categories_not_found'])}"
            )

            if stats["categories_not_found"]:
                print("\n⚠️  Категорії, яких немає в БД:")
                for name in stats["categories_not_found"]:
                    print(f"   - {name}")

            if stats["products_not_found"]:
                print(
                    f"\n❌ Товари, не знайдені за barcode "
                    f"(перші 20):"
                )
                for item in stats["products_not_found"][:20]:
                    print(f"   {item}")
                if len(stats["products_not_found"]) > 20:
                    print(
                        f"   ... та ще "
                        f"{len(stats['products_not_found']) - 20}"
                    )

            # ── Перевірка після виконання ──
            print("\n" + "=" * 60)
            print("🔍 ПЕРЕВІРКА")
            print("=" * 60)
            result = await session.execute(
                text(
                    "SELECT COUNT(*) FROM products "
                    "WHERE category_id IS NOT NULL"
                )
            )
            with_category = result.scalar()
            result = await session.execute(
                text("SELECT COUNT(*) FROM products")
            )
            total_products = result.scalar()
            print(
                f"   Товарів з категорією: {with_category} "
                f"з {total_products}"
            )
            print(
                f"   Без категорії: "
                f"{total_products - with_category}"
            )

        except Exception as e:
            await session.rollback()
            print(f"\n❌ ПОМИЛКА: {e}")
            raise
        finally:
            await engine.dispose()


if __name__ == "__main__":
    asyncio.run(categorize_from_csv())
