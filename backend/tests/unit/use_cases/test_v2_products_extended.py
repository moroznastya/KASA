"""Unit tests: нові методи ProductUseCases (зображення, штрих-коди, barcode-пошук)."""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.use_cases.product_use_cases import ProductUseCases


def _build_use_cases() -> tuple[ProductUseCases, AsyncMock]:
    """ProductUseCases з моками."""
    product_repo = AsyncMock()
    uow = MagicMock()
    event_bus = AsyncMock()
    use_cases = ProductUseCases(
        product_repo=product_repo,
        unit_of_work=uow,
        event_bus=event_bus,
    )
    return use_cases, product_repo


class TestProductBarcodeLookup:
    async def test_get_by_barcode_found(self):
        """Пошук товару за штрих-кодом: знайдено."""
        use_cases, repo = _build_use_cases()
        from app.domain.entities.product import Product
        from app.domain.value_objects.money import Money
        from decimal import Decimal

        product = Product(name="Товар", price=Money(Decimal("100")))
        repo.find_by_barcode.return_value = product

        dto = await use_cases.get_product_by_barcode("4820012345678")

        assert dto is not None
        assert dto.name == "Товар"
        repo.find_by_barcode.assert_awaited_once_with("4820012345678")

    async def test_get_by_barcode_not_found_returns_none(self):
        """Пошук за штрих-кодом: не знайдено → None (роутер віддає 404)."""
        use_cases, repo = _build_use_cases()
        repo.find_by_barcode.return_value = None

        assert await use_cases.get_product_by_barcode("000") is None


class TestProductImages:
    async def test_add_image_success(self):
        """Додавання зображення: товар існує → делегує в репозиторій."""
        use_cases, repo = _build_use_cases()
        product_id = uuid4()
        repo.find_by_id.return_value = MagicMock()
        repo.add_image.return_value = MagicMock(id=uuid4(), url="/uploads/x.jpg")

        result = await use_cases.add_product_image(product_id, "/uploads/x.jpg", is_main=True)

        assert result.id is not None
        repo.add_image.assert_awaited_once_with(product_id, "/uploads/x.jpg", True)

    async def test_add_image_product_not_found(self):
        """Додавання зображення: товар не знайдено → ValueError."""
        use_cases, repo = _build_use_cases()
        repo.find_by_id.return_value = None

        with pytest.raises(ValueError, match="не знайдено"):
            await use_cases.add_product_image(uuid4(), "/uploads/x.jpg")

    async def test_delete_image_delegates(self):
        """Видалення зображення делегується в репозиторій."""
        use_cases, repo = _build_use_cases()
        image_id = uuid4()

        await use_cases.delete_product_image(image_id)

        repo.delete_image.assert_awaited_once_with(image_id)


class TestProductBarcodes:
    async def test_add_barcode_success(self):
        """Додавання штрих-коду: делегує в репозиторій."""
        use_cases, repo = _build_use_cases()
        product_id = uuid4()
        repo.find_by_id.return_value = MagicMock()
        repo.add_barcode.return_value = MagicMock(id=uuid4(), barcode="123")

        result = await use_cases.add_product_barcode(product_id, "123")

        assert result.barcode == "123"
        repo.add_barcode.assert_awaited_once_with(product_id, "123", False)

    async def test_add_barcode_duplicate_raises(self):
        """Дублікат штрих-коду → ValueError (роутер віддає 409)."""
        use_cases, repo = _build_use_cases()
        repo.find_by_id.return_value = MagicMock()
        repo.add_barcode.side_effect = ValueError("Штрих-код '123' вже існує")

        with pytest.raises(ValueError, match="вже існує"):
            await use_cases.add_product_barcode(uuid4(), "123")

    async def test_delete_barcode_delegates(self):
        """Видалення штрих-коду делегується в репозиторій."""
        use_cases, repo = _build_use_cases()
        barcode_id = uuid4()

        await use_cases.delete_product_barcode(barcode_id)

        repo.delete_barcode.assert_awaited_once_with(barcode_id)
