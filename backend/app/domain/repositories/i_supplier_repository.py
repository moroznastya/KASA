"""
Repository Interface: ISupplierRepository.

Визначає контракт для роботи з постачальниками.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from typing import Optional, Protocol
from uuid import UUID

from ..entities.supplier import Supplier


class ISupplierRepository(Protocol):
    """
    Інтерфейс репозиторію постачальників.
    """

    async def save(self, supplier: Supplier) -> Supplier:
        """
        Зберігає нового постачальника.

        Args:
            supplier: Entity постачальника.

        Returns:
            Збережений постачальник.
        """
        ...

    async def update(self, supplier: Supplier) -> Supplier:
        """
        Оновлює існуючого постачальника.

        Args:
            supplier: Entity постачальника з оновленими даними.

        Returns:
            Оновлений постачальник.
        """
        ...

    async def find_by_id(self, supplier_id: UUID) -> Optional[Supplier]:
        """
        Знаходить постачальника за ID.

        Args:
            supplier_id: UUID постачальника.

        Returns:
            Supplier або None.
        """
        ...

    async def find_by_name(self, name: str) -> Optional[Supplier]:
        """
        Знаходить постачальника за назвою.

        Args:
            name: Назва постачальника.

        Returns:
            Supplier або None.
        """
        ...

    async def find_by_edrpou(self, edrpou: str) -> Optional[Supplier]:
        """
        Знаходить постачальника за кодом ЄДРПОУ.

        Args:
            edrpou: Код ЄДРПОУ.

        Returns:
            Supplier або None.
        """
        ...

    async def search(
        self,
        query: Optional[str] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Supplier], int]:
        """
        Пошук постачальників.

        Args:
            query: Текстовий пошук (назва, ЄДРПОУ, телефон).
            is_active: Фільтр за активністю.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список постачальників, загальна кількість).
        """
        ...

    async def delete(self, supplier_id: UUID) -> None:
        """
        Видаляє постачальника за ID.

        Args:
            supplier_id: UUID постачальника.
        """
        ...

    async def count(self) -> int:
        """
        Повертає загальну кількість постачальників.

        Returns:
            Кількість постачальників.
        """
        ...

    async def get_all_with_balance(self) -> list[Supplier]:
        """
        Повертає всіх постачальників з балансом.

        Returns:
            Список постачальників.
        """
        ...
