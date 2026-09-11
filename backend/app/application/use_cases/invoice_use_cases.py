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
from typing import TYPE_CHECKING, Optional
from uuid import UUID

from app.application.dto.invoice_dto import InvoiceConfirmDTO, InvoiceCreateDTO, InvoiceDTO
from app.application.interfaces.i_event_bus import IEventBus
from app.application.mappers.invoice_mapper import InvoiceMapper
from app.domain.entities.invoice import InvoiceStatus
from app.domain.events import (
    InvoiceApproved,
    InvoiceCreated,
    InvoiceUpdated,
)
from app.domain.repositories import IInvoiceRepository, IProductRepository, ISupplierRepository
from app.domain.repositories.i_unit_of_work import IUnitOfWork
from app.domain.value_objects.quantity import Quantity
from app.infrastructure.persistence.models.supplier_ledger import (
    LedgerOperationType,
    SupplierLedger,
)

if TYPE_CHECKING:
    from app.application.dto.invoice_dto import InvoiceUpdateDTO


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
                    # Фіскальна накладна: товар надходить у фіскальний залишок
                    if invoice.is_fiscal:
                        product.mark_as_fiscal()
                        product.update_fiscal_stock(item.quantity)
                    # Оновлюємо роздрібну ціну товару згідно з ціною в накладній
                    if item.price is not None:
                        product.change_price(item.price)
                    # Оновлюємо собівартість
                    if getattr(item, "cost_price", None) is not None:
                        product.change_cost_price(item.cost_price)
                    await self._product_repo.update(product)

            # Створюємо INVOICE-запис у журналі взаєморозрахунків (борг +)
            invoice_amount = getattr(invoice, "total_amount", None)
            if invoice_amount is None:
                invoice_amount = getattr(
                    getattr(invoice, "total", None), "amount", None
                )
            invoice_amount = (
                Decimal(str(invoice_amount))
                if invoice_amount is not None
                else Decimal("0")
            )
            current_balance = await self._uow.ledger.get_supplier_balance(
                invoice.supplier_id
            )
            ledger_entry = SupplierLedger(
                supplier_id=invoice.supplier_id,
                operation_type=LedgerOperationType.INVOICE,
                document_id=invoice.id,
                document_number=invoice.number,
                amount=invoice_amount,
                operation_date=(
                    getattr(invoice, "invoice_date", None) or invoice.created_at
                ),
                notes=f"Прибуткова накладна №{invoice.number}",
                balance_after=Decimal(str(current_balance)) + invoice_amount,
            )
            await self._uow.ledger.save(ledger_entry)

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
                    # Quantity не допускає від'ємних значень —
                    # обчислюємо нове значення через віднімання
                    if product.stock is not None:
                        product.stock = product.stock - item.quantity
                    # Відкат фіскального залишку (не нижче 0)
                    if invoice.is_fiscal and product.fiscal_stock is not None:
                        new_fiscal = max(
                            Decimal("0"),
                            product.fiscal_stock.value - item.quantity.value,
                        )
                        product.fiscal_stock = Quantity(
                            new_fiscal, product.fiscal_stock.unit
                        )
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
        invoice_status = InvoiceStatus(status).value if status else None

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

    # ─── Оновлення / видалення / звіти ─────────────────────────────────────

    async def update_invoice(
        self,
        invoice_id: UUID,
        dto: InvoiceUpdateDTO,
    ) -> InvoiceDTO:
        """
        Оновлює прибуткову накладну (тільки чернетку).

        Args:
            invoice_id: ID накладної.
            dto: DTO з полями для оновлення (items повністю замінюють позиції).

        Returns:
            InvoiceDTO оновленої накладної.

        Raises:
            ValueError: Якщо накладну не знайдено або вона не в статусі DRAFT.
        """
        invoice = await self._invoice_repo.find_by_id(invoice_id)
        if not invoice:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")
        _is_draft = getattr(invoice, "is_draft", True)
        if callable(_is_draft):
            _is_draft = _is_draft()
        if not _is_draft:
            raise ValueError("Можна редагувати тільки чернетки")

        # Оновлюємо скалярні поля
        if dto.number is not None:
            invoice.number = dto.number
        if dto.supplier_id is not None:
            invoice.supplier_id = dto.supplier_id
        if dto.notes is not None:
            invoice.notes = dto.notes
        if dto.is_fiscal is not None:
            invoice.is_fiscal = dto.is_fiscal
        if dto.invoice_date is not None:
            invoice.invoice_date = dto.invoice_date.replace(tzinfo=None)

        # Повністю замінюємо позиції (cascade="all, delete-orphan")
        if dto.items is not None:
            invoice.items.clear()
            for item_dto in dto.items:
                from app.infrastructure.persistence.models.invoice import (
                    InvoiceItem as ORMInvoiceItem,
                )
                quantity = float(item_dto.quantity)
                price = float(item_dto.price)
                invoice.items.append(ORMInvoiceItem(
                    product_id=item_dto.product_id,
                    quantity=quantity,
                    price=price,
                    total=quantity * price,
                ))
            invoice.total_amount = sum(
                float(i.quantity) * float(i.price) for i in invoice.items
            )

        async with self._uow:
            saved = await self._invoice_repo.update(invoice)
            await self._uow.commit()

        event = InvoiceUpdated(
            invoice_id=saved.id,
            changes={"updated": True},
        )
        await self._event_bus.publish(event)

        return InvoiceMapper.entity_to_dto(saved)

    async def delete_invoice(self, invoice_id: UUID) -> None:
        """
        Видаляє прибуткову накладну (тільки чернетку).

        Args:
            invoice_id: ID накладної.

        Raises:
            ValueError: Якщо накладну не знайдено або вона не в статусі DRAFT.
        """
        invoice = await self._invoice_repo.find_by_id(invoice_id)
        if not invoice:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")
        _is_draft = getattr(invoice, "is_draft", True)
        if callable(_is_draft):
            _is_draft = _is_draft()
        if not _is_draft:
            raise ValueError("Можна видалити тільки чернетку")

        async with self._uow:
            await self._invoice_repo.delete(invoice_id)
            await self._uow.commit()

    async def get_invoice_payment_info(self, invoice_id: UUID) -> dict:
        """
        Повертає інформацію про оплату накладної.

        Args:
            invoice_id: ID накладної.

        Returns:
            dict: {invoice_id, invoice_number, invoice_date, total_amount,
                   paid_amount, remaining}.

        Raises:
            ValueError: Якщо накладну не знайдено.
        """
        return await self._invoice_repo.get_payment_info(invoice_id)

    async def get_invoice_price_changes(self, invoice_id: UUID) -> list[dict]:
        """
        Повертає список товарів з накладної з інформацією про зміну цін.

        Для кожного товару: product_id, title, barcode, article,
        invoice_price, current_price, changed, difference.

        Args:
            invoice_id: ID накладної.

        Returns:
            list[dict] зі змінами цін.

        Raises:
            ValueError: Якщо накладну не знайдено.
        """
        from decimal import Decimal

        def _dec(value) -> Decimal:
            """Decimal з Money/float/None (quantize 0.01)."""
            if value is None:
                return Decimal("0.00")
            if hasattr(value, "amount"):
                value = value.amount
            return Decimal(str(value)).quantize(Decimal("0.01"))

        invoice = await self._invoice_repo.find_by_id(invoice_id)
        if not invoice:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")

        changes: list[dict] = []
        for item in invoice.items:
            product = item.product
            if not product:
                continue

            invoice_price = item.price or 0
            # previous_price — ціна товару ДО накладної
            prev_price = item.previous_price or product.price or 0
            current_price = product.price or 0

            prev_price_dec = _dec(prev_price)
            invoice_price_dec = _dec(invoice_price)
            current_price_dec = _dec(current_price)
            difference = (prev_price_dec - invoice_price_dec).quantize(Decimal("0.01"))

            _title = getattr(product, "title", None) or getattr(product, "name", "") or ""
            _barcode = getattr(product, "barcode", None)
            if hasattr(_barcode, "value"):
                _barcode = str(_barcode.value)
            changes.append({
                "product_id": product.id,
                "title": _title,
                "barcode": _barcode,
                "article": getattr(product, "sku", "") or "",
                "invoice_price": str(invoice_price_dec),
                "current_price": str(current_price_dec),
                "changed": difference != Decimal("0.00"),
                "difference": str(difference),
            })

        return changes
