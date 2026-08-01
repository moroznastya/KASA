"""
Repository Interface: IReceiptRepository.

Визначає контракт для роботи з чеками продажу.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from datetime import datetime
from typing import Optional, Protocol
from uuid import UUID

from ..entities.receipt import Receipt


class IReceiptRepository(Protocol):
    """
    Інтерфейс репозиторію чеків продажу.
    """

    async def save(self, receipt: Receipt) -> Receipt:
        """
        Зберігає новий чек.

        Args:
            receipt: Entity чеку.

        Returns:
            Збережений чек.
        """
        ...

    async def find_by_id(self, receipt_id: UUID) -> Optional[Receipt]:
        """
        Знаходить чек за ID.

        Args:
            receipt_id: UUID чеку.

        Returns:
            Receipt або None.
        """
        ...

    async def find_by_number(self, number: str) -> Optional[Receipt]:
        """
        Знаходить чек за номером.

        Args:
            number: Номер чеку.

        Returns:
            Receipt або None.
        """
        ...

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        """
        Знаходить чеки за діапазоном дат.

        Args:
            date_from: Початкова дата.
            date_to: Кінцева дата.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список чеків, загальна кількість).
        """
        ...

    async def search(
        self,
        query: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        payment_method: Optional[str] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        """
        Розширений пошук чеків.

        Args:
            query: Текстовий пошук.
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            payment_method: Фільтр за способом оплати.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список чеків, загальна кількість).
        """
        ...

    async def delete(self, receipt_id: UUID) -> None:
        """
        Видаляє чек за ID.

        Args:
            receipt_id: UUID чеку.
        """
        ...

    async def count(self) -> int:
        """
        Повертає загальну кількість чеків.

        Returns:
            Кількість чеків.
        """
        ...

    async def get_daily_total(self, date: datetime) -> float:
        """
        Повертає загальну суму продажів за день.

        Args:
            date: Дата.

        Returns:
            Загальна сума.
        """
        ...

    async def get_today_stats(self) -> dict:
        """
        Повертає статистику чеків за сьогодні (UTC).

        Returns:
            dict: {total_sales, total_returns, total_profit, total_vat,
                   receipts_count, items_sold, date}
        """
        ...

    async def search_with_details(
        self,
        q: str = "",
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        receipt_type=None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list, int]:
        """
        Пошук чеків для повернень (за номером або назвою товару).

        Args:
            q: Пошуковий запит (номер чеку або назва товару).
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            receipt_type: Тип чеку (sale/return).
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список чеків, загальна кількість).
        """
        ...

    async def find_recent_sales_by_product(
        self,
        query: str,
        limit: int = 5,
    ) -> list[dict]:
        """
        Останні продажі товарів за штрих-кодом або назвою.

        Args:
            query: Штрих-код або назва товару.
            limit: Кількість останніх продажів.

        Returns:
            list[dict]: [{product, total_sold, total_returned,
                          returnable, recent_sales}]
        """
        ...

    async def get_sold_returned_totals(
        self,
        product_id: UUID,
    ) -> tuple:
        """
        Повертає (total_sold, total_returned) для товару.

        Args:
            product_id: UUID товару.

        Returns:
            Кортеж (продано, повернуто) у Decimal.
        """
        ...

    async def get_returnable_quantity(self, product_id: UUID):
        """
        Скільки одиниць товару ще можна повернути.

        Args:
            product_id: UUID товару.

        Returns:
            Decimal: max(0, продано - повернуто).
        """
        ...

    async def find_items_with_products(self, receipt_id: UUID) -> list:
        """
        Знаходить позиції чеку з підвантаженими товарами.

        Args:
            receipt_id: ID чеку.

        Returns:
            Список позицій (з .product).
        """
        ...
