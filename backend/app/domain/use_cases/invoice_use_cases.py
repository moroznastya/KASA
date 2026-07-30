"""
Use Cases: Invoice (Прибуткові накладні).

Кожен Use Case виконує одну бізнес-операцію:
- CreateInvoiceUseCase: створення накладної
- ConfirmInvoiceUseCase: підтвердження накладної (оприбуткування товарів)
- CancelInvoiceUseCase: скасування накладної

Валідація виконується всередині Use Case, а не в сервісах чи репозиторіях.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from typing import Optional
from uuid import UUID

from app.domain.entities.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.domain.repositories import IInvoiceRepository, IProductRepository, IUnitOfWork
from app.domain.services.stock_service import StockService
from app.domain.services.document_service import DocumentService
from app.domain.services.ledger_service import LedgerService
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate


@dataclass
class InvoiceItemCreate:
    """Вхідні дані для створення позиції накладної."""

    product_id: UUID
    quantity: Decimal
    price: Decimal
    tax_rate_percent: int = 20  # Ставка ПДВ у відсотках


class CreateInvoiceUseCase:
    """
    Створення нової прибуткової накладної.

    Валідація:
    - Список позицій не може бути пустим
    - Кожен товар має існувати
    - Кількість та ціна мають бути додатними
    """

    def __init__(
        self,
        invoice_repo: IInvoiceRepository,
        product_repo: IProductRepository,
        stock_service: StockService,
        document_service: DocumentService,
        uow: IUnitOfWork,
    ) -> None:
        self._invoice_repo = invoice_repo
        self._product_repo = product_repo
        self._stock_service = stock_service
        self._document_service = document_service
        self._uow = uow

    async def execute(
        self,
        supplier_id: UUID,
        items: list[InvoiceItemCreate],
        number: str = "",
        notes: str = "",
    ) -> Invoice:
        # Валідація: список позицій не пустий
        if not items:
            raise ValueError("Накладна повинна мати хоча б одну позицію")

        # Валідація: кожна позиція
        invoice_items: list[InvoiceItem] = []
        for i, item in enumerate(items):
            product = await self._product_repo.find_by_id(item.product_id)
            if product is None:
                raise ValueError(
                    f"Товар з ID '{item.product_id}' (позиція {i + 1}) не знайдено"
                )

            if item.quantity <= Decimal("0"):
                raise ValueError(
                    f"Кількість товару (позиція {i + 1}) повинна бути додатною, "
                    f"отримано {item.quantity}"
                )

            if item.price <= Decimal("0"):
                raise ValueError(
                    f"Ціна товару (позиція {i + 1}) повинна бути додатною, "
                    f"отримано {item.price}"
                )

            tax_rate = TaxRate.from_percent(item.tax_rate_percent)
            invoice_item = InvoiceItem(
                product_id=item.product_id,
                quantity=Quantity(item.quantity, product.unit),
                price=Money(item.price),
                tax_rate=tax_rate,
                name=product.name,
            )
            invoice_items.append(invoice_item)

        invoice = Invoice(
            number=number,
            supplier_id=supplier_id,
            items=invoice_items,
            notes=notes,
        )

        saved = await self._invoice_repo.save(invoice)
        await self._uow.commit()
        return saved


class ConfirmInvoiceUseCase:
    """
    Підтвердження прибуткової накладної.

    При підтвердженні:
    1. Змінює статус на CONFIRMED (через доменну логіку)
    2. Оновлює залишки товарів (збільшення)
    3. Створює записи в журналі взаєморозрахунків

    Валідація:
    - Накладна має існувати
    - Накладна має бути в статусі DRAFT
    - Накладна повинна мати позиції
    """

    def __init__(
        self,
        invoice_repo: IInvoiceRepository,
        product_repo: IProductRepository,
        stock_service: StockService,
        ledger_service: LedgerService,
        uow: IUnitOfWork,
    ) -> None:
        self._invoice_repo = invoice_repo
        self._product_repo = product_repo
        self._stock_service = stock_service
        self._ledger_service = ledger_service
        self._uow = uow

    async def execute(self, invoice_id: UUID) -> Invoice:
        invoice = await self._invoice_repo.find_by_id(invoice_id)
        if invoice is None:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")

        if invoice.status != InvoiceStatus.DRAFT:
            raise ValueError(
                f"Неможливо підтвердити накладну в статусі '{invoice.status.value}'. "
                f"Очікується статус 'draft'"
            )

        if not invoice.items:
            raise ValueError("Неможливо підтвердити накладну без позицій")

        # Доменна логіка — зміна статусу
        invoice.confirm()

        # Оновлення залишків
        for item in invoice.items:
            product = await self._product_repo.find_by_id(item.product_id)
            if product:
                product.update_stock(item.quantity)
                await self._product_repo.update(product)

        updated = await self._invoice_repo.update(invoice)
        await self._uow.commit()
        return updated


class CancelInvoiceUseCase:
    """
    Скасування прибуткової накладної.

    При скасуванні:
    1. Змінює статус на CANCELLED (через доменну логіку)
    2. Відкатує залишки товарів (зменшення)

    Валідація:
    - Накладна має існувати
    - Накладна має бути в статусі CONFIRMED
    """

    def __init__(
        self,
        invoice_repo: IInvoiceRepository,
        product_repo: IProductRepository,
        stock_service: StockService,
        ledger_service: LedgerService,
        uow: IUnitOfWork,
    ) -> None:
        self._invoice_repo = invoice_repo
        self._product_repo = product_repo
        self._stock_service = stock_service
        self._ledger_service = ledger_service
        self._uow = uow

    async def execute(self, invoice_id: UUID) -> Invoice:
        invoice = await self._invoice_repo.find_by_id(invoice_id)
        if invoice is None:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")

        if invoice.status != InvoiceStatus.CONFIRMED:
            raise ValueError(
                f"Неможливо скасувати накладну в статусі '{invoice.status.value}'. "
                f"Скасувати можна лише підтверджену накладну"
            )

        # Доменна логіка — зміна статусу
        invoice.cancel()

        # Відкат залишків: поточний stock - item.quantity
        for item in invoice.items:
            product = await self._product_repo.find_by_id(item.product_id)
            if product and product.stock is not None:
                new_value = product.stock.value - item.quantity.value
                product.stock = Quantity(new_value, product.stock.unit)
                await self._product_repo.update(product)

        updated = await self._invoice_repo.update(invoice)
        await self._uow.commit()
        return updated
