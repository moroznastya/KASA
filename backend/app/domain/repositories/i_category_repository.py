"""
Repository Interface: ICategoryRepository.

Визначає контракт для роботи з категоріями товарів.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from typing import Optional, Protocol
from uuid import UUID

from ..entities.category import Category


class ICategoryRepository(Protocol):
    """
    Інтерфейс репозиторію категорій.
    """

    async def save(self, category: Category) -> Category:
        """
        Зберігає нову категорію.

        Args:
            category: Entity категорії.

        Returns:
            Збережена категорія.
        """
        ...

    async def update(self, category: Category) -> Category:
        """
        Оновлює існуючу категорію.

        Args:
            category: Entity категорії з оновленими даними.

        Returns:
            Оновлена категорія.
        """
        ...

    async def find_by_id(self, category_id: UUID) -> Optional[Category]:
        """
        Знаходить категорію за ID.

        Args:
            category_id: UUID категорії.

        Returns:
            Category або None.
        """
        ...

    async def find_by_name(self, name: str) -> Optional[Category]:
        """
        Знаходить категорію за назвою.

        Args:
            name: Назва категорії.

        Returns:
            Category або None.
        """
        ...

    async def find_roots(self) -> list[Category]:
        """
        Знаходить всі кореневі категорії (без батька).

        Returns:
            Список кореневих категорій.
        """
        ...

    async def find_children(self, parent_id: UUID) -> list[Category]:
        """
        Знаходить дочірні категорії.

        Args:
            parent_id: UUID батьківської категорії.

        Returns:
            Список дочірніх категорій.
        """
        ...

    async def find_all(self) -> list[Category]:
        """
        Знаходить всі категорії.

        Returns:
            Список всіх категорій.
        """
        ...

    async def search(
        self,
        query: Optional[str] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Category], int]:
        """
        Пошук категорій.

        Args:
            query: Текстовий пошук (назва).
            is_active: Фільтр за активністю.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список категорій, загальна кількість).
        """
        ...

    async def delete(self, category_id: UUID) -> None:
        """
        Видаляє категорію за ID.

        Args:
            category_id: UUID категорії.
        """
        ...

    async def count(self) -> int:
        """
        Повертає загальну кількість категорій.

        Returns:
            Кількість категорій.
        """
        ...

    async def exists_by_name(self, name: str, exclude_id: Optional[UUID] = None) -> bool:
        """
        Перевіряє, чи існує категорія з такою назвою.

        Args:
            name: Назва для перевірки.
            exclude_id: ID для виключення.

        Returns:
            True якщо існує.
        """
        ...
