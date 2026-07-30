"""
Use Cases: Product (Товари).

Кожен Use Case виконує одну бізнес-операцію:
- CreateProductUseCase: створення товару
- UpdateProductUseCase: оновлення товару
- DeleteProductUseCase: видалення товару
- SearchProductsUseCase: пошук товарів

Валідація виконується всередині Use Case, а не в сервісах чи репозиторіях.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from typing import Generic, Optional, TypeVar
from uuid import UUID

from app.domain.entities.product import Product
from app.domain.repositories import IProductRepository, ICategoryRepository, IUnitOfWork
from app.domain.services.pricing_service import PricingService
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.barcode import Barcode

T = TypeVar("T")


@dataclass
class PaginatedResult(Generic[T]):
    """Результат пошуку з пагінацією."""

    items: list[T] = field(default_factory=list)
    total: int = 0
    page: int = 1
    size: int = 20

    @property
    def total_pages(self) -> int:
        """Загальна кількість сторінок."""
        if self.size <= 0:
            return 0
        return (self.total + self.size - 1) // self.size

    @property
    def has_next(self) -> bool:
        """Чи є наступна сторінка."""
        return self.page < self.total_pages

    @property
    def has_prev(self) -> bool:
        """Чи є попередня сторінка."""
        return self.page > 1


class CreateProductUseCase:
    """
    Створення нового товару.

    Валідація:
    - Назва не може бути пустою
    - Штрих-код має бути унікальним
    - Ціна продажу має бути >= собівартості
    - Категорія (якщо вказана) має існувати
    - Ціна та кількість мають бути невід'ємними
    """

    def __init__(
        self,
        product_repo: IProductRepository,
        category_repo: ICategoryRepository,
        pricing_service: PricingService,
        uow: IUnitOfWork,
    ) -> None:
        self._product_repo = product_repo
        self._category_repo = category_repo
        self._pricing_service = pricing_service
        self._uow = uow

    async def execute(
        self,
        title: str,
        barcode: str,
        price: Decimal,
        cost_price: Decimal,
        category_id: Optional[UUID] = None,
        quantity: Decimal = Decimal("0"),
        supplier_id: Optional[UUID] = None,
        unit: str = "шт",
    ) -> Product:
        # Валідація: назва не пуста
        if not title or not title.strip():
            raise ValueError("Назва товару не може бути пустою")

        title = title.strip()

        # Валідація: штрих-код не пустий
        if not barcode or not barcode.strip():
            raise ValueError("Штрих-код товару не може бути пустим")

        barcode = barcode.strip()

        # Валідація: унікальність штрих-коду
        exists = await self._product_repo.exists_by_barcode(barcode)
        if exists:
            raise ValueError(f"Товар з штрих-кодом '{barcode}' вже існує")

        # Валідація: ціна не від'ємна
        if price < Decimal("0"):
            raise ValueError("Ціна товару не може бути від'ємною")

        # Валідація: собівартість не від'ємна
        if cost_price < Decimal("0"):
            raise ValueError("Собівартість товару не може бути від'ємною")

        # Валідація: ціна >= собівартість
        if price < cost_price:
            raise ValueError(
                f"Ціна продажу ({price}) не може бути меншою за собівартість ({cost_price})"
            )

        # Валідація: кількість не від'ємна
        if quantity < Decimal("0"):
            raise ValueError("Кількість товару не може бути від'ємною")

        # Валідація: категорія існує
        if category_id is not None:
            category = await self._category_repo.find_by_id(category_id)
            if category is None:
                raise ValueError(
                    f"Категорію з ID '{category_id}' не знайдено"
                )

        # Створення value objects
        money_price = Money(price)
        money_cost = Money(cost_price)
        stock_qty = Quantity(quantity, unit)

        try:
            barcode_obj = Barcode(barcode)
        except ValueError as e:
            raise ValueError(f"Невірний формат штрих-коду: {e}")

        # Створення entity
        product = Product(
            name=title,
            barcode=barcode_obj,
            price=money_price,
            cost_price=money_cost,
            stock=stock_qty,
            category_id=category_id,
            supplier_id=supplier_id,
            unit=unit,
        )

        # Збереження
        saved = await self._product_repo.save(product)
        await self._uow.commit()
        return saved


class UpdateProductUseCase:
    """
    Оновлення існуючого товару.

    Валідація:
    - Товар з вказаним ID має існувати
    - Назва (якщо вказана) не може бути пустою
    - Штрих-код (якщо вказаний) має бути унікальним
    - Ціна >= собівартість
    - Категорія (якщо вказана) має існувати
    """

    def __init__(
        self,
        product_repo: IProductRepository,
        category_repo: ICategoryRepository,
        pricing_service: PricingService,
        uow: IUnitOfWork,
    ) -> None:
        self._product_repo = product_repo
        self._category_repo = category_repo
        self._pricing_service = pricing_service
        self._uow = uow

    async def execute(
        self,
        product_id: UUID,
        title: Optional[str] = None,
        barcode: Optional[str] = None,
        price: Optional[Decimal] = None,
        cost_price: Optional[Decimal] = None,
        category_id: Optional[UUID] = None,
        quantity: Optional[Decimal] = None,
        supplier_id: Optional[UUID] = None,
        unit: Optional[str] = None,
    ) -> Product:
        # Валідація: товар існує
        product = await self._product_repo.find_by_id(product_id)
        if product is None:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")

        # Валідація та оновлення назви
        if title is not None:
            if not title.strip():
                raise ValueError("Назва товару не може бути пустою")
            product.name = title.strip()

        # Валідація та оновлення штрих-коду
        if barcode is not None:
            if not barcode.strip():
                raise ValueError("Штрих-код товару не може бути пустим")
            barcode = barcode.strip()
            # Перевірка унікальності
            exists = await self._product_repo.exists_by_barcode(barcode, exclude_id=product_id)
            if exists:
                raise ValueError(f"Товар з штрих-кодом '{barcode}' вже існує")
            try:
                product.barcode = Barcode(barcode)
            except ValueError as e:
                raise ValueError(f"Невірний формат штрих-коду: {e}")

        # Валідація та оновлення ціни
        if price is not None:
            if price < Decimal("0"):
                raise ValueError("Ціна товару не може бути від'ємною")
            final_cost = cost_price if cost_price is not None else (
                product.cost_price.amount if product.cost_price else Decimal("0")
            )
            if price < final_cost:
                raise ValueError(
                    f"Ціна продажу ({price}) не може бути меншою за собівартість ({final_cost})"
                )
            product.price = Money(price)

        # Оновлення собівартості
        if cost_price is not None:
            if cost_price < Decimal("0"):
                raise ValueError("Собівартість товару не може бути від'ємною")
            final_price = price if price is not None else (
                product.price.amount if product.price else Decimal("0")
            )
            if final_price < cost_price:
                raise ValueError(
                    f"Ціна продажу ({final_price}) не може бути меншою за собівартість ({cost_price})"
                )
            product.cost_price = Money(cost_price)

        # Валідація та оновлення кількості
        if quantity is not None:
            if quantity < Decimal("0"):
                raise ValueError("Кількість товару не може бути від'ємною")
            current_unit = unit or product.unit
            product.stock = Quantity(quantity, current_unit)

        # Оновлення одиниці виміру
        if unit is not None:
            product.unit = unit

        # Валідація та оновлення категорії
        if category_id is not None:
            category = await self._category_repo.find_by_id(category_id)
            if category is None:
                raise ValueError(f"Категорію з ID '{category_id}' не знайдено")
            product.category_id = category_id

        # Оновлення постачальника
        if supplier_id is not None:
            product.supplier_id = supplier_id

        updated = await self._product_repo.update(product)
        await self._uow.commit()
        return updated


class DeleteProductUseCase:
    """
    Видалення товару.

    Валідація:
    - Товар з вказаним ID має існувати
    """

    def __init__(
        self,
        product_repo: IProductRepository,
        uow: IUnitOfWork,
    ) -> None:
        self._product_repo = product_repo
        self._uow = uow

    async def execute(self, product_id: UUID) -> None:
        # Валідація: товар існує
        product = await self._product_repo.find_by_id(product_id)
        if product is None:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")

        await self._product_repo.delete(product_id)
        await self._uow.commit()


class SearchProductsUseCase:
    """
    Пошук товарів за текстовим запитом з пагінацією.

    Не потребує транзакції (тільки читання).
    """

    def __init__(
        self,
        product_repo: IProductRepository,
    ) -> None:
        self._product_repo = product_repo

    async def execute(
        self,
        query: str,
        page: int = 1,
        size: int = 20,
    ) -> PaginatedResult[Product]:
        if page < 1:
            raise ValueError("Номер сторінки має бути >= 1")
        if size < 1 or size > 100:
            raise ValueError("Розмір сторінки має бути від 1 до 100")

        items, total = await self._product_repo.search(
            query=query if query.strip() else None,
            page=page,
            size=size,
        )

        return PaginatedResult(
            items=items,
            total=total,
            page=page,
            size=size,
        )
