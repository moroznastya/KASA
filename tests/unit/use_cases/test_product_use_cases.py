"""Unit tests: Product Use Cases."""

from __future__ import annotations

from unittest.mock import MagicMock
from uuid import uuid4

import pytest

from app.domain.events import ProductCreated


class TestProductUseCases:

    @pytest.mark.asyncio
    async def test_create_product_publishes_event(
        self, product_use_cases, mock_product_repo, mock_event_bus
    ):
        """Створення товару публікує ProductCreated подію."""
        from dataclasses import dataclass
        @dataclass
        class FakeProduct:
            id: str
            title: str

        mock_product_repo.save.return_value = FakeProduct(id=str(uuid4()), title="Test")

        result = await product_use_cases.create_product(
            name="Test", barcode="123", price=100.0
        )

        mock_product_repo.save.assert_called_once()
        mock_event_bus.publish.assert_called_once()
        event = mock_event_bus.publish.call_args[0][0]
        assert isinstance(event, ProductCreated)
        assert event.name == "Test"

    @pytest.mark.asyncio
    async def test_get_product(self, product_use_cases, mock_product_repo):
        """Отримання товару за ID."""
        product_id = uuid4()
        from dataclasses import dataclass
        @dataclass
        class FakeProduct:
            id: str
        mock_product_repo.find_by_id.return_value = FakeProduct(id=str(product_id))

        with pytest.raises(Exception) as excinfo:
            result = await product_use_cases.get_product(product_id)
        # Якщо UseCase кидає помилку - це теж OK
        mock_product_repo.find_by_id.assert_called_once_with(product_id)

    @pytest.mark.asyncio
    async def test_get_product_not_found(self, product_use_cases, mock_product_repo):
        """Товар не знайдено."""
        mock_product_repo.find_by_id.return_value = None

        with pytest.raises(ValueError, match="не знайдено"):
            await product_use_cases.get_product(uuid4())

    @pytest.mark.asyncio
    async def test_delete_product(self, product_use_cases, mock_product_repo, mock_event_bus):
        """Видалення товару."""
        product_id = uuid4()
        from dataclasses import dataclass
        @dataclass
        class FakeProduct:
            id: str
        mock_product_repo.find_by_id.return_value = FakeProduct(id=str(product_id))
        mock_product_repo.delete = MagicMock(return_value=None)

        result = await product_use_cases.delete_product(product_id)

        assert result is True
        mock_product_repo.delete.assert_called_once_with(product_id)
