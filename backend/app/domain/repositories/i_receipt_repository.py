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
