"""
Seed-скрипт для наповнення БД початковими даними.

Створює:
  - Admin користувача (login: admin, password: admin123, PIN: 1111)
  - Cashier користувача (login: cashier, password: cashier123, PIN: 2222)
  - Постачальників
  - Категорії (ієрархічні)
  - Товари
  - Документи (прибуткова накладна, чек продажу, списання)

Запуск: python seed.py
"""

import asyncio
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from uuid import uuid4

from sqlalchemy import select
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession, async_sessionmaker

from app.config import settings
from app.infrastructure.persistence.models.user import User, UserRole
from app.infrastructure.persistence.models.supplier import Supplier
from app.infrastructure.persistence.models.category import Category
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem, ReceiptType
from app.infrastructure.persistence.models.write_off import WriteOff, WriteOffItem, WriteOffReason
from app.domain.services.auth_service import AuthService


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
        product1 = Product(
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
        )
        product2 = Product(
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
        )
        product3 = Product(
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
        )
        product4 = Product(
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
        )
        session.add_all([product1, product2, product3, product4])
        await session.flush()
        print("✅ Створено товари")

        # ══════════════════════════════════════════════
        # 5. ДОКУМЕНТИ
        # ══════════════════════════════════════════════

        # ── 5.1 Прибуткова накладна (Invoice) ────────
        now = datetime.now(timezone.utc).replace(tzinfo=None)
        invoice = Invoice(
            id=uuid4(),
            number="INV-2026-0001",
            supplier_id=supplier1.id,
            invoice_date=now - timedelta(days=3),
            status=InvoiceStatus.CONFIRMED.value,  # Використовуємо .value для lowercase
            notes="Основне постачання товарів для поповнення складу",
            total_amount=Decimal("0.00"),
        )
        session.add(invoice)
        await session.flush()

        # Позиції накладної
        invoice_items = [
            InvoiceItem(
                id=uuid4(),
                invoice_id=invoice.id,
                product_id=product1.id,
                quantity=Decimal("20"),
                price=Decimal("32.00"),
                total=Decimal("640.00"),
            ),
            InvoiceItem(
                id=uuid4(),
                invoice_id=invoice.id,
                product_id=product2.id,
                quantity=Decimal("10"),
                price=Decimal("130.00"),
                total=Decimal("1300.00"),
            ),
            InvoiceItem(
                id=uuid4(),
                invoice_id=invoice.id,
                product_id=product4.id,
                quantity=Decimal("25"),
                price=Decimal("28.50"),
                total=Decimal("712.50"),
            ),
        ]
        session.add_all(invoice_items)
        invoice.total_amount = sum(item.total for item in invoice_items)
        print(f"✅ Створено прибуткову накладну: {invoice.number} на суму {invoice.total_amount} грн")

        # ── 5.2 Чек продажу (Receipt) ────────────────
        receipt = Receipt(
            id=uuid4(),
            receipt_number="RCPT-2026-0001",
            receipt_type=ReceiptType.SALE.value,  # Використовуємо .value для lowercase
            cashier_id=cashier.id,
            total_amount=Decimal("0.00"),
            is_return=False,
            notes="Продаж товарів покупцю",
        )
        session.add(receipt)
        await session.flush()

        receipt_items = [
            ReceiptItem(
                id=uuid4(),
                receipt_id=receipt.id,
                product_id=product1.id,
                quantity=Decimal("2"),
                price=Decimal("45.50"),
                total=Decimal("91.00"),
            ),
            ReceiptItem(
                id=uuid4(),
                receipt_id=receipt.id,
                product_id=product4.id,
                quantity=Decimal("1"),
                price=Decimal("38.00"),
                total=Decimal("38.00"),
            ),
        ]
        session.add_all(receipt_items)
        receipt.total_amount = sum(item.total for item in receipt_items)
        print(f"✅ Створено чек продажу: {receipt.receipt_number} на суму {receipt.total_amount} грн")

        # ── 5.3 Списання (WriteOff) ──────────────────
        write_off = WriteOff(
            id=uuid4(),
            number="WO-2026-0001",
            reason=WriteOffReason.EXPIRED.value,  # Використовуємо .value для lowercase
            write_off_date=now - timedelta(days=1),
            notes="Списання прострочених молочних продуктів",
            status="confirmed",
            total_amount=Decimal("0.00"),
        )
        session.add(write_off)
        await session.flush()

        write_off_items = [
            WriteOffItem(
                id=uuid4(),
                write_off_id=write_off.id,
                product_id=product4.id,
                quantity=Decimal("3"),
            ),
        ]
        session.add_all(write_off_items)
        write_off.total_amount = Decimal("85.50")  # 3 * 28.50
        print(f"✅ Створено списання: {write_off.number} на суму {write_off.total_amount} грн")

        # ══════════════════════════════════════════════
        # 6. ЗБЕРЕЖЕННЯ
        # ══════════════════════════════════════════════
        await session.commit()
        print("\n" + "=" * 50)
        print("🎉 Seed завершено успішно!")
        print("=" * 50)
        print(f"👤 Admin:  login='admin',    password='admin123', PIN='1111'")
        print(f"👤 Cashier: login='cashier', password='cashier123', PIN='2222'")
        print(f"📄 Документи:")
        print(f"   - Накладна: {invoice.number} ({invoice.total_amount} грн)")
        print(f"   - Чек: {receipt.receipt_number} ({receipt.total_amount} грн)")
        print(f"   - Списання: {write_off.number} ({write_off.total_amount} грн)")
        print("=" * 50)

    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(seed())
