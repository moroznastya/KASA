"""Unit tests: Receipt Repository."""

from __future__ import annotations

from uuid import uuid4
from decimal import Decimal

import pytest

from app.infrastructure.persistence.models.receipt import Receipt, ReceiptType


class TestReceiptRepository:

    @pytest.mark.asyncio
    async def test_save_receipt(self, receipt_repo, session):
        """Створення нового чеку."""
        r = Receipt(
            id=uuid4(),
            cashier_id=uuid4(),
            receipt_number=f"RCP-{uuid4().hex[:8].upper()}",
            total_amount=Decimal("500.00"),
            receipt_type=ReceiptType.SALE,
        )
        created = await receipt_repo.save(r)
        await session.commit()
        assert created.total_amount == Decimal("500.00")
        assert created.receipt_type == ReceiptType.SALE

    @pytest.mark.asyncio
    async def test_find_by_id(self, receipt_repo, session):
        """Пошук чеку за ID."""
        r = Receipt(
            id=uuid4(),
            cashier_id=uuid4(),
            receipt_number=f"RCP-{uuid4().hex[:8].upper()}",
            total_amount=Decimal("150.00"),
            receipt_type=ReceiptType.SALE,
        )
        await receipt_repo.save(r)
        await session.commit()
        found = await receipt_repo.find_by_id(r.id)
        assert found is not None
        assert found.id == r.id

    @pytest.mark.asyncio
    async def test_find_by_number(self, receipt_repo, session):
        """Пошук чеку за номером."""
        receipt_number = f"RCP-{uuid4().hex[:8].upper()}"
        r = Receipt(
            id=uuid4(),
            cashier_id=uuid4(),
            receipt_number=receipt_number,
            total_amount=Decimal("75.00"),
            receipt_type=ReceiptType.SALE,
        )
        await receipt_repo.save(r)
        await session.commit()
        found = await receipt_repo.find_by_number(receipt_number)
        assert found is not None
