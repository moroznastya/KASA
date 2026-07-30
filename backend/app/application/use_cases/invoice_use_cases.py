"""
Use Cases для Invoice (Прибуткова накладна).

Реалізує бізнес-логіку для роботи з прибутковими накладними:
- CreateInvoice: створення нової накладної
- ConfirmInvoice: підтвердження накладної (оновлює залишки)
- CancelInvoice: скасування накладної
- GetInvoices: отримання списку накладних з фільтрацією
"""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from app.domain.entities.invoice import Invoice, InvoiceStatus
from app.domain.repositories import IInvoiceRepository, IProductRepository, ISupplierRepository
from app.domain.repositories.i_unit_of_work import IUnitOfWork
from app.application.dto.invoice_dto import InvoiceDTO, InvoiceCreateDTO, InvoiceConfirmDTO
from app.application.mappers.invoice_mapper import InvoiceMapper
from app.application.interfaces.i_event_bus import IEventBus
from app.domain.events import (
    InvoiceCreated,
    InvoiceUpdated,
    InvoiceApproved,
)


class InvoiceUseCases:
    """
    Use Cases для прибуткових накладних.

    Використовує Dependency Injection через конструктор.
    Залежності: IInvoiceRepository, IProductRepository, ISupplierRepository,
                IUnitOfWork, IEventBus.
    """

    def __init__(
        self,
        invoice_repo: IInvoiceRepository,
        product_repo: IProductRepository,
        supplier_repo: ISupplierRepository,
        unit_of_work: IUnitOfWork,
        event_bus: IEventBus,
    ):
        """
        Ініціалізація Use Cases.

        Args:
            invoice_repo: Репозиторій накладних.
            product_repo: Репозиторій товарів.
            supplier_repo: Репозиторій постачальників.
            unit_of_work: Unit of Work для транзакцій.
            event_bus: Event Bus для публікації подій.
        """
        self._invoice_repo = invoice_repo
        self._product_repo = product_repo
        self._supplier_repo = supplier_repo
        self._uow = unit_of_work
        self._event_bus = event_bus

    async def create_invoice(self, dto: InvoiceCreateDTO) -> InvoiceDTO:
        """
        Створює нову прибуткову накладну.

        Args:
            dto: DTO з даними для створення накладної.

        Returns:
            InvoiceDTO створеної накладної.

        Raises:
            ValueError: Якщо постачальника не знайдено або номер вже існує.
        """
        # Перевіряємо існування постачальника
        supplier = await self._supplier_repo.find_by_id(dto.supplier_id)
        if not supplier:
            raise ValueError(f"Постачальника з ID '{dto.supplier_id}' не знайдено")

        # Перевіряємо унікальність номера
        existing = await self._invoice_repo.find_by_number(dto.number)
        if existing:
            raise ValueError(f"Накладна з номером '{dto.number}' вже існує")

        # Конвертуємо DTO в Entity
        invoice = InvoiceMapper.create_dto_to_entity(dto)

        # Зберігаємо через репозиторій
        async with self._uow:
            saved = await self._invoice_repo.save(invoice)
            await self._uow.commit()

        # Публікуємо подію InvoiceCreated
        event = InvoiceCreated(
            invoice_id=saved.id,
            supplier_id=saved.supplier_id,
            total_amount=saved.total or Decimal("0"),
            status=saved.status.value if hasattr(saved.status, 'value') else str(saved.status),
        )
        await self._event_bus.publish(event)

        return InvoiceMapper.entity_to_dto(saved)

    async def confirm_invoice(self, dto: InvoiceConfirmDTO) -> InvoiceDTO:
        """
        Підтверджує прибуткову накладну.

        При підтвердженні:
        - Змінює статус на CONFIRMED
        - Оновлює залишки товарів на складі
        - Оновлює баланс постачальника

        Args:
            dto: DTO з ID накладної для підтвердження.

        Returns:
            InvoiceDTO підтвердженої накладної.

        Raises:
            ValueError: Якщо накладну не знайдено або вона не в статусі DRAFT.
        """
        # Знаходимо накладну
        invoice = await self._invoice_repo.find_by_id(dto.invoice_id)
        if not invoice:
            raise ValueError(f"Накладну з ID '{dto.invoice_id}' не знайдено")

        # Підтверджуємо накладну (доменна логіка)
        invoice.confirm()

        async with self._uow:
            # Оновлюємо залишки товарів
            for item in invoice.items:
                product = await self._product_repo.find_by_id(item.product_id)
                if product:
                    product.update_stock(item.quantity)
                    await self._product_repo.update(product)

            # Оновлюємо баланс постачальника
            supplier = await self._supplier_repo.find_by_id(invoice.supplier_id)
            if supplier and invoice.total:
                supplier.update_balance(invoice.total)
                await self._supplier_repo.update(supplier)

            # Зберігаємо накладну
            saved = await self._invoice_repo.update(invoice)
            await self._uow.commit()

        # Публікуємо подію InvoiceApproved
        event = InvoiceApproved(
            invoice_id=saved.id,
            items_count=len(saved.items),
        )
        await self._event_bus.publish(event)

        return InvoiceMapper.entity_to_dto(saved)

    async def cancel_invoice(self, invoice_id: UUID) -> InvoiceDTO:
        """
        Скасовує прибуткову накладну.

        При скасуванні:
        - Змінює статус на CANCELLED
        - Відкочує залишки товарів
        - Відкочує баланс постачальника

        Args:
            invoice_id: ID накладної для скасування.

        Returns:
            InvoiceDTO скасованої накладної.

        Raises:
            ValueError: Якщо накладну не знайдено або вона не в статусі CONFIRMED.
        """
        # Знаходимо накладну
        invoice = await self._invoice_repo.find_by_id(invoice_id)
        if not invoice:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")

        # Скасовуємо накладну (доменна логіка)
        invoice.cancel()

        async with self._uow:
            # Відкочуємо залишки товарів
            for item in invoice.items:
                product = await self._product_repo.find_by_id(item.product_id)
                if product:
                    # Від'ємна кількість для зменшення залишку
                    product.update_stock(item.quantity * -1)
                    await self._product_repo.update(product)

            # Відкочуємо баланс постачальника
            supplier = await self._supplier_repo.find_by_id(invoice.supplier_id)
            if supplier and invoice.total:
                supplier.reduce_balance(invoice.total)
                await self._supplier_repo.update(supplier)

            # Зберігаємо накладну
            saved = await self._invoice_repo.update(invoice)
            await self._uow.commit()

        # Публікуємо подію InvoiceUpdated (скасування)
        event = InvoiceUpdated(
            invoice_id=saved.id,
            changes={"status": ("CONFIRMED", "CANCELLED")},
        )
        await self._event_bus.publish(event)

        return InvoiceMapper.entity_to_dto(saved)

    async def get_invoice(self, invoice_id: UUID) -> InvoiceDTO:
        """
        Отримує накладну за ID.

        Args:
            invoice_id: ID накладної.

        Returns:
            InvoiceDTO накладної.

        Raises:
            ValueError: Якщо накладну не знайдено.
        """
        invoice = await self._invoice_repo.find_by_id(invoice_id)
        if not invoice:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")
        return InvoiceMapper.entity_to_dto(invoice)

    async def get_invoices(
        self,
        query: Optional[str] = None,
        supplier_id: Optional[UUID] = None,
        status: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[InvoiceDTO], int]:
        """
        Отримує список накладних з фільтрацією та пагінацією.

        Args:
            query: Текстовий пошук.
            supplier_id: Фільтр за постачальником.
            status: Фільтр за статусом.
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список InvoiceDTO, загальна кількість).
        """
        invoice_status = InvoiceStatus(status) if status else None

        invoices, total = await self._invoice_repo.search(
            query=query,
            supplier_id=supplier_id,
            status=invoice_status,
            date_from=date_from,
            date_to=date_to,
            page=page,
            size=size,
        )
        return [InvoiceMapper.entity_to_dto(inv) for inv in invoices], total
