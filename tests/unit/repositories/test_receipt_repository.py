"""Unit tests: Receipt Repository."""

from __future__ import annotations

from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.receipt import Receipt, ReceiptType


class TestReceiptRepository:

    @pytest.mark.asyncio
    async def test_save_receipt(self, receipt_repo, session):
        r = Receipt(
            id=uuid4(), cashier_id=uuid4(),
            receipt_number="RCP-001",
            total_amount=500.00, receipt_type=ReceiptType.SALE,
            is_return=False,
        )
        created = await receipt_repo.save(r)
        await session.commit()
        assert created.total_amount == 500.00
