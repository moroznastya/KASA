"""
Seed-скрипт для наповнення БД початковими даними.

Створює:
  - Admin користувача (login: admin, password: admin123, PIN: 1111)
  - Cashier користувача (login: cashier, password: cashier123, PIN: 2222)
  - Постачальників
  - Категорії (ієрархічні)
  - Товари

Запуск: python seed.py
"""

import asyncio
from decimal import Decimal
from uuid import uuid4

from sqlalchemy import select, text
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession, async_sessionmaker

from app.config import settings
from app.models.user import User, UserRole
from app.models.supplier import Supplier
from app.models.category import Category
from app.models.product import Product
from app.services.auth_service import AuthService


async def seed():
    """Головна функція seed-скрипта."""
    engine = create_async_engine(settings.DATABASE_URL)
    async_session = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)

    async with async_session() as session:
        # Перевіряємо, чи є вже користувачі
        result = await session.execute(select(User).limit(1))
        if result.scalar_one_or_none():
            print("⚠️  Дані вже існують. Seed пропущено.")
            return

        # ══════════════════════════════════════════════
        # 1. КОРИСТУВАЧІ
        # ══════════════════════════════════════════════
        admin = User(
            id=uuid4(),
            name="Олександр (Адміністратор)",
            login="admin",
            password_hash=AuthService.hash_password("admin123"),
            pin_code=AuthService.hash_password("1111"),
            role=UserRole.ADMIN,
            is_active=True,
        )
        session.add(admin)

        cashier = User(
            id=uuid4(),
            name="Анастасія (Касир)",
            login="cashier",
            password_hash=AuthService.hash_password("cashier123"),
            pin_code=AuthService.hash_password("2222"),
            role=UserRole.CASHIER,
            is_active=True,
        )
        session.add(cashier)
        await session.flush()
        print("✅ Створено користувачів")

        # ══════════════════════════════════════════════
        # 2. ПОСТАЧАЛЬНИКИ
        # ══════════════════════════════════════════════
        supplier1 = Supplier(
            id=uuid4(),
            name='ТОВ "Галицький Дистриб\'ютор"',
            edrpou="12345678",
            phone="+380501112233",
        )
        session.add(supplier1)

        supplier2 = Supplier(
            id=uuid4(),
            name="ФОП Петренко А.В. (Крафтові сири)",
            edrpou="9876543210",
            phone="+380674445566",
        )
        session.add(supplier2)
        await session.flush()
        print("✅ Створено постачальників")

        # ══════════════════════════════════════════════
        # 3. КАТЕГОРІЇ (ієрархічні)
        # ══════════════════════════════════════════════
        cat_bakaliya = Category(id=uuid4(), name="Бакалія")
        cat_alcohol = Category(id=uuid4(), name="Алкогольні напої")
        cat_milk = Category(id=uuid4(), name="Молочні продукти")
        session.add_all([cat_bakaliya, cat_alcohol, cat_milk])
        await session.flush()

        cat_krupy = Category(id=uuid4(), name="Крупи та макарони", parent_id=cat_bakaliya.id)
        cat_beer = Category(id=uuid4(), name="Пиво", parent_id=cat_alcohol.id)
        cat_cheese = Category(id=uuid4(), name="Сири", parent_id=cat_milk.id)
        session.add_all([cat_krupy, cat_beer, cat_cheese])
        await session.flush()
        print("✅ Створено категорії")

        # ══════════════════════════════════════════════
        # 4. ТОВАРИ
        # ══════════════════════════════════════════════
        products = [
            Product(
                id=uuid4(),
                barcode="4820000123456",
                sku="BAC-001",
                title='Макарони "La Pasta" Спіральки 400г',
                price=Decimal("45.50"),
                cost_price=Decimal("32.00"),
                stock=Decimal("50"),
                category_id=cat_krupy.id,
                supplier_id=supplier1.id,
                is_weight=False,
                tax_rate=Decimal("20.00"),
                tax_group="А",
                unit="шт",
            ),
            Product(
                id=uuid4(),
                barcode="4823000999001",
                sku="ALC-005",
                title='Коньяк "Закарпатський" 3* 0.5л',
                price=Decimal("189.00"),
                cost_price=Decimal("130.00"),
                stock=Decimal("12"),
                category_id=cat_beer.id,
                supplier_id=supplier1.id,
                is_weight=False,
                tax_rate=Decimal("20.00"),
                tax_group="А",
                unit="шт",
            ),
            Product(
                id=uuid4(),
                barcode="2100000123454",
                sku="WGT-002",
                title='Сир "Радомер" 45% (ваговий)',
                price=Decimal("320.00"),
                cost_price=Decimal("240.00"),
                stock=Decimal("15.450"),
                category_id=cat_cheese.id,
                supplier_id=supplier2.id,
                is_weight=True,
                tax_rate=Decimal("20.00"),
                tax_group="А",
                unit="кг",
            ),
            Product(
                id=uuid4(),
                barcode="4820002221111",
                sku="DRY-012",
                title='Молоко "Яготинське" 2.5% 900г',
                price=Decimal("38.00"),
                cost_price=Decimal("28.50"),
                stock=Decimal("30"),
                category_id=cat_milk.id,
                supplier_id=supplier1.id,
                is_weight=False,
                tax_rate=Decimal("20.00"),
                tax_group="А",
                unit="шт",
            ),
        ]
        session.add_all(products)
        await session.flush()
        print("✅ Створено товари")

        await session.commit()
        print("\n" + "=" * 50)
        print("🎉 Seed завершено успішно!")
        print("=" * 50)
        print(f"👤 Admin:  login='admin',    password='admin123', PIN='1111'")
        print(f"👤 Cashier: login='cashier', password='cashier123', PIN='2222'")
        print("=" * 50)

    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(seed())
