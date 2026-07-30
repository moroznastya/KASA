"""Unit tests: Invoice Use Cases."""

from __future__ import annotations

from uuid import uuid4
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock

import pytest

from app.domain.use_cases.invoice_use_cases import (
    CreateInvoiceUseCase,
    ConfirmInvoiceUseCase,
    CancelInvoiceUseCase,
    InvoiceItemCreate,
)
from app.domain.entities.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.domain.entities.product import Product
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate
from app.domain.repositories import IProductRepository


class TestCreateInvoiceUseCase:
    """Тести для CreateInvoiceUseCase."""

    @pytest.mark.asyncio
    async def test_create_invoice_success(self):
        """Успішне створення накладної."""
        invoice_repo = AsyncMock()
        product_repo = AsyncMock()
        stock_service = MagicMock()
        document_service = MagicMock()
        uow = AsyncMock()

        supplier_id = uuid4()
        product_id = uuid4()
        product = Product(
            id=product_id,
            name="Тестовий товар",
            price=Money(Decimal("100")),
            unit="шт",
        )
        product_repo.find_by_id.return_value = product

        expected_invoice = Invoice(
            id=uuid4(),
            number="INV-001",
            supplier_id=supplier_id,
        )
        invoice_repo.save.return_value = expected_invoice

        use_case = CreateInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=product_repo,
            stock_service=stock_service,
            document_service=document_service,
            uow=uow,
        )

        items = [
            InvoiceItemCreate(
                product_id=product_id,
                quantity=Decimal("10"),
                price=Decimal("100.00"),
                tax_rate_percent=20,
            )
        ]

        result = await use_case.execute(
            supplier_id=supplier_id,
            items=items,
            number="INV-001",
        )

        assert result.number == "INV-001"
        invoice_repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_create_invoice_empty_items_raises(self):
        """Помилка при пустому списку позицій."""
        use_case = CreateInvoiceUseCase(
            invoice_repo=AsyncMock(),
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="хоча б одну позицію"):
            await use_case.execute(
                supplier_id=uuid4(),
                items=[],
            )

    @pytest.mark.asyncio
    async def test_create_invoice_product_not_found_raises(self):
        """Помилка коли товар не знайдено."""
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = None

        use_case = CreateInvoiceUseCase(
            invoice_repo=AsyncMock(),
            product_repo=product_repo,
            stock_service=MagicMock(),
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(
                supplier_id=uuid4(),
                items=[
                    InvoiceItemCreate(
                        product_id=uuid4(),
                        quantity=Decimal("5"),
                        price=Decimal("100"),
                    )
                ],
            )

    @pytest.mark.asyncio
    async def test_create_invoice_zero_quantity_raises(self):
        """Помилка при нульовій кількості."""
        product_id = uuid4()
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = Product(
            id=product_id, name="Товар", unit="шт",
            price=Money(Decimal("100")),
        )

        use_case = CreateInvoiceUseCase(
            invoice_repo=AsyncMock(),
            product_repo=product_repo,
            stock_service=MagicMock(),
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="повинна бути додатною"):
            await use_case.execute(
                supplier_id=uuid4(),
                items=[
                    InvoiceItemCreate(
                        product_id=product_id,
                        quantity=Decimal("0"),
                        price=Decimal("100"),
                    )
                ],
            )


class TestConfirmInvoiceUseCase:
    """Тести для ConfirmInvoiceUseCase."""

    @pytest.mark.asyncio
    async def test_confirm_invoice_success(self):
        """Успішне підтвердження накладної."""
        invoice_repo = AsyncMock()
        product_repo = AsyncMock()
        stock_service = MagicMock()
        ledger_service = MagicMock()
        uow = AsyncMock()

        invoice_id = uuid4()
        product_id = uuid4()
        item = InvoiceItem(
            product_id=product_id,
            quantity=Quantity(Decimal("10")),
            price=Money(Decimal("100")),
            tax_rate=TaxRate.standard(),
            name="Товар",
        )
        invoice = Invoice(
            id=invoice_id,
            number="INV-001",
            supplier_id=uuid4(),
            items=[item],
            status=InvoiceStatus.DRAFT,
        )
        invoice_repo.find_by_id.return_value = invoice
        invoice_repo.update.return_value = invoice

        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("5")),
            unit="шт",
        )
        product_repo.find_by_id.return_value = product

        use_case = ConfirmInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=product_repo,
            stock_service=stock_service,
            ledger_service=ledger_service,
            uow=uow,
        )

        result = await use_case.execute(invoice_id=invoice_id)

        assert result.status == InvoiceStatus.CONFIRMED
        invoice_repo.update.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_confirm_invoice_not_found_raises(self):
        """Помилка при неіснуючій накладній."""
        invoice_repo = AsyncMock()
        invoice_repo.find_by_id.return_value = None
        use_case = ConfirmInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            ledger_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(invoice_id=uuid4())

    @pytest.mark.asyncio
    async def test_confirm_invoice_already_confirmed_raises(self):
        """Помилка при повторному підтвердженні."""
        invoice_id = uuid4()
        invoice = Invoice(
            id=invoice_id,
            number="INV-001",
            supplier_id=uuid4(),
            status=InvoiceStatus.CONFIRMED,
        )
        invoice_repo = AsyncMock()
        invoice_repo.find_by_id.return_value = invoice

        use_case = ConfirmInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            ledger_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="Неможливо підтвердити"):
            await use_case.execute(invoice_id=invoice_id)

    @pytest.mark.asyncio
    async def test_confirm_invoice_no_items_raises(self):
        """Помилка при підтвердженні накладної без позицій."""
        invoice_id = uuid4()
        invoice = Invoice(
            id=invoice_id,
            number="INV-001",
            supplier_id=uuid4(),
            status=InvoiceStatus.DRAFT,
            items=[],
        )
        invoice_repo = AsyncMock()
        invoice_repo.find_by_id.return_value = invoice

        use_case = ConfirmInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            ledger_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="без позицій"):
            await use_case.execute(invoice_id=invoice_id)


class TestCancelInvoiceUseCase:
    """Тести для CancelInvoiceUseCase."""

    @pytest.mark.asyncio
    async def test_cancel_invoice_success(self):
        """Успішне скасування накладної."""
        invoice_repo = AsyncMock()
        product_repo = AsyncMock()
        stock_service = MagicMock()
        ledger_service = MagicMock()
        uow = AsyncMock()

        invoice_id = uuid4()
        product_id = uuid4()
        item = InvoiceItem(
            product_id=product_id,
            quantity=Quantity(Decimal("10")),
            price=Money(Decimal("100")),
            tax_rate=TaxRate.standard(),
            name="Товар",
        )
        invoice = Invoice(
            id=invoice_id,
            number="INV-001",
            supplier_id=uuid4(),
            items=[item],
            status=InvoiceStatus.CONFIRMED,
        )
        invoice_repo.find_by_id.return_value = invoice
        invoice_repo.update.return_value = invoice

        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("15")),  # 5 original + 10 from invoice
            unit="шт",
        )
        product_repo.find_by_id.return_value = product

        use_case = CancelInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=product_repo,
            stock_service=stock_service,
            ledger_service=ledger_service,
            uow=uow,
        )

        result = await use_case.execute(invoice_id=invoice_id)

        assert result.status == InvoiceStatus.CANCELLED
        invoice_repo.update.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_cancel_invoice_not_found_raises(self):
        """Помилка при неіснуючій накладній."""
        invoice_repo = AsyncMock()
        invoice_repo.find_by_id.return_value = None
        use_case = CancelInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            ledger_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(invoice_id=uuid4())

    @pytest.mark.asyncio
    async def test_cancel_invoice_not_confirmed_raises(self):
        """Помилка при скасуванні непідтвердженої накладної."""
        invoice_id = uuid4()
        invoice = Invoice(
            id=invoice_id,
            number="INV-001",
            supplier_id=uuid4(),
            status=InvoiceStatus.DRAFT,
        )
        invoice_repo = AsyncMock()
        invoice_repo.find_by_id.return_value = invoice

        use_case = CancelInvoiceUseCase(
            invoice_repo=invoice_repo,
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            ledger_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="лише підтверджену"):
            await use_case.execute(invoice_id=invoice_id)
