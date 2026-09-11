"""
Фінальний етап категоризації - ручне призначення для 14 товарів.
"""
import asyncio

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

from app.config import settings


async def categorize():
    engine = create_async_engine(settings.DATABASE_URL, echo=False)
    session_factory = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)
    async with session_factory() as session:
        result = await session.execute(text("SELECT id, name FROM categories"))
        cat_map = {r.name: r.id for r in result.all()}

        # Ручне призначення для останніх 14 товарів
        # Використовуємо LIKE для пошуку
        manual_mapping = [
            ("2003500101282", "barcode", "Інші товари"),
            ("21110869", "barcode", "Інші товари"),
            ("21348753", "barcode", "Інші товари"),
            ("4823077604492", "barcode", "Інші товари"),
            ("4823088608656", "barcode", "Інші товари"),
            ("Кисла пятка%", "title_like", "Цукерки"),
            ("ліпучка на мухи%", "title_like", "Засоби захисту від комарів, сонця"),
            ("Мікс мягка уп%", "title_like", "Хліб"),
            ("Невидимки для волосся%", "title_like", "Аксесуари для гігієни"),
            ("Око монстра%", "title_like", "Цукерки"),
            ("Печ. цукр.Хелло діно%", "title_like", "Печиво, вафлі, бісквіт"),
            ("Салфетка волога мала%", "title_like", "Паперові вироби"),
            ("салфетки вологі суперфреш%", "title_like", "Паперові вироби"),
            ("Ядро СанСанич 50 г%", "title_like", "Насіння соняшникове, гарбузове"),
        ]

        updates = 0
        for search_key, search_type, cat_name in manual_mapping:
            cat_id = cat_map.get(cat_name)
            if not cat_id:
                print(f"❌ Категорію '{cat_name}' не знайдено!")
                continue

            if search_type == "barcode":
                result = await session.execute(
                    text("SELECT id, title FROM products WHERE barcode = :barcode"),
                    {"barcode": search_key}
                )
            elif search_type == "title_like":
                result = await session.execute(
                    text("SELECT id, title FROM products WHERE title LIKE :title"),
                    {"title": search_key}
                )

            products = result.all()
            if products:
                for prod in products:
                    await session.execute(
                        text("UPDATE products SET category_id = :cat_id WHERE id = :prod_id"),
                        {"cat_id": cat_id, "prod_id": prod.id}
                    )
                    updates += 1
                    print(f"  ✅ {prod.title} → {cat_name}")
            else:
                print(f"  ❌ Товар '{search_key}' не знайдено")

        await session.commit()
        print(f"\n✅ Оновлено {updates} товарів")

        # Фінальна перевірка
        result = await session.execute(text("""
            SELECT count(*) FROM products p
            JOIN categories c ON c.id = p.category_id
            WHERE c.name = 'Без категорії'
        """))
        remaining = result.scalar()
        print(f"\n📊 Залишилось товарів у 'Без категорії': {remaining}")

        # Загальна статистика
        result = await session.execute(text("""
            SELECT c.name, count(p.id) as cnt
            FROM categories c
            LEFT JOIN products p ON p.category_id = c.id
            GROUP BY c.name
            ORDER BY cnt DESC
        """))
        print("\n📊 Розподіл товарів по категоріях:")
        for r in result.all():
            print(f"  {r.name}: {r.cnt}")

    await engine.dispose()

asyncio.run(categorize())
