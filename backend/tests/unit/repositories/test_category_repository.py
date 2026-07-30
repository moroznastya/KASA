"""Unit tests: Category Repository."""

from __future__ import annotations

from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.category import Category


class TestCategoryRepository:

    @pytest.mark.asyncio
    async def test_save_category(self, category_repo, session):
        """Створення нової категорії."""
        cat = Category(id=uuid4(), name="Тестова категорія")
        created = await category_repo.save(cat)
        await session.commit()
        assert created.name == "Тестова категорія"

    @pytest.mark.asyncio
    async def test_find_by_id(self, category_repo, session):
        """Пошук категорії за ID."""
        cat = Category(id=uuid4(), name="Категорія 1")
        await category_repo.save(cat)
        await session.commit()
        found = await category_repo.find_by_id(cat.id)
        assert found is not None
        assert found.name == "Категорія 1"

    @pytest.mark.asyncio
    async def test_find_by_name(self, category_repo, session):
        """Пошук категорії за назвою."""
        cat = Category(id=uuid4(), name="Унікальна назва")
        await category_repo.save(cat)
        await session.commit()
        found = await category_repo.find_by_name("Унікальна назва")
        assert found is not None
        assert found.id == cat.id

    @pytest.mark.asyncio
    async def test_delete_category(self, category_repo, session):
        """Видалення категорії."""
        cat = Category(id=uuid4(), name="Видалити")
        await category_repo.save(cat)
        await session.commit()

        await category_repo.delete(cat.id)
        await session.commit()

        found = await category_repo.find_by_id(cat.id)
        assert found is None
