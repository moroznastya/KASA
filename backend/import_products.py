"""
Скрипт для очищення та імпорту товарів з CSV-файлів.

Використання:
    python import_products.py

Що робить:
    1. Очищає всі пов'язані таблиці (barcodes, product_images, invoice_items,
       receipt_items, transfer_items, write_off_items, return_invoice_items, products)
    2. Імпортує товари з 8 CSV-файлів з папки /home/anastasia/Andriy/Bot/aegis_v3/товари/
    3. Виводить статистику
"""

import asyncio
import csv
import os
from decimal import Decimal, InvalidOperation
from uuid import uuid4

from sqlalchemy import text
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession, async_sessionmaker

from app.config import settings
from app.models.product import Product


# Шлях до папки з CSV-файлами
CSV_DIR = "/home/anastasia/Andriy/Bot/aegis_v3/товари/"

# Маппінг назв колонок CSV → поля Product
COLUMN_MAPPING = {
    "Код": "barcode",
    "Найменування": "title",
    "Собіварт.": "cost_price",
    "Націнка": "markup",
    "Ціна": "price",
    "К-сть": "stock",
}

# Поля за замовчуванням
DEFAULT_FIELDS = {
    "unit": "шт",
    "is_weight": False,
    "tax_rate": Decimal("0.00"),
    "tax_group": "А",
    "scan_excise": False,
    "category_id": None,
    "supplier_id": None,
    "description": None,
    "uktzed": None,
    "sku": None,
}


def parse_decimal(value: str) -> Decimal | None:
    """Безпечний парсинг Decimal з CSV (коми, пробіли, порожні рядки)."""
    if value is None:
        return None
    value = value.strip().replace(" ", "").replace(",", ".")
    if not value:
        return None
    try:
        return Decimal(value)
    except (InvalidOperation, ValueError):
        return None


def parse_int(value: str) -> int | None:
    """Безпечний парсинг int з CSV."""
    if value is None:
        return None
    value = value.strip().replace(" ", "")
    if not value:
        return None
    try:
        return int(value)
    except (ValueError, InvalidOperation):
        return None


def read_csv_file(filepath: str) -> list[dict]:
    """Читає CSV-файл і повертає список словників."""
    rows = []
    with open(filepath, mode="r", encoding="utf-8-sig") as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append(row)
    return rows


def map_row_to_product(row: dict) -> dict:
    """Маппінг рядка CSV в словник полів Product."""
    product_data = {}

    # Основний маппінг
    for csv_col, product_field in COLUMN_MAPPING.items():
        raw_value = row.get(csv_col, "").strip()

        if product_field in ("cost_price", "markup", "price", "stock"):
            # Числові поля
            parsed = parse_decimal(raw_value)
            if parsed is not None:
                product_data[product_field] = parsed
            else:
                product_data[product_field] = Decimal("0.00")
        elif product_field == "barcode":
            # Штрих-код — очищаємо від зайвих символів
            barcode = raw_value.strip().replace(" ", "")
            product_data["barcode"] = barcode if barcode else None
        elif product_field == "title":
            product_data["title"] = raw_value if raw_value else "Без назви"
        else:
            product_data[product_field] = raw_value if raw_value else None

    # Додаємо поля за замовчуванням
    for field, value in DEFAULT_FIELDS.items():
        if field not in product_data or product_data[field] is None:
            product_data[field] = value

    # Генеруємо ID
    product_data["id"] = uuid4()

    return product_data


async def clear_tables(session: AsyncSession):
    """Очищає всі пов'язані таблиці в правильному порядку."""
    print("🧹 Очищення таблиць...")

    tables_to_clear = [
        "return_invoice_items",
        "write_off_items",
        "transfer_items",
        "receipt_items",
        "invoice_items",
        "product_images",
        "barcodes",
        "products",
    ]

    for table in tables_to_clear:
        await session.execute(text(f"DELETE FROM {table}"))
        print(f"   ✅ {table} — очищено")

    await session.flush()
    print("✅ Всі таблиці очищено")


async def import_products(session: AsyncSession) -> int:
    """Імпортує товари з усіх CSV-файлів. Повертає кількість імпортованих."""
    # Збираємо всі CSV-файли
    csv_files = sorted(
        [f for f in os.listdir(CSV_DIR) if f.endswith(".csv") and f.startswith("Список товарів")]
    )
    print(f"\n📂 Знайдено файлів: {len(csv_files)}")
    for f in csv_files:
        print(f"   - {f}")

    # Множина для відстеження унікальних штрих-кодів
    seen_barcodes: set[str] = set()
    total_imported = 0
    total_skipped = 0
    total_errors = 0

    for filename in csv_files:
        filepath = os.path.join(CSV_DIR, filename)
        print(f"\n📄 Читання: {filename}...")

        try:
            rows = read_csv_file(filepath)
        except Exception as e:
            print(f"   ❌ Помилка читання файлу: {e}")
            total_errors += 1
            continue

        file_imported = 0
        file_skipped = 0
        file_errors = 0

        for row_num, row in enumerate(rows, start=2):  # 2 = перший рядок даних
            try:
                product_data = map_row_to_product(row)

                # Перевірка обов'язкових полів
                if not product_data.get("title"):
                    file_skipped += 1
                    continue

                barcode = product_data.get("barcode")

                # Перевірка дублікатів за штрих-кодом
                if barcode:
                    if barcode in seen_barcodes:
                        file_skipped += 1
                        continue
                    seen_barcodes.add(barcode)

                # Створюємо об'єкт Product
                product = Product(**product_data)
                session.add(product)
                file_imported += 1

            except Exception as e:
                file_errors += 1
                if file_errors <= 3:  # Показуємо перші 3 помилки
                    print(f"   ⚠️  Рядок {row_num}: {e}")

        total_imported += file_imported
        total_skipped += file_skipped
        total_errors += file_errors

        print(f"   ✅ Імпортовано: {file_imported}, пропущено: {file_skipped}, помилок: {file_errors}")

    return total_imported, total_skipped, total_errors


async def main():
    """Головна функція."""
    print("=" * 60)
    print("🚀 ІМПОРТ ТОВАРІВ З CSV")
    print("=" * 60)

    engine = create_async_engine(settings.DATABASE_URL)
    async_session = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)

    async with async_session() as session:
        try:
            # 1. Очищення
            await clear_tables(session)

            # 2. Імпорт
            imported, skipped, errors = await import_products(session)

            # 3. COMMIT
            await session.commit()

            # 4. Статистика
            print("\n" + "=" * 60)
            print("📊 СТАТИСТИКА ІМПОРТУ")
            print("=" * 60)
            print(f"   ✅ Імпортовано товарів: {imported}")
            print(f"   ⏭️  Пропущено (дублікати/порожні): {skipped}")
            print(f"   ❌ Помилок: {errors}")
            print(f"   📦 Всього унікальних штрих-кодів: {imported}")
            print("=" * 60)

        except Exception as e:
            await session.rollback()
            print(f"\n❌ КРИТИЧНА ПОМИЛКА: {e}")
            print("🔄 Виконано ROLLBACK")
            raise

    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(main())
