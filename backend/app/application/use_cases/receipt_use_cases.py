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
from uuid import UUID, uuid4

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
        receipt.receipt_type = "sale"
        receipt.cashier_id = dto.cashier_id
        if not receipt.number:
            receipt.number = f"RCPT-{datetime.now().strftime('%Y%m%d')}-{uuid4().hex[:6].upper()}"

        async with self._uow:
            # Перевіряємо наявність товарів та оновлюємо залишки
            for item in receipt.items:
                product = await self._product_repo.find_by_id(item.product_id)
                if not product:
                    raise ValueError(f"Товар з ID '{item.product_id}' не знайдено")

                stock = _stock_as_float(product.stock)
                if stock is not None and stock < float(item.quantity.value):
                    raise ValueError(
                        f"Недостатньо залишку товару '{getattr(product, 'name', getattr(product, 'title', ''))}': "
                        f"доступно {stock}, потрібно {item.quantity.value}"
                    )

                # Зменшуємо залишок (ORM Product зберігає stock як число)
                if stock is not None:
                    product.stock = stock - float(item.quantity.value)
                await self._product_repo.update(product)

            # Зберігаємо чек
            saved = await self._receipt_repo.save(receipt)
            await self._uow.commit()

        # Публікуємо подію ReceiptCreated
        event = ReceiptCreated(
            receipt_id=saved.id,
            cashier_id=getattr(saved, "cashier_id", None),
            total_amount=(
                getattr(saved, "total", None) or getattr(saved, "total_amount", None) or Decimal("0")
            ),
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
        receipt.receipt_type = "return"
        receipt.cashier_id = dto.cashier_id
        if not receipt.number:
            receipt.number = f"RCPT-{datetime.now().strftime('%Y%m%d')}-{uuid4().hex[:6].upper()}"

        async with self._uow:
            # Повертаємо товари на склад (збільшуємо залишки)
            for item in receipt.items:
                product = await self._product_repo.find_by_id(item.product_id)
                if not product:
                    raise ValueError(f"Товар з ID '{item.product_id}' не знайдено")

                # Збільшуємо залишок (ORM Product зберігає stock як число)
                stock = _stock_as_float(product.stock)
                product.stock = (stock or 0) + float(item.quantity.value)
                await self._product_repo.update(product)

            # Зберігаємо чек
            saved = await self._receipt_repo.save(receipt)
            await self._uow.commit()

        # Публікуємо подію ReceiptRefunded
        event = ReceiptRefunded(
            receipt_id=saved.id,
            original_receipt_id=getattr(saved, 'original_receipt_id', saved.id) or saved.id,
            refund_amount=(
                getattr(saved, "total", None) or getattr(saved, "total_amount", None) or Decimal("0")
            ),
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

    # ─── Статистика, пошук, повернення ─────────────────────────────────────

    async def get_today_stats(self) -> dict:
        """
        Повертає статистику чеків за сьогодні (UTC).

        Returns:
            dict: {total_sales, total_returns, total_profit, total_vat,
                   receipts_count, items_sold, date}.
        """
        return await self._receipt_repo.get_today_stats()

    async def search_receipts(
        self,
        q: str = "",
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        receipt_type: Optional[str] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[dict], int]:
        """
        Пошук чеків для повернень (за номером або назвою товару).

        Args:
            q: Пошуковий запит.
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            receipt_type: Тип чеку ("sale"/"return").
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (list[dict] спрощених чеків, загальна кількість).
        """
        from app.infrastructure.persistence.models.receipt import ReceiptType

        type_enum = None
        if receipt_type:
            try:
                type_enum = ReceiptType(receipt_type)
            except ValueError:
                raise ValueError(
                    f"Невірний тип чеку '{receipt_type}'. Використовуйте 'sale' або 'return'"
                )

        receipts, total = await self._receipt_repo.search_with_details(
            q=q,
            date_from=date_from,
            date_to=date_to,
            receipt_type=type_enum,
            page=page,
            size=size,
        )

        items = []
        for r in receipts:
            items.append({
                "id": r.id,
                "receipt_number": r.receipt_number,
                "receipt_type": r.receipt_type.value
                if hasattr(r.receipt_type, "value")
                else str(r.receipt_type),
                "total_amount": float(r.total_amount or 0),
                "created_at": r.created_at,
                "cashier_name": r.cashier.name if r.cashier else "",
                "items_count": len(r.items),
            })
        return items, total

    async def get_recent_sales_by_product(
        self,
        query: str,
        limit: int = 5,
    ) -> list[dict]:
        """
        Останні продажі товарів за штрих-кодом або назвою (для повернення).

        Args:
            query: Штрих-код або назва товару.
            limit: Кількість останніх продажів (1-20).

        Returns:
            list[dict]: [{product, total_sold, total_returned,
                          returnable, recent_sales}].

        Raises:
            ValueError: Якщо товарів за запитом не знайдено.
        """
        items = await self._receipt_repo.find_recent_sales_by_product(query, limit)
        if not items:
            raise ValueError(
                f"Товарів за запитом '{query}' не знайдено. "
                "Спробуйте ввести штрих-код або назву товару"
            )
        return items

    async def get_returnable_quantity(self, product_id: UUID) -> dict:
        """
        Скільки одиниць товару можна повернути.

        Args:
            product_id: ID товару.

        Returns:
            dict: {product_id, total_sold, total_returned, returnable}.

        Raises:
            ValueError: Якщо товар не знайдено.
        """
        product = await self._product_repo.find_by_id(product_id)
        if not product:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")

        total_sold, total_returned = await self._receipt_repo.get_sold_returned_totals(
            product_id
        )
        returnable = await self._receipt_repo.get_returnable_quantity(product_id)

        return {
            "product_id": str(product_id),
            "total_sold": float(total_sold),
            "total_returned": float(total_returned),
            "returnable": float(returnable),
        }

    async def get_receipt_items(self, receipt_id: UUID) -> list[dict]:
        """
        Отримує всі позиції чеку (для вибору товарів при поверненні).

        Args:
            receipt_id: ID чеку.

        Returns:
            list[dict]: позиції з product_name/product_barcode.

        Raises:
            ValueError: Якщо чек не знайдено.
        """
        receipt = await self._receipt_repo.find_by_id(receipt_id)
        if not receipt:
            raise ValueError(f"Чек з ID '{receipt_id}' не знайдено")

        items = await self._receipt_repo.find_items_with_products(receipt_id)
        result = []
        for item in items:
            result.append({
                "id": item.id,
                "product_id": item.product_id,
                "product_name": item.product.title if item.product else "",
                "product_barcode": item.product.barcode if item.product else None,
                "quantity": float(item.quantity),
                "price": float(item.price),
                "total": float(item.total),
                "purchase_price": item.purchase_price,
                "created_at": item.created_at,
            })
        return result


def _stock_as_float(stock) -> float | None:
    """Повертає float з ORM-поля stock або доменного Quantity."""
    if stock is None:
        return None
    if hasattr(stock, "value"):
        return float(stock.value)
    return float(stock)
