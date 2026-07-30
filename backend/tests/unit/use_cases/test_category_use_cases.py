"""Unit tests: Category Use Cases."""

from __future__ import annotations

from uuid import uuid4
from unittest.mock import AsyncMock

import pytest

from app.domain.use_cases.category_use_cases import (
    CreateCategoryUseCase,
    UpdateCategoryUseCase,
    DeleteCategoryUseCase,
)
from app.domain.entities.category import Category


class TestCreateCategoryUseCase:
    """Тести для CreateCategoryUseCase."""

    @pytest.mark.asyncio
    async def test_create_category_success(self):
        """Успішне створення категорії."""
        category_repo = AsyncMock()
        category_repo.exists_by_name.return_value = False  # <-- важливо!
        uow = AsyncMock()

        expected_category = Category(id=uuid4(), name="Тестова", parent_id=None)
        category_repo.save.return_value = expected_category

        use_case = CreateCategoryUseCase(category_repo=category_repo, uow=uow)
        result = await use_case.execute(name="Тестова")

        assert result.name == "Тестова"
        assert result.parent_id is None
        category_repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_create_category_empty_name_raises(self):
        """Помилка при пустій назві."""
        category_repo = AsyncMock()
        uow = AsyncMock()
        use_case = CreateCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="Назва категорії не може бути пустою"):
            await use_case.execute(name="")
        category_repo.save.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_create_category_duplicate_name_raises(self):
        """Помилка при дублюванні назви."""
        category_repo = AsyncMock()
        category_repo.exists_by_name.return_value = True
        uow = AsyncMock()
        use_case = CreateCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="вже існує"):
            await use_case.execute(name="Дубль")
        category_repo.save.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_create_category_with_parent(self):
        """Створення категорії з батьківською категорією."""
        parent_id = uuid4()
        category_repo = AsyncMock()
        category_repo.exists_by_name.return_value = False  # <-- важливо!
        category_repo.find_by_id.return_value = Category(id=parent_id, name="Батько")
        category_repo.save.return_value = Category(id=uuid4(), name="Дочірня", parent_id=parent_id)
        uow = AsyncMock()

        use_case = CreateCategoryUseCase(category_repo=category_repo, uow=uow)
        result = await use_case.execute(name="Дочірня", parent_id=parent_id)

        assert result.parent_id == parent_id
        category_repo.find_by_id.assert_awaited_with(parent_id)

    @pytest.mark.asyncio
    async def test_create_category_parent_not_found_raises(self):
        """Помилка при неіснуючій батьківській категорії."""
        parent_id = uuid4()
        category_repo = AsyncMock()
        category_repo.exists_by_name.return_value = False  # <-- важливо!
        category_repo.find_by_id.return_value = None
        uow = AsyncMock()
        use_case = CreateCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="не знайдена"):
            await use_case.execute(name="Дочірня", parent_id=parent_id)
        category_repo.save.assert_not_awaited()


class TestUpdateCategoryUseCase:
    """Тести для UpdateCategoryUseCase."""

    @pytest.mark.asyncio
    async def test_update_category_success(self):
        """Успішне оновлення категорії."""
        category_id = uuid4()
        category_repo = AsyncMock()
        existing = Category(id=category_id, name="Стара назва")
        category_repo.find_by_id.return_value = existing
        category_repo.exists_by_name.return_value = False  # <-- важливо!
        category_repo.update.return_value = Category(id=category_id, name="Нова назва")
        uow = AsyncMock()

        use_case = UpdateCategoryUseCase(category_repo=category_repo, uow=uow)
        result = await use_case.execute(category_id=category_id, name="Нова назва")

        assert result.name == "Нова назва"
        category_repo.update.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_update_category_not_found_raises(self):
        """Помилка при оновленні неіснуючої категорії."""
        category_repo = AsyncMock()
        category_repo.find_by_id.return_value = None
        uow = AsyncMock()
        use_case = UpdateCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(category_id=uuid4(), name="Нова")

    @pytest.mark.asyncio
    async def test_update_category_empty_name_raises(self):
        """Помилка при пустій назві."""
        category_id = uuid4()
        category_repo = AsyncMock()
        category_repo.find_by_id.return_value = Category(id=category_id, name="Стара")
        uow = AsyncMock()
        use_case = UpdateCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="не може бути пустою"):
            await use_case.execute(category_id=category_id, name="")
        category_repo.update.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_update_category_self_parent_raises(self):
        """Помилка при спробі зробити категорію власним батьком."""
        category_id = uuid4()
        category_repo = AsyncMock()
        category_repo.find_by_id.return_value = Category(id=category_id, name="Тест")
        uow = AsyncMock()
        use_case = UpdateCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="власним батьком"):
            await use_case.execute(
                category_id=category_id, name="Тест", parent_id=category_id
            )


class TestDeleteCategoryUseCase:
    """Тести для DeleteCategoryUseCase."""

    @pytest.mark.asyncio
    async def test_delete_category_success(self):
        """Успішне видалення категорії."""
        category_id = uuid4()
        category_repo = AsyncMock()
        category_repo.find_by_id.return_value = Category(id=category_id, name="Видалити")
        category_repo.find_children.return_value = []
        uow = AsyncMock()

        use_case = DeleteCategoryUseCase(category_repo=category_repo, uow=uow)
        await use_case.execute(category_id=category_id)

        category_repo.delete.assert_awaited_with(category_id)
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_delete_category_not_found_raises(self):
        """Помилка при видаленні неіснуючої категорії."""
        category_repo = AsyncMock()
        category_repo.find_by_id.return_value = None
        uow = AsyncMock()
        use_case = DeleteCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(category_id=uuid4())
        category_repo.delete.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_delete_category_with_children_raises(self):
        """Помилка при видаленні категорії з дочірніми."""
        category_id = uuid4()
        category_repo = AsyncMock()
        category_repo.find_by_id.return_value = Category(id=category_id, name="Батько")
        category_repo.find_children.return_value = [
            Category(id=uuid4(), name="Дочірня"),
        ]
        uow = AsyncMock()
        use_case = DeleteCategoryUseCase(category_repo=category_repo, uow=uow)

        with pytest.raises(ValueError, match="дочірніх категорій"):
            await use_case.execute(category_id=category_id)
        category_repo.delete.assert_not_awaited()
