"""Unit tests: Product Repository."""

from __future__ import annotations

from uuid import uuid4
from decimal import Decimal

import pytest

from app.infrastructure.persistence.models.product import Product


class TestProductRepository:

    @pytest.mark.asyncio
    async def test_save_product(self, product_repo, session):
        """Створення нового товару."""
        product = Product(
            id=uuid4(),
            title="Тестовий товар",
            barcode="123456789",
            price=Decimal("100.00"),
            cost_price=Decimal("50.00"),
            stock=Decimal("10"),
            unit="шт",
        )
        created = await product_repo.save(product)
        await session.commit()
        assert created.id == product.id
        assert created.title == "Тестовий товар"
        assert created.barcode == "123456789"

    @pytest.mark.asyncio
    async def test_find_by_id(self, product_repo, session):
        """Пошук товару за ID."""
        product = Product(
            id=uuid4(),
            title="Пошук",
            barcode="456",
            price=Decimal("200.00"),
            stock=Decimal("5"),
            unit="шт",
        )
        await product_repo.save(product)
        await session.commit()
        found = await product_repo.find_by_id(product.id)
        assert found is not None
        assert found.title == "Пошук"

    @pytest.mark.asyncio
    async def test_find_by_id_not_found(self, product_repo):
        """Пошук неіснуючого товару."""
        found = await product_repo.find_by_id(uuid4())
        assert found is None

    @pytest.mark.asyncio
    async def test_find_by_barcode(self, product_repo, session):
        """Пошук товару за штрих-кодом."""
        product = Product(
            id=uuid4(),
            title="Баркод",
            barcode="999888777",
            price=Decimal("50.00"),
            stock=Decimal("3"),
            unit="шт",
        )
        await product_repo.save(product)
        await session.commit()
        found = await product_repo.find_by_barcode("999888777")
        assert found is not None
        assert found.title == "Баркод"

    @pytest.mark.asyncio
    async def test_update_product(self, product_repo, session):
        """Оновлення товару."""
        product = Product(
            id=uuid4(),
            title="Старе",
            barcode="111",
            price=Decimal("100.00"),
            stock=Decimal("5"),
            unit="шт",
        )
        await product_repo.save(product)
        await session.commit()

        product.title = "Нове"
        product.price = Decimal("150.00")
        updated = await product_repo.update(product)
        await session.commit()
        assert updated.title == "Нове"
        assert updated.price == Decimal("150.00")

    @pytest.mark.asyncio
    async def test_delete_product(self, product_repo, session):
        """Видалення товару."""
        product = Product(
            id=uuid4(),
            title="Видалити",
            barcode="222",
            price=Decimal("75.00"),
            stock=Decimal("2"),
            unit="шт",
        )
        await product_repo.save(product)
        await session.commit()

        await product_repo.delete(product.id)
        await session.commit()

        found = await product_repo.find_by_id(product.id)
        assert found is None

    @pytest.mark.asyncio
    async def test_search_pagination(self, product_repo, session):
        """Пагінація при пошуку товарів."""
        for i in range(5):
            p = Product(
                id=uuid4(),
                title=f"Товар {i}",
                barcode=f"bc{i}",
                price=Decimal(str(i * 10)),
                stock=Decimal("1"),
                unit="шт",
            )
            await product_repo.save(p)
        await session.commit()

        result_page1, total = await product_repo.search(page=1, size=3)
        assert total == 5
        assert len(result_page1) == 3

        result_page2, total2 = await product_repo.search(page=2, size=3)
        assert len(result_page2) == 2
