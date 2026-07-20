"""
Infrastructure Layer: CategoryRepository — реалізація ICategoryRepository.

Використовує SQLAlchemy ORM модель CategoryModel для роботи з БД.
"""

from __future__ import annotations

import logging
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_, and_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.entities.category import Category
from app.domain.repositories.i_category_repository import ICategoryRepository
from app.infrastructure.persistence.models import CategoryModel

logger = logging.getLogger(__name__)


class CategoryRepository(ICategoryRepository):
    """
    Репозиторій категорій.

    Реалізує ICategoryRepository використовуючи SQLAlchemy ORM.
    """

    def __init__(self) -> None:
        self._session: AsyncSession | None = None

    @property
    def session(self) -> AsyncSession:
        if self._session is None:
            raise RuntimeError("Session not set.")
        return self._session

    def set_session(self, session: AsyncSession) -> None:
        self._session = session

    async def save(self, category: Category) -> Category:
        model = self._to_model(category)
        self.session.add(model)
        await self.session.flush()
        return self._to_domain(model)

    async def update(self, category: Category) -> Category:
        model = await self._get_model(category.id)
        if model is None:
            raise ValueError(f"Category with id {category.id} not found")
        self._update_model(model, category)
        await self.session.flush()
        return self._to_domain(model)

    async def find_by_id(self, category_id: UUID) -> Optional[Category]:
        model = await self._get_model(category_id)
        return self._to_domain(model) if model else None

    async def find_by_name(self, name: str) -> Optional[Category]:
        result = await self.session.execute(
            select(CategoryModel).where(CategoryModel.name == name)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_roots(self) -> list[Category]:
        result = await self.session.execute(
            select(CategoryModel).where(CategoryModel.parent_id.is_(None))
        )
        return [self._to_domain(m) for m in result.scalars().all()]

    async def find_children(self, parent_id: UUID) -> list[Category]:
        result = await self.session.execute(
            select(CategoryModel).where(CategoryModel.parent_id == parent_id)
        )
        return [self._to_domain(m) for m in result.scalars().all()]

    async def find_all(self) -> list[Category]:
        result = await self.session.execute(
            select(CategoryModel).order_by(CategoryModel.name)
        )
        return [self._to_domain(m) for m in result.scalars().all()]

    async def search(
        self,
        query: Optional[str] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Category], int]:
        stmt = select(CategoryModel)
        count_stmt = select(func.count(CategoryModel.id))

        conditions = []
        if query:
            like_pattern = f"%{query}%"
            conditions.append(CategoryModel.name.ilike(like_pattern))
        if is_active is not None:
            conditions.append(CategoryModel.is_active == is_active)

        if conditions:
            stmt = stmt.where(and_(*conditions))
            count_stmt = count_stmt.where(and_(*conditions))

        total_result = await self.session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = stmt.offset(offset).limit(size).order_by(CategoryModel.name)

        result = await self.session.execute(stmt)
        models = result.scalars().all()

        return [self._to_domain(m) for m in models], total

    async def delete(self, category_id: UUID) -> None:
        model = await self._get_model(category_id)
        if model:
            await self.session.delete(model)
            await self.session.flush()

    async def count(self) -> int:
        result = await self.session.execute(
            select(func.count(CategoryModel.id))
        )
        return result.scalar() or 0

    async def exists_by_name(
        self,
        name: str,
        exclude_id: Optional[UUID] = None,
    ) -> bool:
        stmt = select(CategoryModel).where(CategoryModel.name == name)
        if exclude_id:
            stmt = stmt.where(CategoryModel.id != exclude_id)
        result = await self.session.execute(stmt)
        return result.scalar_one_or_none() is not None

    # ─── Маппінг ────────────────────────────────────────────────────────────

    def _to_domain(self, model: CategoryModel | None) -> Category | None:
        if model is None:
            return None
        return Category(
            id=model.id,
            name=model.name,
            parent_id=model.parent_id,
            is_active=model.is_active,
        )

    def _to_model(self, domain: Category) -> CategoryModel:
        return CategoryModel(
            id=domain.id,
            name=domain.name,
            parent_id=domain.parent_id,
            is_active=domain.is_active,
        )

    def _update_model(self, model: CategoryModel, domain: Category) -> None:
        model.name = domain.name
        model.parent_id = domain.parent_id
        model.is_active = domain.is_active

    async def _get_model(self, category_id: UUID) -> Optional[CategoryModel]:
        result = await self.session.execute(
            select(CategoryModel).where(CategoryModel.id == category_id)
        )
        return result.scalar_one_or_none()
