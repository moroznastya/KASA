"""Unit tests: нові методи InvoiceUseCases (update, delete, payment-info, price-changes)."""

from __future__ import annotations

from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.use_cases.invoice_use_cases import InvoiceUseCases
from app.domain.entities.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.domain.entities.product import Product
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate


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


def _make_invoice(status=InvoiceStatus.DRAFT) -> Invoice:
    invoice = Invoice(number="INV-001", supplier_id=uuid4())
    invoice.add_item(InvoiceItem(
        product_id=uuid4(),
        quantity=Quantity(Decimal("2")),
        price=Money(Decimal("10")),
        tax_rate=TaxRate(Decimal("20")),
    ))
    if status == InvoiceStatus.CONFIRMED:
        invoice.confirm()
    elif status == InvoiceStatus.CANCELLED:
        invoice.cancel()
    return invoice


def _build_use_cases(*, invoice: Invoice) -> tuple[InvoiceUseCases, AsyncMock]:
    invoice_repo = AsyncMock()
    invoice_repo.find_by_id.return_value = invoice
    invoice_repo.update.return_value = invoice
    use_cases = InvoiceUseCases(
        invoice_repo=invoice_repo,
        product_repo=AsyncMock(),
        supplier_repo=AsyncMock(),
        unit_of_work=_make_uow(),
        event_bus=AsyncMock(),
    )
    return use_cases, invoice_repo


class TestUpdateInvoice:
    async def test_update_draft_success(self):
        """Оновлення чернетки: змінює номер та зберігає."""
        invoice = _make_invoice()
        use_cases, repo = _build_use_cases(invoice=invoice)

        from app.application.dto.invoice_dto import InvoiceUpdateDTO
        result = await use_cases.update_invoice(
            invoice.id,
            InvoiceUpdateDTO(number="INV-002", notes="оновлено"),
        )

        assert invoice.number == "INV-002"
        assert invoice.notes == "оновлено"
        repo.update.assert_awaited_once()
        assert result.number == "INV-002"

    async def test_update_confirmed_raises(self):
        """Оновлення підтвердженої накладної заборонено."""
        invoice = _make_invoice(status=InvoiceStatus.CONFIRMED)
        use_cases, _ = _build_use_cases(invoice=invoice)

        from app.application.dto.invoice_dto import InvoiceUpdateDTO
        with pytest.raises(ValueError, match="тільки чернетки"):
            await use_cases.update_invoice(invoice.id, InvoiceUpdateDTO(number="X"))

    async def test_update_not_found_raises(self):
        """Накладна не знайдена → ValueError."""
        use_cases, repo = _build_use_cases(invoice=_make_invoice())
        repo.find_by_id.return_value = None

        from app.application.dto.invoice_dto import InvoiceUpdateDTO
        with pytest.raises(ValueError, match="не знайдено"):
            await use_cases.update_invoice(uuid4(), InvoiceUpdateDTO())


class TestDeleteInvoice:
    async def test_delete_draft_success(self):
        """Видалення чернетки."""
        invoice = _make_invoice()
        use_cases, repo = _build_use_cases(invoice=invoice)

        await use_cases.delete_invoice(invoice.id)

        repo.delete.assert_awaited_once_with(invoice.id)

    async def test_delete_confirmed_raises(self):
        """Видалення підтвердженої накладної заборонено."""
        invoice = _make_invoice(status=InvoiceStatus.CONFIRMED)
        use_cases, _ = _build_use_cases(invoice=invoice)

        with pytest.raises(ValueError, match="тільки чернетку"):
            await use_cases.delete_invoice(invoice.id)


class TestPaymentInfo:
    async def test_payment_info_delegates(self):
        """payment-info делегується в репозиторій."""
        invoice = _make_invoice()
        use_cases, repo = _build_use_cases(invoice=invoice)
        repo.get_payment_info.return_value = {
            "invoice_id": invoice.id,
            "invoice_number": "INV-001",
            "invoice_date": None,
            "total_amount": Decimal("20.00"),
            "paid_amount": Decimal("10.00"),
            "remaining": Decimal("10.00"),
        }

        result = await use_cases.get_invoice_payment_info(invoice.id)

        assert result["total_amount"] == Decimal("20.00")
        assert result["remaining"] == Decimal("10.00")
        repo.get_payment_info.assert_awaited_once_with(invoice.id)


class TestPriceChanges:
    async def test_price_changes_computes_difference(self):
        """price-changes: рахує зміну ціни (previous_price vs invoice price)."""
        product = Product(
            id=uuid4(),
            name="Товар",
            price=Money(Decimal("15")),
        )
        invoice = Invoice(number="INV-001", supplier_id=uuid4(), status=InvoiceStatus.CONFIRMED)
        # previous_price=10 (ціна до накладної), ціна в накладній=12
        invoice.items.append(MagicMock(
            product=product,
            price=12.0,
            previous_price=10.0,
        ))

        use_cases, _ = _build_use_cases(invoice=invoice)

        changes = await use_cases.get_invoice_price_changes(invoice.id)

        assert len(changes) == 1
        assert changes[0]["changed"] is True
        assert changes[0]["invoice_price"] == "12.00"
        assert changes[0]["current_price"] == "15.00"
        assert changes[0]["difference"] == "-2.00"

    async def test_price_changes_no_change(self):
        """Ціна не змінилась → changed=False."""
        product = Product(
            id=uuid4(),
            name="Товар",
            price=Money(Decimal("10")),
        )
        invoice = Invoice(number="INV-001", supplier_id=uuid4(), status=InvoiceStatus.CONFIRMED)
        invoice.items.append(MagicMock(
            product=product,
            price=10.0,
            previous_price=10.0,
        ))

        use_cases, _ = _build_use_cases(invoice=invoice)

        changes = await use_cases.get_invoice_price_changes(invoice.id)

        assert changes[0]["changed"] is False
        assert changes[0]["difference"] == "0.00"
