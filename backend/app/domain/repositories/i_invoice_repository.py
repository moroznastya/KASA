"""
Repository Interface: IInvoiceRepository.

Визначає контракт для роботи з прибутковими накладними.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from datetime import datetime
from typing import Optional, Protocol
from uuid import UUID

from ..entities.invoice import Invoice, InvoiceStatus


class IInvoiceRepository(Protocol):
    """
    Інтерфейс репозиторію прибуткових накладних.
    """

    async def save(self, invoice: Invoice) -> Invoice:
        """
        Зберігає нову накладну.

        Args:
            invoice: Entity накладної.

        Returns:
            Збережена накладна.
        """
        ...

    async def update(self, invoice: Invoice) -> Invoice:
        """
        Оновлює існуючу накладну.

        Args:
            invoice: Entity накладної з оновленими даними.

        Returns:
            Оновлена накладна.
        """
        ...

    async def find_by_id(self, invoice_id: UUID) -> Optional[Invoice]:
        """
        Знаходить накладну за ID.

        Args:
            invoice_id: UUID накладної.

        Returns:
            Invoice або None.
        """
        ...

    async def find_by_number(self, number: str) -> Optional[Invoice]:
        """
        Знаходить накладну за номером.

        Args:
            number: Номер накладної.

        Returns:
            Invoice або None.
        """
        ...

    async def find_by_supplier(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        """
        Знаходить накладні постачальника.

        Args:
            supplier_id: UUID постачальника.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список накладних, загальна кількість).
        """
        ...

    async def find_by_status(
        self,
        status: InvoiceStatus,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        """
        Знаходить накладні за статусом.

        Args:
            status: Статус накладної.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список накладних, загальна кількість).
        """
        ...

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        """
        Знаходить накладні за діапазоном дат.

        Args:
            date_from: Початкова дата.
            date_to: Кінцева дата.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список накладних, загальна кількість).
        """
        ...

    async def search(
        self,
        query: Optional[str] = None,
        supplier_id: Optional[UUID] = None,
        status: Optional[InvoiceStatus] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        """
        Розширений пошук накладних.

        Args:
            query: Текстовий пошук (номер, нотатки).
            supplier_id: Фільтр за постачальником.
            status: Фільтр за статусом.
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список накладних, загальна кількість).
        """
        ...

    async def delete(self, invoice_id: UUID) -> None:
        """
        Видаляє накладну за ID.

        Args:
            invoice_id: UUID накладної.
        """
        ...

    async def count(self) -> int:
        """
        Повертає загальну кількість накладних.

        Returns:
            Кількість накладних.
        """
        ...
