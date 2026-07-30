"""
Repository Implementation: SQLAlchemyCategoryRepository.

Реалізація ICategoryRepository з використанням SQLAlchemy.
"""

from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.repositories import ICategoryRepository
from app.infrastructure.persistence.models.category import Category


class SQLAlchemyCategoryRepository(ICategoryRepository):
    """
    SQLAlchemy реалізація репозиторію категорій.

    Працює з моделлю Category (самопосилальна ієрархічна структура).
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, category: Category) -> Category:
        """Зберігає нову категорію."""
        self._session.add(category)
        await self._session.flush()
        return category

    async def update(self, category: Category) -> Category:
        """Оновлює існуючу категорію."""
        merged = await self._session.merge(category)
        await self._session.flush()
        return merged

    async def find_by_id(self, category_id: UUID) -> Optional[Category]:
        """Знаходить категорію за ID."""
        stmt = select(Category).where(Category.id == category_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_name(self, name: str) -> Optional[Category]:
        """Знаходить категорію за назвою."""
        stmt = select(Category).where(Category.name == name)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_roots(self) -> list[Category]:
        """Знаходить всі кореневі категорії (без батька)."""
        stmt = (
            select(Category)
            .where(Category.parent_id.is_(None))
            .order_by(Category.name)
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def find_children(self, parent_id: UUID) -> list[Category]:
        """Знаходить дочірні категорії."""
        stmt = (
            select(Category)
            .where(Category.parent_id == parent_id)
            .order_by(Category.name)
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def find_all(self) -> list[Category]:
        """Знаходить всі категорії."""
        stmt = select(Category).order_by(Category.name)
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def search(
        self,
        query: Optional[str] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Category], int]:
        """Пошук категорій з фільтрацією та пагінацією."""
        base_stmt = select(Category)

        if query:
            like_pattern = f"%{query}%"
            base_stmt = base_stmt.where(
                Category.name.ilike(like_pattern)
            )

        # Категорії не мають поля is_active, тому фільтр пропускаємо
        # (але параметр приймаємо для сумісності з інтерфейсом)

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = base_stmt.offset(offset).limit(size).order_by(Category.name)
        result = await self._session.execute(stmt)
        categories = list(result.scalars().all())

        return categories, total

    async def delete(self, category_id: UUID) -> None:
        """Видаляє категорію за ID."""
        category = await self.find_by_id(category_id)
        if category is not None:
            await self._session.delete(category)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість категорій."""
        stmt = select(func.count()).select_from(Category)
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def exists_by_name(
        self, name: str, exclude_id: Optional[UUID] = None
    ) -> bool:
        """Перевіряє, чи існує категорія з такою назвою."""
        stmt = select(Category).where(Category.name == name)
        if exclude_id is not None:
            stmt = stmt.where(Category.id != exclude_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none() is not None
