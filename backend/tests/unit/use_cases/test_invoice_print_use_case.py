"""Unit tests: InvoicePrintUseCases (друк цінників/етикеток з накладної)."""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.use_cases.invoice_print_use_cases import InvoicePrintUseCases
from app.infrastructure.persistence.models.invoice import InvoiceStatus


def _make_session(*, invoice_status=InvoiceStatus.CONFIRMED) -> AsyncMock:
    """Мок сесії з накладною та шаблоном."""
    session = AsyncMock()

    # Мок результату запиту накладної
    invoice_result = MagicMock()
    invoice = MagicMock()
    invoice.id = uuid4()
    invoice.number = "INV-001"
    invoice.status = invoice_status
    invoice.items = []

    product = MagicMock()
    product.id = uuid4()
    product.title = "Товар"
    product.barcode = "4820012345678"
    product.sku = "SKU-1"
    product.price = 25.0

    item = MagicMock()
    item.product = product
    item.price = 20.0
    item.previous_price = 18.0  # ціна змінилась: 18 -> 20
    invoice.items.append(item)
    invoice_result.scalar_one_or_none.return_value = invoice

    # Мок результату запиту шаблону
    template_result = MagicMock()
    template = MagicMock()
    template.id = uuid4()
    template.is_active = True
    template.content = "<html>{{ products }}</html>"
    template_result.scalar_one_or_none.return_value = template

    # Мок результату запиту налаштувань (SystemSetting)
    setting_result = MagicMock()
    setting_result.scalar_one_or_none.return_value = None  # default fields

    async def execute(stmt):
        # Визначаємо за типом запиту (спрощено: по наявності where)
        text = str(stmt)
        if "print_template" in text.lower() or "PrintTemplate" in text:
            return template_result
        if "system_setting" in text.lower() or "SystemSetting" in text:
            return setting_result
        return invoice_result

    session.execute = execute
    return session


class TestInvoicePrint:
    async def test_render_price_tags_success(self):
        """Рендер цінників (price_tag) для підтвердженої накладної."""
        session = _make_session()
        use_cases = InvoicePrintUseCases(session=session)

        result = await use_cases.render_invoice_print_items(
            invoice_id=uuid4(),
            print_type="price_tag",
            only_changed=False,
            template_id=uuid4(),
            width_mm=40,
            height_mm=25,
            gap_mm=3,
            margin_mm=10,
            barcode_type="code128",
            barcode_height_mm=12,
        )

        assert result["html"] != ""
        assert result["total_labels"] == 1
        assert result["changed_count"] == 1
        assert result["total_count"] == 1

    async def test_render_labels_success(self):
        """Рендер етикеток (label)."""
        session = _make_session()
        use_cases = InvoicePrintUseCases(session=session)

        result = await use_cases.render_invoice_print_items(
            invoice_id=uuid4(),
            print_type="label",
            only_changed=False,
            template_id=uuid4(),
            width_mm=58,
            height_mm=40,
            gap_mm=2,
            margin_mm=0,
            barcode_type="code128",
            barcode_height_mm=10,
        )

        assert result["html"] != ""
        assert result["total_pages"] is None  # для етикеток сторінки не рахуються

    async def test_only_changed_filters(self):
        """only_changed=True залишає тільки товари зі зміненою ціною."""
        session = _make_session()
        use_cases = InvoicePrintUseCases(session=session)

        # Обидва товари у списку змінені, тож результат той самий
        result = await use_cases.render_invoice_print_items(
            invoice_id=uuid4(),
            print_type="price_tag",
            only_changed=True,
            template_id=uuid4(),
            width_mm=40,
            height_mm=25,
            gap_mm=3,
            margin_mm=10,
            barcode_type="code128",
            barcode_height_mm=12,
        )

        assert result["total_labels"] == 1

    async def test_draft_invoice_raises(self):
        """Друк з чернетки заборонено."""
        session = _make_session(invoice_status=InvoiceStatus.DRAFT)
        use_cases = InvoicePrintUseCases(session=session)

        with pytest.raises(ValueError, match="підтверджених"):
            await use_cases.render_invoice_print_items(
                invoice_id=uuid4(),
                print_type="price_tag",
                only_changed=False,
                template_id=uuid4(),
                width_mm=40,
                height_mm=25,
                gap_mm=3,
                margin_mm=10,
                barcode_type="code128",
                barcode_height_mm=12,
            )

    async def test_invoice_not_found_raises(self):
        """Накладна не знайдена → ValueError."""
        session = AsyncMock()
        result = MagicMock()
        result.scalar_one_or_none.return_value = None

        async def execute(stmt):
            return result

        session.execute = execute
        use_cases = InvoicePrintUseCases(session=session)

        with pytest.raises(ValueError, match="не знайдено"):
            await use_cases.render_invoice_print_items(
                invoice_id=uuid4(),
                print_type="price_tag",
                only_changed=False,
                template_id=uuid4(),
                width_mm=40,
                height_mm=25,
                gap_mm=3,
                margin_mm=10,
                barcode_type="code128",
                barcode_height_mm=12,
            )
