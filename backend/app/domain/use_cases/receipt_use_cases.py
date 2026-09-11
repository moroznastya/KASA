"""
Use Cases: Receipt (Чеки продажу).

Кожен Use Case виконує одну бізнес-операцію:
- CreateReceiptUseCase: створення чеку продажу
- ReturnReceiptUseCase: повернення товару за чеком

Валідація виконується всередині Use Case, а не в сервісах чи репозиторіях.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from uuid import UUID

from app.domain.entities.receipt import PaymentMethod, Receipt, ReceiptItem
from app.domain.repositories import IProductRepository, IReceiptRepository, IUnitOfWork
from app.domain.services.document_service import DocumentService
from app.domain.services.stock_service import StockService
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate


@dataclass
class ReceiptItemCreate:
    """Вхідні дані для створення позиції чеку продажу."""

    product_id: UUID
    quantity: Decimal
    price: Decimal
    tax_rate_percent: int = 20


@dataclass
class ReturnItemCreate:
    """Вхідні дані для повернення позиції."""

    product_id: UUID
    quantity: Decimal  # Кількість для повернення
    price: Decimal
    tax_rate_percent: int = 20


class CreateReceiptUseCase:
    """
    Створення нового чеку продажу.

    При створенні:
    1. Перевіряє достатність залишків
    2. Зменшує залишки товарів
    3. Створює чек з позиціями

    Валідація:
    - Список позицій не може бути пустим
    - Кожен товар має існувати
    - Кількість має бути додатною
    - Має бути достатньо товару на складі
    - Спосіб оплати має бути валідним
    """

    def __init__(
        self,
        receipt_repo: IReceiptRepository,
        product_repo: IProductRepository,
        stock_service: StockService,
        document_service: DocumentService,
        uow: IUnitOfWork,
    ) -> None:
        self._receipt_repo = receipt_repo
        self._product_repo = product_repo
        self._stock_service = stock_service
        self._document_service = document_service
        self._uow = uow

    async def execute(
        self,
        items: list[ReceiptItemCreate],
        payment_method: str = "cash",
        notes: str = "",
    ) -> Receipt:
        # Валідація: список позицій не пустий
        if not items:
            raise ValueError("Чек повинен мати хоча б одну позицію")

        # Валідація способу оплати
        try:
            method = PaymentMethod(payment_method)
        except ValueError:
            raise ValueError(
                f"Невідомий спосіб оплати: '{payment_method}'. "
                f"Доступні: {[m.value for m in PaymentMethod]}"
            )

        # Валідація та створення позицій з перевіркою залишків
        receipt_items: list[ReceiptItem] = []
        for i, item_data in enumerate(items):
            product = await self._product_repo.find_by_id(item_data.product_id)
            if product is None:
                raise ValueError(
                    f"Товар з ID '{item_data.product_id}' (позиція {i + 1}) не знайдено"
                )

            if item_data.quantity <= Decimal("0"):
                raise ValueError(
                    f"Кількість товару (позиція {i + 1}) повинна бути додатною, "
                    f"отримано {item_data.quantity}"
                )

            # Перевірка залишку
            if product.stock is not None:
                required_qty = Quantity(item_data.quantity, product.unit)
                if not self._stock_service.check_sufficient(product.stock, required_qty):
                    raise ValueError(
                        f"Недостатньо товару '{product.name}' на складі: "
                        f"доступно {product.stock.value} {product.unit}, "
                        f"потрібно {item_data.quantity}"
                    )

                # Зменшуємо залишок (Quantity не може бути від'ємним,
                # тому встановлюємо нове значення напряму)
                new_stock_value = product.stock.value - item_data.quantity
                product.stock = Quantity(new_stock_value, product.stock.unit)
                await self._product_repo.update(product)

            # Створення позиції чеку
            tax_rate = TaxRate.from_percent(item_data.tax_rate_percent)
            receipt_item = ReceiptItem(
                product_id=item_data.product_id,
                name=product.name,
                quantity=Quantity(item_data.quantity, product.unit),
                price=Money(item_data.price),
                tax_rate=tax_rate,
            )
            receipt_items.append(receipt_item)

        # Створення чеку
        receipt = Receipt(
            items=receipt_items,
            payment_method=method,
            notes=notes,
        )

        saved = await self._receipt_repo.save(receipt)
        await self._uow.commit()
        return saved


class ReturnReceiptUseCase:
    """
    Повернення товару за чеком.

    При поверненні:
    1. Перевіряє існування оригінального чеку
    2. Збільшує залишки товарів
    3. Створює чек повернення

    Валідація:
    - Оригінальний чек має існувати
    - Список позицій для повернення не може бути пустим
    - Кількість для повернення має бути додатною
    - Кожен товар має існувати
    """

    def __init__(
        self,
        receipt_repo: IReceiptRepository,
        product_repo: IProductRepository,
        stock_service: StockService,
        uow: IUnitOfWork,
    ) -> None:
        self._receipt_repo = receipt_repo
        self._product_repo = product_repo
        self._stock_service = stock_service
        self._uow = uow

    async def execute(
        self,
        original_receipt_id: UUID,
        items: list[ReturnItemCreate],
        notes: str = "",
    ) -> Receipt:
        # Валідація: оригінальний чек існує
        original = await self._receipt_repo.find_by_id(original_receipt_id)
        if original is None:
            raise ValueError(f"Чек з ID '{original_receipt_id}' не знайдено")

        # Валідація: список позицій не пустий
        if not items:
            raise ValueError("Повернення повинно мати хоча б одну позицію")

        # Валідація та створення позицій повернення
        return_items: list[ReceiptItem] = []
        for i, item_data in enumerate(items):
            product = await self._product_repo.find_by_id(item_data.product_id)
            if product is None:
                raise ValueError(
                    f"Товар з ID '{item_data.product_id}' (позиція {i + 1}) не знайдено"
                )

            if item_data.quantity <= Decimal("0"):
                raise ValueError(
                    f"Кількість товару для повернення (позиція {i + 1}) "
                    f"повинна бути додатною, отримано {item_data.quantity}"
                )

            # Збільшуємо залишок (повернення на склад)
            return_qty = Quantity(item_data.quantity, product.unit)
            if product.stock is not None:
                new_stock_value = product.stock.value + return_qty.value
                product.stock = Quantity(new_stock_value, product.stock.unit)
            else:
                product.stock = return_qty
            await self._product_repo.update(product)

            # Створення позиції повернення
            tax_rate = TaxRate.from_percent(item_data.tax_rate_percent)
            return_item = ReceiptItem(
                product_id=item_data.product_id,
                name=product.name,
                quantity=Quantity(item_data.quantity, product.unit),
                price=Money(item_data.price),
                tax_rate=tax_rate,
            )
            return_items.append(return_item)

        # Створення чеку повернення
        receipt = Receipt(
            items=return_items,
            payment_method=original.payment_method,
            notes=notes,
        )

        saved = await self._receipt_repo.save(receipt)
        await self._uow.commit()
        return saved
