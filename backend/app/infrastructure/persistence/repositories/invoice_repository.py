"""
Infrastructure Layer: InvoiceRepository — реалізація IInvoiceRepository.

Використовує SQLAlchemy ORM модель InvoiceModel для роботи з БД.
"""

from __future__ import annotations

import logging
from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_, and_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.entities.invoice import Invoice, InvoiceStatus
from app.domain.repositories.i_invoice_repository import IInvoiceRepository
from app.infrastructure.persistence.models import InvoiceModel, InvoiceItemModel

logger = logging.getLogger(__name__)


class InvoiceRepository(IInvoiceRepository):
    """
    Репозиторій прибуткових накладних.

    Реалізує IInvoiceRepository використовуючи SQLAlchemy ORM.
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

    async def save(self, invoice: Invoice) -> Invoice:
        model = self._to_model(invoice)
        self.session.add(model)
        await self.session.flush()
        return self._to_domain(model)

    async def update(self, invoice: Invoice) -> Invoice:
        model = await self._get_model(invoice.id)
        if model is None:
            raise ValueError(f"Invoice with id {invoice.id} not found")
        self._update_model(model, invoice)
        await self.session.flush()
        return self._to_domain(model)

    async def find_by_id(self, invoice_id: UUID) -> Optional[Invoice]:
        model = await self._get_model(invoice_id)
        return self._to_domain(model) if model else None

    async def find_by_number(self, number: str) -> Optional[Invoice]:
        result = await self.session.execute(
            select(InvoiceModel).where(InvoiceModel.number == number)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_by_supplier(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        stmt = select(InvoiceModel).where(InvoiceModel.supplier_id == supplier_id)
        count_stmt = select(func.count(InvoiceModel.id)).where(
            InvoiceModel.supplier_id == supplier_id
        )
        return await self._paginate(stmt, count_stmt, page, size)

    async def find_by_status(
        self,
        status: InvoiceStatus,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        status_str = status.value
        stmt = select(InvoiceModel).where(InvoiceModel.status == status_str)
        count_stmt = select(func.count(InvoiceModel.id)).where(
            InvoiceModel.status == status_str
        )
        return await self._paginate(stmt, count_stmt, page, size)

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        stmt = select(InvoiceModel).where(
            and_(
                InvoiceModel.invoice_date >= date_from,
                InvoiceModel.invoice_date <= date_to,
            )
        )
        count_stmt = select(func.count(InvoiceModel.id)).where(
            and_(
                InvoiceModel.invoice_date >= date_from,
                InvoiceModel.invoice_date <= date_to,
            )
        )
        return await self._paginate(stmt, count_stmt, page, size)

    async def search(
        self,
        query: Optional[str] = None,
        supplier_id: Optional[UUID] = None,
        status: Optional[InvoiceStatus] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        stmt = select(InvoiceModel)
        count_stmt = select(func.count(InvoiceModel.id))

        conditions = []
        if query:
            like_pattern = f"%{query}%"
            conditions.append(
                or_(
                    InvoiceModel.number.ilike(like_pattern),
                    InvoiceModel.notes.ilike(like_pattern),
                )
            )
        if supplier_id:
            conditions.append(InvoiceModel.supplier_id == supplier_id)
        if status:
            conditions.append(InvoiceModel.status == status.value)
        if date_from:
            conditions.append(InvoiceModel.invoice_date >= date_from)
        if date_to:
            conditions.append(InvoiceModel.invoice_date <= date_to)

        if conditions:
            stmt = stmt.where(and_(*conditions))
            count_stmt = count_stmt.where(and_(*conditions))

        return await self._paginate(stmt, count_stmt, page, size)

    async def delete(self, invoice_id: UUID) -> None:
        model = await self._get_model(invoice_id)
        if model:
            await self.session.delete(model)
            await self.session.flush()

    async def count(self) -> int:
        result = await self.session.execute(
            select(func.count(InvoiceModel.id))
        )
        return result.scalar() or 0

    # ─── Допоміжні методи ──────────────────────────────────────────────────

    async def _paginate(
        self,
        stmt,
        count_stmt,
        page: int,
        size: int,
    ) -> tuple[list[Invoice], int]:
        total_result = await self.session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = stmt.offset(offset).limit(size).order_by(InvoiceModel.created_at.desc())

        result = await self.session.execute(stmt)
        models = result.scalars().all()

        return [self._to_domain(m) for m in models], total

    # ─── Маппінг ────────────────────────────────────────────────────────────

    def _to_domain(self, model: InvoiceModel | None) -> Invoice | None:
        if model is None:
            return None
        return Invoice(
            id=model.id,
            number=model.number,
            supplier_id=model.supplier_id,
            status=InvoiceStatus(model.status),
            notes=model.notes or "",
        )

    def _to_model(self, domain: Invoice) -> InvoiceModel:
        return InvoiceModel(
            id=domain.id,
            number=domain.number,
            supplier_id=domain.supplier_id,
            status=domain.status.value,
            notes=domain.notes or None,
        )

    def _update_model(self, model: InvoiceModel, domain: Invoice) -> None:
        model.number = domain.number
        model.supplier_id = domain.supplier_id
        model.status = domain.status.value
        model.notes = domain.notes or None

    async def _get_model(self, invoice_id: UUID) -> Optional[InvoiceModel]:
        result = await self.session.execute(
            select(InvoiceModel).where(InvoiceModel.id == invoice_id)
        )
        return result.scalar_one_or_none()
