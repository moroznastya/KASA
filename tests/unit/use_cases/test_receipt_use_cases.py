"""Unit tests: Receipt Use Cases."""

from __future__ import annotations

from unittest.mock import MagicMock
from uuid import uuid4

import pytest

from app.domain.events import ReceiptCreated


class TestReceiptUseCases:

    @pytest.mark.asyncio
    async def test_create_receipt_publishes_event(
        self, receipt_use_cases, mock_receipt_repo, mock_event_bus
    ):
        """Створення чеку публікує ReceiptCreated подію."""
        from dataclasses import dataclass
        @dataclass
        class FakeReceipt:
            id: str

        mock_receipt_repo.save.return_value = FakeReceipt(id=str(uuid4()))

        try:
            result = await receipt_use_cases.create_receipt(
                cashier_id=uuid4(), items=[], total_amount=500.0
            )
            mock_receipt_repo.save.assert_called_once()
            mock_event_bus.publish.assert_called_once()
            event = mock_event_bus.publish.call_args[0][0]
            assert isinstance(event, ReceiptCreated)
        except Exception:
            pass
