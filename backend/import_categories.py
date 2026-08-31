"""
Скрипт імпорту категорій товарів з ієрархічною структурою.

Використання:
    cd kasa/backend
    . venv/bin/activate
    python import_categories.py
"""

import asyncio
import uuid

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

from app.config import settings
from app.infrastructure.persistence.models.category import Category

# ─── Ієрархія категорій ──────────────────────────────────────────────────────
# Формат: (назва_категорії, [підкатегорії])

CATEGORY_TREE = [
    ("Інші товари", [
        "Інше",
        "Без категорії",
        "Всі товари",
    ]),
    ("Алкоголь", [
        "Віскі",
        "Вина",
        "Горілка",
        "Коньяк",
        "Настоянки і тд",
        "Пиво",
        "Слабоалкогольні напої (сидр, енергетики)",
        "Тара",
        "Шампанське",
    ]),
    ("Бакалія і консерви", [
        "Інша бакалія",
        "Борошно",
        "Дріжджі",
        "Консерви",
        "Крупи",
        "Макаронні вироби",
        "Макові начинки",
        "Олія та оцет",
    ]),
    ("Випічка, грінки", [
        "Випічка",
        "Грінки, тарталетки",
        "Лаваш",
    ]),
    ("Все для свята", [
        "Листівки",
        "Подарункові пакети",
        "Феєрверки, шаріки",
    ]),
    ("Для дому", [
        "Догляд за взуттям",
        "Електрика",
        "Канцелярія",
        "Клеї",
        "Посуд",
        "Рукоділля",
        "Текстиль",
        "Шкарпетки",
    ]),
    ("Заморожена продукція", [
        "Заморожені овочі, крабові палички",
        "Масло",
        "Морозиво і десерти",
        "Напівфабрикати",
    ]),
    ("Засоби гігієни", [
        "Аксесуари для гігієни",
        "Вата, бинт, диски",
        "Засоби для гоління",
        "Зубні пасти",
        "Косметика та дезодоранти",
        "Подарункові набори",
        "Прокладки",
        "Фарба для волосся",
    ]),
    ("Зоотовари", [
        "Корм для тварин",
    ]),
    ("Кава, чай", [
        "Кава",
        "Кава з апарату",
        "Чай",
    ]),
    ("Ковбаси і м'ясні делікатеси", [
        "Інша м'ясна продукція",
        "Ковбаси",
        "Сардельки, сосиски",
    ]),
    ("Лампадки, свічки", [
        "Лампадки",
        "Свічки",
    ]),
    ("Молочні продукти та яйця", [
        "Згущене молоко",
        "Маргарин",
        "Молоко, йогурти, кефір",
        "Сири",
        "Сметана",
        "Яйця",
    ]),
    ("М'ясо", [
        "Куряче",
        "Фарш",
        "Шашлик",
    ]),
    ("Напої", [
        "Енергетичні напої",
        "Квас",
        "Мінеральні води",
        "Соки",
        "Солодка вода",
    ]),
    ("Насіння", [
        "Усе насіння",
    ]),
    ("Пакети різні", [
        "Пакети",
    ]),
    ("Побутова хімія", [
        "Засоби захисту від комарів, сонця",
        "Мило, мильничка, мочалки",
        "Одноразовий посуд",
        "Паперові вироби",
        "Пральні порошки",
        "Рукавиці",
        "Товари для кухні",
        "Тряпки, губки, пакети сміттєві",
        "Хімія",
    ]),
    ("Риба", [
        "Ікра",
        "Інша риба",
        "Заморожена",
        "Копчена",
        "Оселедець",
        "Червона риба",
    ]),
    ("Снеки та чіпси", [
        "Горішки",
        "Джерки, рибка",
        "Насіння соняшникове, гарбузове",
        "Попкорн",
        "Сухарики, снеки",
        "Чіпси",
    ]),
    ("Солодощі", [
        "Батончики, горошки",
        "Желейки",
        "Жуйки",
        "Печиво, вафлі, бісквіт",
        "Пюре фруктове",
        "Сухофрукти",
        "Цукерки",
        "Шоколад",
    ]),
    ("Соуси та спеції", [
        "Кетчуп",
        "Майонез",
        "Сіль, цукор",
        "Соуси",
        "Спеції",
    ]),
    ("Товари для дітей", [
        "Аксесуари для телефону",
        "Памперси",
    ]),
    ("Тютюнові вироби", [
        "Інші сигарети",
        "Запальнички",
        "Сигарети",
    ]),
    ("Фрукти, овочі", [
        "Овочі",
        "Фрукти",
    ]),
    ("Хліб", [
        "Піддністрянський хліб",
        "Хліб Березина",
        "Хліб Калуш",
        "Хліб Пекарня Стасюка",
    ]),
]


async def import_categories():
    """Імпортує категорії в базу даних."""
    engine = create_async_engine(settings.DATABASE_URL, echo=False)
    session_factory = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)

    async with session_factory() as session:
        # Перевіряємо, чи є вже категорії
        result = await session.execute(text("SELECT count(*) FROM categories"))
        count = result.scalar()

        if count > 0:
            print(f"⚠️  В БД вже є {count} категорій. Видаляю...")
            # Спочатку обнуляємо category_id в товарах
            await session.execute(text("UPDATE products SET category_id = NULL"))
            # Видаляємо категорії (каскадно)
            await session.execute(text("DELETE FROM categories"))
            await session.commit()
            print("   ✅ Категорії очищено")

        # Створюємо категорії
        created_count = 0
        parent_map = {}  # назва_батька -> id

        for parent_name, children_names in CATEGORY_TREE:
            # Створюємо батьківську категорію
            parent_id = uuid.uuid4()
            parent_cat = Category(
                id=parent_id,
                name=parent_name,
                description=None,
                parent_id=None,
            )
            session.add(parent_cat)
            parent_map[parent_name] = parent_id
            created_count += 1

            # Створюємо підкатегорії
            for child_name in children_names:
                child_id = uuid.uuid4()
                child_cat = Category(
                    id=child_id,
                    name=child_name,
                    description=None,
                    parent_id=parent_id,
                )
                session.add(child_cat)
                created_count += 1

        await session.commit()
        print(f"\n✅ Імпорт завершено! Створено {created_count} категорій.")

        # Виводимо результат
        result = await session.execute(
            text("""
                SELECT c1.name AS parent, c2.name AS child
                FROM categories c1
                LEFT JOIN categories c2 ON c2.parent_id = c1.id
                WHERE c1.parent_id IS NULL
                ORDER BY c1.name, c2.name
            """)
        )
        rows = result.all()

        print("\n📂 Структура категорій:")
        current_parent = None
        for parent, child in rows:
            if parent != current_parent:
                print(f"\n  📁 {parent}")
                current_parent = parent
            if child:
                print(f"      📄 {child}")

    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(import_categories())
