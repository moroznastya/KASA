"""Unit tests: Invoice Repository."""

from __future__ import annotations

from uuid import uuid4
from datetime import datetime, timezone

import pytest

from app.infrastructure.persistence.models.invoice import Invoice, InvoiceStatus


class TestInvoiceRepository:

    @pytest.mark.asyncio
    async def test_save_invoice(self, invoice_repo, session):
        inv = Invoice(
            id=uuid4(), supplier_id=uuid4(),
            number="INV-001", total_amount=1000.00,
            status=InvoiceStatus.DRAFT,
            invoice_date=datetime.now(timezone.utc),
            created_by_id=uuid4(),
        )
        created = await invoice_repo.save(inv)
        await session.commit()
        assert created.number == "INV-001"
