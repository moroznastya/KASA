"""Unit tests: fiscal_stock — оприбуткування фіскальних накладних (2.1).

Покриває:
  - Application InvoiceUseCases.confirm_invoice: фіскальна накладна
    збільшує Product.fiscal_stock та позначає товар is_fiscal=True;
  - Application InvoiceUseCases.cancel_invoice: скасування фіскальної
    накладної зменшує fiscal_stock (не нижче 0);
  - DocumentService.confirm_return_invoice: повернення постачальнику
    з фіскального документа зменшує fiscal_stock;
  - DocumentService.cancel_return_invoice: відкат повернення повертає
    товар у fiscal_stock.
"""

from __future__ import annotations

from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.application.dto.invoice_dto import InvoiceConfirmDTO
from app.application.use_cases.invoice_use_cases import InvoiceUseCases
from app.domain.entities.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.domain.entities.product import Product
from app.domain.services.document_service import DocumentService
from app.infrastructure.persistence.models.supplier import Supplier
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate
from app.domain.value_objects.quantity import Quantity as _Q
Quantity = _Q
from app.infrastructure.persistence.models.product import Product as ProductModel
from app.infrastructure.persistence.models.invoice import (
    Invoice as InvoiceModel,
    InvoiceItem as InvoiceItemModel,
    InvoiceStatus as InvoiceStatusModel,
    PaymentMethod,
)
from app.infrastructure.persistence.models.return_invoice import (
    ReturnInvoice as ReturnInvoiceModel,
    ReturnInvoiceItem as ReturnInvoiceItemModel,
    ReturnInvoiceStatus,
    ReturnActionType,
)
from app.infrastructure.persistence.models.user import User, UserRole


# ─── Application InvoiceUseCases ─────────────────────────────────────────────

def _make_uow() -> MagicMock:
    """UoW як асинхронний контекстний менеджер."""
    uow = MagicMock()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        return False

    uow.__aenter__ = __aenter__
    uow.__aexit__ = __aexit__
    uow.commit = AsyncMock()
    return uow


def _make_invoice_use_cases(*, product: Product, invoice: Invoice) -> InvoiceUseCases:
    """InvoiceUseCases з моками (domain entities)."""
    invoice_repo = AsyncMock()
    invoice_repo.find_by_id.return_value = invoice
    invoice_repo.update.return_value = invoice
    product_repo = AsyncMock()
    product_repo.find_by_id.return_value = product
    product_repo.update = AsyncMock()
    supplier_repo = AsyncMock()
    supplier_repo.find_by_id.return_value = MagicMock()
    supplier_repo.update = AsyncMock()
    return InvoiceUseCases(
        invoice_repo=invoice_repo,
        product_repo=product_repo,
        supplier_repo=supplier_repo,
        unit_of_work=_make_uow(),
        event_bus=AsyncMock(),
    )


def _invoice_item(product_id, qty: str) -> InvoiceItem:
    return InvoiceItem(
        product_id=product_id,
        quantity=Quantity(Decimal(qty)),
        price=Money(Decimal("100")),
        tax_rate=TaxRate.standard(),
        name="Товар",
    )


class TestApplicationInvoiceFiscalStock:
    """Application InvoiceUseCases: fiscal_stock при підтвердженні/скасуванні."""

    async def test_confirm_fiscal_invoice_increases_fiscal_stock(self):
        """Підтвердження фіскальної накладної: fiscal_stock += qty, is_fiscal=True."""
        product_id = uuid4()
        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("5")),
            unit="шт",
            is_fiscal=False,
            fiscal_stock=None,
        )
        invoice = Invoice(
            id=uuid4(),
            number="ПН-001",
            supplier_id=uuid4(),
            items=[_invoice_item(product_id, "10")],
            status=InvoiceStatus.DRAFT,
            is_fiscal=True,
        )
        use_cases = _make_invoice_use_cases(product=product, invoice=invoice)

        result = await use_cases.confirm_invoice(InvoiceConfirmDTO(invoice_id=invoice.id))

        assert result.status == "confirmed"
        assert product.is_fiscal is True
        assert product.fiscal_stock == Quantity(Decimal("10"))

    async def test_confirm_non_fiscal_invoice_keeps_fiscal_stock(self):
        """Звичайна накладна НЕ змінює fiscal_stock."""
        product_id = uuid4()
        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("5")),
            unit="шт",
            is_fiscal=False,
            fiscal_stock=None,
        )
        invoice = Invoice(
            id=uuid4(),
            number="ПН-002",
            supplier_id=uuid4(),
            items=[_invoice_item(product_id, "10")],
            status=InvoiceStatus.DRAFT,
            is_fiscal=False,
        )
        use_cases = _make_invoice_use_cases(product=product, invoice=invoice)

        await use_cases.confirm_invoice(InvoiceConfirmDTO(invoice_id=invoice.id))

        assert product.is_fiscal is False
        assert product.fiscal_stock is None
        # Звичайний залишок збільшився
        assert product.stock == Quantity(Decimal("15"))

    async def test_cancel_fiscal_invoice_decreases_fiscal_stock(self):
        """Скасування фіскальної накладної: fiscal_stock -= qty (не нижче 0)."""
        product_id = uuid4()
        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("15")),
            unit="шт",
            is_fiscal=True,
            fiscal_stock=Quantity(Decimal("10")),
        )
        invoice = Invoice(
            id=uuid4(),
            number="ПН-003",
            supplier_id=uuid4(),
            items=[_invoice_item(product_id, "10")],
            status=InvoiceStatus.CONFIRMED,
            is_fiscal=True,
        )
        use_cases = _make_invoice_use_cases(product=product, invoice=invoice)

        result = await use_cases.cancel_invoice(invoice.id)

        assert result.status == "cancelled"
        assert product.fiscal_stock == Quantity(Decimal("0"))

    async def test_cancel_fiscal_invoice_clamps_at_zero(self):
        """Скасування: fiscal_stock не опускається нижче 0."""
        product_id = uuid4()
        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("15")),
            unit="шт",
            is_fiscal=True,
            fiscal_stock=Quantity(Decimal("3")),  # менше за qty накладної
        )
        invoice = Invoice(
            id=uuid4(),
            number="ПН-004",
            supplier_id=uuid4(),
            items=[_invoice_item(product_id, "10")],
            status=InvoiceStatus.CONFIRMED,
            is_fiscal=True,
        )
        use_cases = _make_invoice_use_cases(product=product, invoice=invoice)

        await use_cases.cancel_invoice(invoice.id)

        assert product.fiscal_stock == Quantity(Decimal("0"))


# ─── DocumentService (повернення постачальнику) ──────────────────────────────

@pytest.mark.asyncio
async def _create_cashier(session: AsyncSession) -> User:
    cashier = User(
        id=uuid4(),
        name="Касир",
        login=f"cashier-{uuid4().hex[:8]}",
        password_hash="hash",
        pin_code="0000",
        role=UserRole.CASHIER,
        is_active=True,
    )
    session.add(cashier)
    return cashier


@pytest.mark.asyncio
async def _create_supplier(session: AsyncSession) -> Supplier:
    supplier = Supplier(id=uuid4(), name="Постачальник Тест")
    session.add(supplier)
    return supplier


@pytest.mark.asyncio
async def _create_product(session: AsyncSession, *, fiscal_stock) -> ProductModel:
    product = ProductModel(
        id=uuid4(),
        title="Товар",
        price=Decimal("100.00"),
        stock=Decimal("50"),
        fiscal_stock=fiscal_stock,
        is_fiscal=True,
        tax_rate=Decimal("20.00"),
        unit="шт",
    )
    session.add(product)
    return product


class TestDocumentServiceReturnInvoiceFiscalStock:
    async def test_confirm_fiscal_return_invoice_decreases_fiscal_stock(
        self, session: AsyncSession
    ):
        """Повернення постачальнику (is_fiscal=True): fiscal_stock -= qty."""
        cashier = await _create_cashier(session)
        product = await _create_product(session, fiscal_stock=Decimal("10"))
        supplier = await _create_supplier(session)
        await session.flush()

        return_invoice = ReturnInvoiceModel(
            id=uuid4(),
            number="ПВ-001",
            supplier_id=supplier.id,
            return_date=__import__("datetime").datetime.utcnow(),
            status=ReturnInvoiceStatus.DRAFT,
            return_action=ReturnActionType.DEDUCT_FROM_DEBT,
            is_fiscal=True,
            total_amount=Decimal("200.00"),
            created_by_id=cashier.id,
        )
        session.add(return_invoice)
        await session.flush()
        session.add(ReturnInvoiceItemModel(
            id=uuid4(),
            return_invoice_id=return_invoice.id,
            product_id=product.id,
            quantity=Decimal("2"),
            price=Decimal("100.00"),
            total=Decimal("200.00"),
        ))
        await session.flush()

        service = DocumentService(session)
        await service.confirm_return_invoice(return_invoice.id)

        await session.refresh(product)
        assert float(product.fiscal_stock) == 8.0

    async def test_confirm_non_fiscal_return_invoice_keeps_fiscal_stock(
        self, session: AsyncSession
    ):
        """Повернення (is_fiscal=False): fiscal_stock не змінюється."""
        cashier = await _create_cashier(session)
        product = await _create_product(session, fiscal_stock=Decimal("10"))
        supplier = await _create_supplier(session)
        await session.flush()

        return_invoice = ReturnInvoiceModel(
            id=uuid4(),
            number="ПВ-002",
            supplier_id=supplier.id,
            return_date=__import__("datetime").datetime.utcnow(),
            status=ReturnInvoiceStatus.DRAFT,
            return_action=ReturnActionType.DEDUCT_FROM_DEBT,
            is_fiscal=False,
            total_amount=Decimal("200.00"),
            created_by_id=cashier.id,
        )
        session.add(return_invoice)
        await session.flush()
        session.add(ReturnInvoiceItemModel(
            id=uuid4(),
            return_invoice_id=return_invoice.id,
            product_id=product.id,
            quantity=Decimal("2"),
            price=Decimal("100.00"),
            total=Decimal("200.00"),
        ))
        await session.flush()

        service = DocumentService(session)
        await service.confirm_return_invoice(return_invoice.id)

        await session.refresh(product)
        assert float(product.fiscal_stock) == 10.0

    async def test_cancel_fiscal_return_invoice_restores_fiscal_stock(
        self, session: AsyncSession
    ):
        """Скасування фіскального повернення: fiscal_stock += qty (відкат)."""
        cashier = await _create_cashier(session)
        product = await _create_product(session, fiscal_stock=Decimal("8"))
        supplier = await _create_supplier(session)
        await session.flush()

        return_invoice = ReturnInvoiceModel(
            id=uuid4(),
            number="ПВ-003",
            supplier_id=supplier.id,
            return_date=__import__("datetime").datetime.utcnow(),
            status=ReturnInvoiceStatus.CONFIRMED,
            return_action=ReturnActionType.DEDUCT_FROM_DEBT,
            is_fiscal=True,
            total_amount=Decimal("200.00"),
            created_by_id=cashier.id,
        )
        session.add(return_invoice)
        await session.flush()
        session.add(ReturnInvoiceItemModel(
            id=uuid4(),
            return_invoice_id=return_invoice.id,
            product_id=product.id,
            quantity=Decimal("2"),
            price=Decimal("100.00"),
            total=Decimal("200.00"),
        ))
        await session.flush()

        service = DocumentService(session)
        await service.cancel_return_invoice(return_invoice.id)

        await session.refresh(product)
        assert float(product.fiscal_stock) == 10.0

    async def test_confirm_fiscal_invoice_increases_fiscal_stock_db(
        self, session: AsyncSession
    ):
        """DocumentService.confirm_invoice: фіскальна накладна → fiscal_stock += qty."""
        product = await _create_product(session, fiscal_stock=Decimal("0"))
        supplier = await _create_supplier(session)
        await session.flush()

        cashier = await _create_cashier(session)
        invoice = InvoiceModel(
            id=uuid4(),
            number="ПН-ДБ-001",
            supplier_id=supplier.id,
            invoice_date=__import__("datetime").datetime.utcnow(),
            status=InvoiceStatusModel.DRAFT,
            payment_method=PaymentMethod.CASH,
            is_fiscal=True,
            total_amount=Decimal("1000.00"),
            created_by_id=cashier.id,
        )
        session.add(invoice)
        await session.flush()
        session.add(InvoiceItemModel(
            id=uuid4(),
            invoice_id=invoice.id,
            product_id=product.id,
            quantity=Decimal("10"),
            price=Decimal("100.00"),
            total=Decimal("1000.00"),
        ))
        await session.flush()

        service = DocumentService(session)
        await service.confirm_invoice(invoice.id)

        await session.refresh(product)
        assert float(product.fiscal_stock) == 10.0
        assert product.is_fiscal is True
