"""
Infrastructure Layer: SupplierRepository — реалізація ISupplierRepository.

Використовує SQLAlchemy ORM модель SupplierModel для роботи з БД.
"""

from __future__ import annotations

import logging
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_, and_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.entities.supplier import Supplier
from app.domain.repositories.i_supplier_repository import ISupplierRepository
from app.infrastructure.persistence.models import SupplierModel

logger = logging.getLogger(__name__)


class SupplierRepository(ISupplierRepository):
    """
    Репозиторій постачальників.

    Реалізує ISupplierRepository використовуючи SQLAlchemy ORM.
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

    async def save(self, supplier: Supplier) -> Supplier:
        model = self._to_model(supplier)
        self.session.add(model)
        await self.session.flush()
        return self._to_domain(model)

    async def update(self, supplier: Supplier) -> Supplier:
        model = await self._get_model(supplier.id)
        if model is None:
            raise ValueError(f"Supplier with id {supplier.id} not found")
        self._update_model(model, supplier)
        await self.session.flush()
        return self._to_domain(model)

    async def find_by_id(self, supplier_id: UUID) -> Optional[Supplier]:
        model = await self._get_model(supplier_id)
        return self._to_domain(model) if model else None

    async def find_by_name(self, name: str) -> Optional[Supplier]:
        result = await self.session.execute(
            select(SupplierModel).where(SupplierModel.name == name)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_by_edrpou(self, edrpou: str) -> Optional[Supplier]:
        result = await self.session.execute(
            select(SupplierModel).where(SupplierModel.edrpou == edrpou)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def search(
        self,
        query: Optional[str] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Supplier], int]:
        stmt = select(SupplierModel)
        count_stmt = select(func.count(SupplierModel.id))

        conditions = []
        if query:
            like_pattern = f"%{query}%"
            conditions.append(
                or_(
                    SupplierModel.name.ilike(like_pattern),
                    SupplierModel.edrpou.ilike(like_pattern),
                    SupplierModel.phone.ilike(like_pattern),
                )
            )
        if is_active is not None:
            conditions.append(SupplierModel.is_active == is_active)

        if conditions:
            stmt = stmt.where(and_(*conditions))
            count_stmt = count_stmt.where(and_(*conditions))

        total_result = await self.session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = stmt.offset(offset).limit(size).order_by(SupplierModel.name)

        result = await self.session.execute(stmt)
        models = result.scalars().all()

        return [self._to_domain(m) for m in models], total

    async def delete(self, supplier_id: UUID) -> None:
        model = await self._get_model(supplier_id)
        if model:
            await self.session.delete(model)
            await self.session.flush()

    async def count(self) -> int:
        result = await self.session.execute(
            select(func.count(SupplierModel.id))
        )
        return result.scalar() or 0

    async def get_all_with_balance(self) -> list[Supplier]:
        result = await self.session.execute(
            select(SupplierModel).where(SupplierModel.is_active == True)
        )
        return [self._to_domain(m) for m in result.scalars().all()]

    # ─── Маппінг ────────────────────────────────────────────────────────────

    def _to_domain(self, model: SupplierModel | None) -> Supplier | None:
        if model is None:
            return None
        return Supplier(
            id=model.id,
            name=model.name,
            edrpou=model.edrpou or "",
            phone=model.phone or "",
            is_active=model.is_active,
        )

    def _to_model(self, domain: Supplier) -> SupplierModel:
        return SupplierModel(
            id=domain.id,
            name=domain.name,
            edrpou=domain.edrpou or None,
            phone=domain.phone or None,
            is_active=domain.is_active,
        )

    def _update_model(self, model: SupplierModel, domain: Supplier) -> None:
        model.name = domain.name
        model.edrpou = domain.edrpou or None
        model.phone = domain.phone or None
        model.is_active = domain.is_active

    async def _get_model(self, supplier_id: UUID) -> Optional[SupplierModel]:
        result = await self.session.execute(
            select(SupplierModel).where(SupplierModel.id == supplier_id)
        )
        return result.scalar_one_or_none()
