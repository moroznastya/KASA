"""
Use Cases: Category (Категорії товарів).

Кожен Use Case виконує одну бізнес-операцію:
- CreateCategoryUseCase: створення категорії
- UpdateCategoryUseCase: оновлення категорії
- DeleteCategoryUseCase: видалення категорії

Валідація виконується всередині Use Case, а не в сервісах чи репозиторіях.
"""

from __future__ import annotations

from typing import Optional
from uuid import UUID

from app.domain.entities.category import Category
from app.domain.repositories import ICategoryRepository, IUnitOfWork


class CreateCategoryUseCase:
    """
    Створення нової категорії товарів.

    Валідація:
    - Назва не може бути пустою
    - Назва не може дублюватись
    - Батьківська категорія (якщо вказана) має існувати
    """

    def __init__(
        self,
        category_repo: ICategoryRepository,
        uow: IUnitOfWork,
    ) -> None:
        self._category_repo = category_repo
        self._uow = uow

    async def execute(
        self,
        name: str,
        parent_id: Optional[UUID] = None,
    ) -> Category:
        # Валідація: назва не пуста
        if not name or not name.strip():
            raise ValueError("Назва категорії не може бути пустою")

        name = name.strip()

        # Валідація: унікальність назви
        exists = await self._category_repo.exists_by_name(name)
        if exists:
            raise ValueError(f"Категорія з назвою '{name}' вже існує")

        # Валідація: батьківська категорія існує
        if parent_id is not None:
            parent = await self._category_repo.find_by_id(parent_id)
            if parent is None:
                raise ValueError(
                    f"Батьківська категорія з ID '{parent_id}' не знайдена"
                )

        # Створення entity
        category = Category(name=name, parent_id=parent_id)

        # Збереження
        saved = await self._category_repo.save(category)
        await self._uow.commit()
        return saved


class UpdateCategoryUseCase:
    """
    Оновлення існуючої категорії товарів.

    Валідація:
    - Категорія з вказаним ID має існувати
    - Назва не може бути пустою
    - Назва не може дублюватись (виключаючи поточну категорію)
    - Батьківська категорія (якщо вказана) має існувати
    - Категорія не може бути власним батьком
    """

    def __init__(
        self,
        category_repo: ICategoryRepository,
        uow: IUnitOfWork,
    ) -> None:
        self._category_repo = category_repo
        self._uow = uow

    async def execute(
        self,
        category_id: UUID,
        name: str,
        parent_id: Optional[UUID] = None,
    ) -> Category:
        # Валідація: категорія існує
        category = await self._category_repo.find_by_id(category_id)
        if category is None:
            raise ValueError(f"Категорію з ID '{category_id}' не знайдено")

        # Валідація: назва не пуста
        if not name or not name.strip():
            raise ValueError("Назва категорії не може бути пустою")

        name = name.strip()

        # Валідація: унікальність назви (виключаючи поточну)
        if name != category.name:
            exists = await self._category_repo.exists_by_name(name, exclude_id=category_id)
            if exists:
                raise ValueError(f"Категорія з назвою '{name}' вже існує")

        # Валідація: не може бути власним батьком
        if parent_id == category_id:
            raise ValueError("Категорія не може бути власним батьком")

        # Валідація: батьківська категорія існує
        if parent_id is not None:
            parent = await self._category_repo.find_by_id(parent_id)
            if parent is None:
                raise ValueError(
                    f"Батьківська категорія з ID '{parent_id}' не знайдена"
                )

        # Оновлення entity
        category.name = name
        category.parent_id = parent_id

        updated = await self._category_repo.update(category)
        await self._uow.commit()
        return updated


class DeleteCategoryUseCase:
    """
    Видалення категорії товарів.

    Валідація:
    - Категорія з вказаним ID має існувати
    - Категорія не повинна мати дочірніх категорій
    """

    def __init__(
        self,
        category_repo: ICategoryRepository,
        uow: IUnitOfWork,
    ) -> None:
        self._category_repo = category_repo
        self._uow = uow

    async def execute(self, category_id: UUID) -> None:
        # Валідація: категорія існує
        category = await self._category_repo.find_by_id(category_id)
        if category is None:
            raise ValueError(f"Категорію з ID '{category_id}' не знайдено")

        # Валідація: немає дочірніх категорій
        children = await self._category_repo.find_children(category_id)
        if children:
            raise ValueError(
                f"Неможливо видалити категорію: вона має {len(children)} дочірніх категорій"
            )

        await self._category_repo.delete(category_id)
        await self._uow.commit()
