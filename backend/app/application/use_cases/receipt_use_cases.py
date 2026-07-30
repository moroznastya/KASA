"""
Use Cases для Receipt (Чек продажу).

Реалізує бізнес-логіку для роботи з чеками продажу:
- CreateReceipt: створення чеку продажу (sale/return)
- GetReceipts: отримання списку чеків з фільтрацією
"""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from app.domain.entities.receipt import Receipt, PaymentMethod
from app.domain.repositories import IReceiptRepository, IProductRepository
from app.domain.repositories.i_unit_of_work import IUnitOfWork
from app.application.dto.receipt_dto import ReceiptDTO, ReceiptCreateDTO
from app.application.mappers.receipt_mapper import ReceiptMapper
from app.application.interfaces.i_event_bus import IEventBus
from app.domain.events import ReceiptCreated, ReceiptRefunded


class ReceiptUseCases:
    """
    Use Cases для чеків продажу.

    Використовує Dependency Injection через конструктор.
    Залежності: IReceiptRepository, IProductRepository, IUnitOfWork, IEventBus.
    """

    def __init__(
        self,
        receipt_repo: IReceiptRepository,
        product_repo: IProductRepository,
        unit_of_work: IUnitOfWork,
        event_bus: IEventBus,
    ):
        """
        Ініціалізація Use Cases.

        Args:
            receipt_repo: Репозиторій чеків.
            product_repo: Репозиторій товарів.
            unit_of_work: Unit of Work для транзакцій.
            event_bus: Event Bus для публікації подій.
        """
        self._receipt_repo = receipt_repo
        self._product_repo = product_repo
        self._uow = unit_of_work
        self._event_bus = event_bus

    async def create_sale_receipt(self, dto: ReceiptCreateDTO) -> ReceiptDTO:
        """
        Створює чек продажу (зменшує залишки товарів).

        Args:
            dto: DTO з даними для створення чеку.

        Returns:
            ReceiptDTO створеного чеку.

        Raises:
            ValueError: Якщо товар не знайдено або недостатньо залишку.
        """
        # Конвертуємо DTO в Entity
        receipt = ReceiptMapper.create_dto_to_entity(dto)

        async with self._uow:
            # Перевіряємо наявність товарів та оновлюємо залишки
            for item in receipt.items:
                product = await self._product_repo.find_by_id(item.product_id)
                if not product:
                    raise ValueError(f"Товар з ID '{item.product_id}' не знайдено")

                if product.stock and product.stock < item.quantity:
                    raise ValueError(
                        f"Недостатньо залишку товару '{product.name}': "
                        f"доступно {product.stock.value}, потрібно {item.quantity.value}"
                    )

                # Зменшуємо залишок (від'ємна кількість)
                product.update_stock(item.quantity * -1)
                await self._product_repo.update(product)

            # Зберігаємо чек
            saved = await self._receipt_repo.save(receipt)
            await self._uow.commit()

        # Публікуємо подію ReceiptCreated
        event = ReceiptCreated(
            receipt_id=saved.id,
            cashier_id=saved.cashier_id,
            total_amount=saved.total or Decimal("0"),
            payment_method=saved.payment_method.value if hasattr(saved.payment_method, 'value') else str(saved.payment_method),
        )
        await self._event_bus.publish(event)

        return ReceiptMapper.entity_to_dto(saved)

    async def create_return_receipt(self, dto: ReceiptCreateDTO) -> ReceiptDTO:
        """
        Створює чек повернення (збільшує залишки товарів).

        Args:
            dto: DTO з даними для створення чеку повернення.

        Returns:
            ReceiptDTO створеного чеку повернення.

        Raises:
            ValueError: Якщо товар не знайдено.
        """
        # Конвертуємо DTO в Entity
        receipt = ReceiptMapper.create_dto_to_entity(dto)

        async with self._uow:
            # Повертаємо товари на склад (збільшуємо залишки)
            for item in receipt.items:
                product = await self._product_repo.find_by_id(item.product_id)
                if not product:
                    raise ValueError(f"Товар з ID '{item.product_id}' не знайдено")

                # Збільшуємо залишок
                product.update_stock(item.quantity)
                await self._product_repo.update(product)

            # Зберігаємо чек
            saved = await self._receipt_repo.save(receipt)
            await self._uow.commit()

        # Публікуємо подію ReceiptRefunded
        event = ReceiptRefunded(
            receipt_id=saved.id,
            original_receipt_id=getattr(saved, 'original_receipt_id', saved.id) or saved.id,
            refund_amount=saved.total or Decimal("0"),
        )
        await self._event_bus.publish(event)

        return ReceiptMapper.entity_to_dto(saved)

    async def get_receipt(self, receipt_id: UUID) -> ReceiptDTO:
        """
        Отримує чек за ID.

        Args:
            receipt_id: ID чеку.

        Returns:
            ReceiptDTO чеку.

        Raises:
            ValueError: Якщо чек не знайдено.
        """
        receipt = await self._receipt_repo.find_by_id(receipt_id)
        if not receipt:
            raise ValueError(f"Чек з ID '{receipt_id}' не знайдено")
        return ReceiptMapper.entity_to_dto(receipt)

    async def get_receipts(
        self,
        query: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        payment_method: Optional[str] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[ReceiptDTO], int]:
        """
        Отримує список чеків з фільтрацією та пагінацією.

        Args:
            query: Текстовий пошук.
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            payment_method: Фільтр за способом оплати.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список ReceiptDTO, загальна кількість).
        """
        receipts, total = await self._receipt_repo.search(
            query=query,
            date_from=date_from,
            date_to=date_to,
            payment_method=payment_method,
            page=page,
            size=size,
        )
        return [ReceiptMapper.entity_to_dto(r) for r in receipts], total

    async def get_daily_total(self, date: datetime) -> float:
        """
        Повертає загальну суму продажів за день.

        Args:
            date: Дата.

        Returns:
            Загальна сума.
        """
        return await self._receipt_repo.get_daily_total(date)
