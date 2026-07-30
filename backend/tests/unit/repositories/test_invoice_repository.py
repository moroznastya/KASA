"""Unit tests: Invoice Repository."""

from __future__ import annotations

from uuid import uuid4
from decimal import Decimal
from datetime import date

import pytest

from app.infrastructure.persistence.models.invoice import Invoice, InvoiceStatus


class TestInvoiceRepository:

    @pytest.mark.asyncio
    async def test_save_invoice(self, invoice_repo, session):
        """Створення нової накладної."""
        inv = Invoice(
            id=uuid4(),
            supplier_id=uuid4(),
            number="INV-001",
            total_amount=Decimal("1000.00"),
            status=InvoiceStatus.DRAFT,
            invoice_date=date.today(),
            created_by_id=uuid4(),
        )
        created = await invoice_repo.save(inv)
        await session.commit()
        assert created.number == "INV-001"
        assert created.status == InvoiceStatus.DRAFT

    @pytest.mark.asyncio
    async def test_find_by_id(self, invoice_repo, session):
        """Пошук накладної за ID."""
        inv = Invoice(
            id=uuid4(),
            supplier_id=uuid4(),
            number="INV-002",
            total_amount=Decimal("500.00"),
            status=InvoiceStatus.CONFIRMED,
            invoice_date=date.today(),
            created_by_id=uuid4(),
        )
        await invoice_repo.save(inv)
        await session.commit()
        found = await invoice_repo.find_by_id(inv.id)
        assert found is not None
        assert found.number == "INV-002"

    @pytest.mark.asyncio
    async def test_find_by_number(self, invoice_repo, session):
        """Пошук накладної за номером."""
        inv = Invoice(
            id=uuid4(),
            supplier_id=uuid4(),
            number="INV-UNIQUE-123",
            total_amount=Decimal("250.00"),
            status=InvoiceStatus.DRAFT,
            invoice_date=date.today(),
            created_by_id=uuid4(),
        )
        await invoice_repo.save(inv)
        await session.commit()
        found = await invoice_repo.find_by_number("INV-UNIQUE-123")
        assert found is not None
