"""
Infrastructure Layer: ReceiptRepository — реалізація IReceiptRepository.

Використовує SQLAlchemy ORM модель ReceiptModel для роботи з БД.
"""

from __future__ import annotations

import logging
from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, and_, extract
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.entities.receipt import Receipt
from app.domain.repositories.i_receipt_repository import IReceiptRepository
from app.infrastructure.persistence.models import ReceiptModel

logger = logging.getLogger(__name__)


class ReceiptRepository(IReceiptRepository):
    """
    Репозиторій чеків продажу.

    Реалізує IReceiptRepository використовуючи SQLAlchemy ORM.
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

    async def save(self, receipt: Receipt) -> Receipt:
        model = self._to_model(receipt)
        self.session.add(model)
        await self.session.flush()
        return self._to_domain(model)

    async def find_by_id(self, receipt_id: UUID) -> Optional[Receipt]:
        result = await self.session.execute(
            select(ReceiptModel).where(ReceiptModel.id == receipt_id)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_by_number(self, number: str) -> Optional[Receipt]:
        result = await self.session.execute(
            select(ReceiptModel).where(ReceiptModel.receipt_number == number)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        stmt = select(ReceiptModel).where(
            and_(
                ReceiptModel.created_at >= date_from,
                ReceiptModel.created_at <= date_to,
            )
        )
        count_stmt = select(func.count(ReceiptModel.id)).where(
            and_(
                ReceiptModel.created_at >= date_from,
                ReceiptModel.created_at <= date_to,
            )
        )
        return await self._paginate(stmt, count_stmt, page, size)

    async def search(
        self,
        query: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        payment_method: Optional[str] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        stmt = select(ReceiptModel)
        count_stmt = select(func.count(ReceiptModel.id))

        conditions = []
        if query:
            like_pattern = f"%{query}%"
            conditions.append(ReceiptModel.receipt_number.ilike(like_pattern))
        if date_from:
            conditions.append(ReceiptModel.created_at >= date_from)
        if date_to:
            conditions.append(ReceiptModel.created_at <= date_to)
        if payment_method:
            conditions.append(ReceiptModel.payment_type == payment_method)

        if conditions:
            stmt = stmt.where(and_(*conditions))
            count_stmt = count_stmt.where(and_(*conditions))

        return await self._paginate(stmt, count_stmt, page, size)

    async def delete(self, receipt_id: UUID) -> None:
        result = await self.session.execute(
            select(ReceiptModel).where(ReceiptModel.id == receipt_id)
        )
        model = result.scalar_one_or_none()
        if model:
            await self.session.delete(model)
            await self.session.flush()

    async def count(self) -> int:
        result = await self.session.execute(
            select(func.count(ReceiptModel.id))
        )
        return result.scalar() or 0

    async def get_daily_total(self, date: datetime) -> float:
        result = await self.session.execute(
            select(func.coalesce(func.sum(ReceiptModel.total_amount), 0)).where(
                and_(
                    extract("year", ReceiptModel.created_at) == date.year,
                    extract("month", ReceiptModel.created_at) == date.month,
                    extract("day", ReceiptModel.created_at) == date.day,
                )
            )
        )
        return float(result.scalar() or 0.0)

    # ─── Допоміжні методи ──────────────────────────────────────────────────

    async def _paginate(self, stmt, count_stmt, page: int, size: int) -> tuple[list[Receipt], int]:
        total_result = await self.session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = stmt.offset(offset).limit(size).order_by(ReceiptModel.created_at.desc())

        result = await self.session.execute(stmt)
        models = result.scalars().all()

        return [self._to_domain(m) for m in models], total

    # ─── Маппінг ────────────────────────────────────────────────────────────

    def _to_domain(self, model: ReceiptModel | None) -> Receipt | None:
        if model is None:
            return None
        return Receipt(
            id=model.id,
            number=model.receipt_number,
            total_amount=float(model.total_amount or 0),
            payment_type=model.payment_type or "cash",
        )

    def _to_model(self, domain: Receipt) -> ReceiptModel:
        return ReceiptModel(
            id=domain.id,
            receipt_number=domain.number,
            total_amount=domain.total_amount,
            payment_type=domain.payment_type,
        )

    async def _get_model(self, receipt_id: UUID) -> Optional[ReceiptModel]:
        result = await self.session.execute(
            select(ReceiptModel).where(ReceiptModel.id == receipt_id)
        )
        return result.scalar_one_or_none()
