"""Unit tests: Product Repository."""

from __future__ import annotations

from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.product import Product


class TestProductRepository:

    @pytest.mark.asyncio
    async def test_save_product(self, product_repo, session):
        product = Product(
            id=uuid4(), title="Тест", barcode="123", price=100.0,
        )
        created = await product_repo.save(product)
        await session.commit()
        assert created.id == product.id
        assert created.title == "Тест"

    @pytest.mark.asyncio
    async def test_find_by_id(self, product_repo, session):
        product = Product(
            id=uuid4(), title="Пошук", barcode="456", price=200.0,
        )
        await product_repo.save(product)
        await session.commit()
        found = await product_repo.find_by_id(product.id)
        assert found is not None
        assert found.title == "Пошук"

    @pytest.mark.asyncio
    async def test_find_by_id_not_found(self, product_repo):
        found = await product_repo.find_by_id(uuid4())
        assert found is None

    @pytest.mark.asyncio
    async def test_find_by_barcode(self, product_repo, session):
        product = Product(
            id=uuid4(), title="Баркод", barcode="999", price=50.0,
        )
        await product_repo.save(product)
        await session.commit()
        found = await product_repo.find_by_barcode("999")
        assert found is not None

    @pytest.mark.asyncio
    async def test_update_product(self, product_repo, session):
        product = Product(
            id=uuid4(), title="Старе", barcode="111", price=100.0,
        )
        await product_repo.save(product)
        await session.commit()
        product.title = "Нове"
        product.price = 150.0
        updated = await product_repo.update(product)
        await session.commit()
        assert updated.title == "Нове"
        assert updated.price == 150.0

    @pytest.mark.asyncio
    async def test_delete_product(self, product_repo, session):
        product = Product(
            id=uuid4(), title="Видалити", barcode="222", price=75.0,
        )
        await product_repo.save(product)
        await session.commit()
        await product_repo.delete(product.id)
        await session.commit()
        found = await product_repo.find_by_id(product.id)
        assert found is None

    @pytest.mark.asyncio
    async def test_search_pagination(self, product_repo, session):
        for i in range(5):
            p = Product(
                id=uuid4(), title=f"Товар {i}", barcode=f"bc{i}",
                price=float(i*10),
            )
            await product_repo.save(p)
        await session.commit()
        items, total = await product_repo.search(page=1, size=3)
        assert total == 5
        assert len(items) == 3
        items, total = await product_repo.search(page=2, size=3)
        assert len(items) == 2
