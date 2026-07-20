"""
Repository Interface: ILedgerRepository.

Визначає контракт для роботи з журналом взаєморозрахунків.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from datetime import datetime
from typing import Optional, Protocol
from uuid import UUID

from ..entities.ledger_entry import LedgerEntry, OperationType


class ILedgerRepository(Protocol):
    """
    Інтерфейс репозиторію журналу взаєморозрахунків.
    """

    async def save(self, entry: LedgerEntry) -> LedgerEntry:
        """
        Зберігає новий запис у журналі.

        Args:
            entry: Entity запису.

        Returns:
            Збережений запис.
        """
        ...

    async def find_by_id(self, entry_id: UUID) -> Optional[LedgerEntry]:
        """
        Знаходить запис за ID.

        Args:
            entry_id: UUID запису.

        Returns:
            LedgerEntry або None.
        """
        ...

    async def find_by_supplier(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        """
        Знаходить записи для постачальника.

        Args:
            supplier_id: UUID постачальника.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список записів, загальна кількість).
        """
        ...

    async def find_by_operation_type(
        self,
        operation_type: OperationType,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        """
        Знаходить записи за типом операції.

        Args:
            operation_type: Тип операції.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список записів, загальна кількість).
        """
        ...

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        """
        Знаходить записи за діапазоном дат.

        Args:
            date_from: Початкова дата.
            date_to: Кінцева дата.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список записів, загальна кількість).
        """
        ...

    async def search(
        self,
        supplier_id: Optional[UUID] = None,
        operation_type: Optional[OperationType] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        """
        Розширений пошук записів.

        Args:
            supplier_id: Фільтр за постачальником.
            operation_type: Фільтр за типом операції.
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список записів, загальна кількість).
        """
        ...

    async def get_supplier_balance(self, supplier_id: UUID) -> float:
        """
        Повертає поточний баланс постачальника.

        Args:
            supplier_id: UUID постачальника.

        Returns:
            Поточний баланс.
        """
        ...

    async def get_all_supplier_balances(self) -> list[dict]:
        """
        Повертає баланси всіх постачальників.

        Returns:
            Список словників {supplier_id, supplier_name, balance, last_operation_date}.
        """
        ...

    async def delete(self, entry_id: UUID) -> None:
        """
        Видаляє запис за ID.

        Args:
            entry_id: UUID запису.
        """
        ...

    async def count(self) -> int:
        """
        Повертає загальну кількість записів.

        Returns:
            Кількість записів.
        """
        ...
