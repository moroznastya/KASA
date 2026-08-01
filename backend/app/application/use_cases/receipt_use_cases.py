"""
Use Cases для Receipt (Чек продажу).

Реалізує бізнес-логіку для роботи з чеками продажу:
- CreateReceipt: створення чеку продажу (sale/return)
- GetReceipts: отримання списку чеків з фільтрацією

Інтеграція з ПРРО:
- Після створення чеку (sale/return) авто-фіскалізація
  (fiscalize_receipt) ставиться у ФОН (FastAPI BackgroundTasks), якщо
  передано background_tasks; інакше виконується синхронно (fallback).
- HTTP-відповідь повертається одразу зі статусом "pending"; статус
  фіскалізації оновлюється в БД та доступний через GET /receipts/{id}.
- Фіскалізація обгорнута в try/except — проблеми з ПРРО НЕ блокують продаж.
"""

from __future__ import annotations

import logging
from datetime import datetime
from decimal import Decimal
from typing import Callable, Optional
from uuid import UUID

from app.domain.entities.receipt import Receipt, PaymentMethod
from app.domain.repositories import IReceiptRepository, IProductRepository
from app.domain.repositories.i_unit_of_work import IUnitOfWork
from app.application.dto.receipt_dto import ReceiptDTO, ReceiptCreateDTO
from app.application.mappers.receipt_mapper import ReceiptMapper
from app.application.interfaces.i_event_bus import IEventBus
from app.domain.events import ReceiptCreated, ReceiptRefunded

logger = logging.getLogger(__name__)


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
        fiscalizer_factory: Optional[Callable[[], object]] = None,
    ):
        """
        Ініціалізація Use Cases.

        Args:
            receipt_repo: Репозиторій чеків.
            product_repo: Репозиторій товарів.
            unit_of_work: Unit of Work для транзакцій.
            event_bus: Event Bus для публікації подій.
            fiscalizer_factory: Опційна фабрика FiscalizeReceiptUseCase
                для авто-фіскалізації після створення чеку. Якщо None —
                фіскалізація не виконується.
        """
        self._receipt_repo = receipt_repo
        self._product_repo = product_repo
        self._uow = unit_of_work
        self._event_bus = event_bus
        self._fiscalizer_factory = fiscalizer_factory

    async def _auto_fiscalize(
        self,
        receipt_id: UUID,
        background_tasks=None,
    ) -> None:
        """
        Авто-фіскалізація чеку: у фон (BackgroundTasks) або синхронно.

        Якщо передано background_tasks (FastAPI BackgroundTasks) — gRPC-виклик
        ПРРО (таймаут 30с × 3 ретраї) виконується ПІСЛЯ HTTP-відповіді,
        тому продаж не "зависає" до 90 секунд. HTTP-відповідь повертається
        одразу, статус фіскалізації оновлюється в БД (fiscal_status) та
        доступний через GET /receipts/{id}.

        Якщо background_tasks не передано (CLI/тести) — виконується
        синхронно (не блокує продаж при помилках ПРРО).

        Args:
            receipt_id: ID створеного чеку.
            background_tasks: FastAPI BackgroundTasks або None.
        """
        if self._fiscalizer_factory is None:
            return
        if background_tasks is not None:
            background_tasks.add_task(self._run_fiscalize, receipt_id)
            logger.info(
                "ПРРО: авто-фіскалізація чеку %s поставлена у фон", receipt_id
            )
            return
        await self._run_fiscalize(receipt_id)

    async def _run_fiscalize(self, receipt_id: UUID) -> None:
        """
        Виконує фіскалізацію (фонова задача або синхронний виклик).

        Args:
            receipt_id: ID створеного чеку.
        """
        fiscalizer = None
        try:
            fiscalizer = self._fiscalizer_factory()
            await fiscalizer.fiscalize_receipt(receipt_id, manual=False)
        except Exception:  # noqa: BLE001
            logger.exception(
                "ПРРО: авто-фіскалізація чеку %s не вдалася "
                "(продаж не заблоковано)", receipt_id
            )
        finally:
            # Сесія фіскалізації створюється фабрикою окремо від
            # per-request сесії — закриваємо її після завершення.
            if fiscalizer is not None:
                session = getattr(fiscalizer, "session", None)
                if session is not None:
                    try:
                        await session.close()
                    except Exception:  # noqa: BLE001
                        logger.debug(
                            "ПРРО: помилка закриття сесії фіскалізації",
                            exc_info=True,
                        )

    async def create_sale_receipt(
        self,
        dto: ReceiptCreateDTO,
        background_tasks=None,
    ) -> ReceiptDTO:
        """
        Створює чек продажу (зменшує залишки товарів).

        Args:
            dto: DTO з даними для створення чеку.
            background_tasks: FastAPI BackgroundTasks для авто-фіскалізації
                у фоні (якщо None — фіскалізація виконується синхронно).

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

                # Зменшуємо залишок (Quantity не допускає від'ємних значень —
                # обчислюємо нове значення через віднімання)
                if product.stock is not None:
                    product.stock = product.stock - item.quantity
                await self._product_repo.update(product)

            # Зберігаємо чек
            saved = await self._receipt_repo.save(receipt)
            await self._uow.commit()

        # Публікуємо подію ReceiptCreated
        event = ReceiptCreated(
            receipt_id=saved.id,
            cashier_id=getattr(saved, "cashier_id", None),
            total_amount=saved.total or Decimal("0"),
            payment_method=saved.payment_method.value if hasattr(saved.payment_method, 'value') else str(saved.payment_method),
        )
        await self._event_bus.publish(event)

        # Авто-фіскалізація: у фон (BackgroundTasks) або синхронно
        await self._auto_fiscalize(saved.id, background_tasks)

        return ReceiptMapper.entity_to_dto(saved)

    async def create_return_receipt(
        self,
        dto: ReceiptCreateDTO,
        background_tasks=None,
    ) -> ReceiptDTO:
        """
        Створює чек повернення (збільшує залишки товарів).

        Args:
            dto: DTO з даними для створення чеку повернення.
            background_tasks: FastAPI BackgroundTasks для авто-фіскалізації
                у фоні (якщо None — фіскалізація виконується синхронно).

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

        # Авто-фіскалізація: у фон (BackgroundTasks) або синхронно
        await self._auto_fiscalize(saved.id, background_tasks)

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
