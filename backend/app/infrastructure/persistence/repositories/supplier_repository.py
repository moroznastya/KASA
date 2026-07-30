"""
Repository Implementation: SQLAlchemySupplierRepository.

Реалізація ISupplierRepository з використанням SQLAlchemy.
"""

from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.repositories import ISupplierRepository
from app.infrastructure.persistence.models.supplier import Supplier


class SQLAlchemySupplierRepository(ISupplierRepository):
    """
    SQLAlchemy реалізація репозиторію постачальників.

    Працює з моделлю Supplier.
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, supplier: Supplier) -> Supplier:
        """Зберігає нового постачальника."""
        self._session.add(supplier)
        await self._session.flush()
        return supplier

    async def update(self, supplier: Supplier) -> Supplier:
        """Оновлює існуючого постачальника."""
        merged = await self._session.merge(supplier)
        await self._session.flush()
        return merged

    async def find_by_id(self, supplier_id: UUID) -> Optional[Supplier]:
        """Знаходить постачальника за ID."""
        stmt = select(Supplier).where(Supplier.id == supplier_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_name(self, name: str) -> Optional[Supplier]:
        """Знаходить постачальника за назвою."""
        stmt = select(Supplier).where(Supplier.name == name)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_edrpou(self, edrpou: str) -> Optional[Supplier]:
        """Знаходить постачальника за кодом ЄДРПОУ."""
        stmt = select(Supplier).where(Supplier.edrpou == edrpou)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def search(
        self,
        query: Optional[str] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Supplier], int]:
        """Пошук постачальників з фільтрацією та пагінацією."""
        base_stmt = select(Supplier)

        if query:
            like_pattern = f"%{query}%"
            base_stmt = base_stmt.where(
                or_(
                    Supplier.name.ilike(like_pattern),
                    Supplier.edrpou.ilike(like_pattern),
                    Supplier.phone.ilike(like_pattern),
                )
            )

        # Supplier не має поля is_active, тому фільтр пропускаємо
        # (але параметр приймаємо для сумісності з інтерфейсом)

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = base_stmt.offset(offset).limit(size).order_by(Supplier.name)
        result = await self._session.execute(stmt)
        suppliers = list(result.scalars().all())

        return suppliers, total

    async def delete(self, supplier_id: UUID) -> None:
        """Видаляє постачальника за ID."""
        supplier = await self.find_by_id(supplier_id)
        if supplier is not None:
            await self._session.delete(supplier)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість постачальників."""
        stmt = select(func.count()).select_from(Supplier)
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def get_all_with_balance(self) -> list[Supplier]:
        """Повертає всіх постачальників."""
        stmt = select(Supplier).order_by(Supplier.name)
        result = await self._session.execute(stmt)
        return list(result.scalars().all())
