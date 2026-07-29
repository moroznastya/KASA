"""
Імпорт товарів з CSV-файлів у базу даних pos_system.
Запуск: python import_products.py
"""

import csv
import uuid
import asyncio
from datetime import datetime
from pathlib import Path
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession
from sqlalchemy.orm import sessionmaker
from sqlalchemy import text

# Конфігурація БД
DB_URL = "postgresql+asyncpg://postgres:VgxWd7MBJ10X@localhost:5434/pos_system"

CSV_FILES = [
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів.csv",
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів (1).csv",
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів (2).csv",
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів (3).csv",
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів (4).csv",
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів (5).csv",
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів (6).csv",
    "/home/anastasia/Andriy/Bot/aegis_v3/товари/Список товарів (7).csv",
]


def parse_price(val: str) -> float:
    """Парсить ціну: '1,300.00' -> 1300.00"""
    val = val.strip().replace('"', '')
    # Якщо кома - розділювач тисяч
    if ',' in val and val.count(',') == 1 and val.endswith('.00'):
        val = val.replace(',', '')
    return float(val)


async def import_products():
    engine = create_async_engine(DB_URL)
    async_session = sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)

    total = 0
    errors = 0

    async with async_session() as session:
        for csv_path in CSV_FILES:
            path = Path(csv_path)
            if not path.exists():
                print(f"❌ Файл не знайдено: {csv_path}")
                continue

            print(f"📂 Обробка: {path.name}")
            with open(path, "r", encoding="utf-8-sig") as f:
                reader = csv.DictReader(f)
                for row in reader:
                    try:
                        barcode = row.get("Код", "").strip()
                        title = row.get("Найменування", "").strip()
                        cost_price = parse_price(row.get("Собіварт.", "0"))
                        price = parse_price(row.get("Ціна", "0"))
                        markup_str = row.get("Націнка", "0").strip()
                        markup = float(markup_str) if markup_str else 0
                        qty_str = row.get("К-сть", "0").strip()
                        qty = float(qty_str) if qty_str else 0

                        if not title:
                            continue

                        now = datetime.utcnow()
                        product_id = uuid.uuid4()

                        await session.execute(
                            text("""
                                INSERT INTO products (id, barcode, sku, title, description, 
                                    price, cost_price, markup, stock, unit,
                                    is_weight, scan_excise, tax_rate, tax_group,
                                    category_id, supplier_id, created_at, updated_at)
                                VALUES (:id, :barcode, :sku, :title, :description,
                                    :price, :cost_price, :markup, :stock, :unit,
                                    :is_weight, :scan_excise, :tax_rate, :tax_group,
                                    :category_id, :supplier_id, :created_at, :updated_at)
                                ON CONFLICT (barcode) DO UPDATE SET
                                    title = EXCLUDED.title,
                                    price = EXCLUDED.price,
                                    cost_price = EXCLUDED.cost_price,
                                    markup = EXCLUDED.markup,
                                    stock = products.stock + EXCLUDED.stock,
                                    updated_at = EXCLUDED.updated_at
                            """),
                            {
                                "id": product_id,
                                "barcode": barcode if barcode else None,
                                "sku": barcode if barcode else str(uuid.uuid4())[:8],
                                "title": title,
                                "description": "",
                                "price": price,
                                "cost_price": cost_price,
                                "markup": markup,
                                "stock": qty,
                                "unit": "шт",
                                "is_weight": False,
                                "scan_excise": False,
                                "tax_rate": 20.0,
                                "tax_group": "А",
                                "category_id": None,
                                "supplier_id": None,
                                "created_at": now,
                                "updated_at": now,
                            }
                        )
                        total += 1
                    except Exception as e:
                        errors += 1
                        print(f"  ⚠️ Помилка: {row.get('Найменування', '?')} - {e}")

        await session.commit()
        print(f"\n✅ Імпорт завершено!")
        print(f"   Додано/оновлено: {total}")
        print(f"   Помилок: {errors}")

    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(import_products())
