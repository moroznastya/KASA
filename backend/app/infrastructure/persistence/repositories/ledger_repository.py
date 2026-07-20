"""
Infrastructure Layer: LedgerRepository — реалізація ILedgerRepository.

Використовує SQLAlchemy ORM модель SupplierLedgerModel для роботи з БД.
"""

from __future__ import annotations

import logging
from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, and_, desc
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.entities.ledger_entry import LedgerEntry, OperationType
from app.domain.repositories.i_ledger_repository import ILedgerRepository
from app.infrastructure.persistence.models import SupplierLedgerModel, SupplierModel

logger = logging.getLogger(__name__)


class LedgerRepository(ILedgerRepository):
    """
    Репозиторій журналу взаєморозрахунків.

    Реалізує ILedgerRepository використовуючи SQLAlchemy ORM.
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

    async def save(self, entry: LedgerEntry) -> LedgerEntry:
        model = self._to_model(entry)
        self.session.add(model)
        await self.session.flush()
        return self._to_domain(model)

    async def find_by_id(self, entry_id: UUID) -> Optional[LedgerEntry]:
        result = await self.session.execute(
            select(SupplierLedgerModel).where(SupplierLedgerModel.id == entry_id)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_by_supplier(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        stmt = select(SupplierLedgerModel).where(
            SupplierLedgerModel.supplier_id == supplier_id
        )
        count_stmt = select(func.count(SupplierLedgerModel.id)).where(
            SupplierLedgerModel.supplier_id == supplier_id
        )
        return await self._paginate(stmt, count_stmt, page, size)

    async def find_by_operation_type(
        self,
        operation_type: OperationType,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        op_str = operation_type.value
        stmt = select(SupplierLedgerModel).where(
            SupplierLedgerModel.operation_type == op_str
        )
        count_stmt = select(func.count(SupplierLedgerModel.id)).where(
            SupplierLedgerModel.operation_type == op_str
        )
        return await self._paginate(stmt, count_stmt, page, size)

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        stmt = select(SupplierLedgerModel).where(
            and_(
                SupplierLedgerModel.operation_date >= date_from,
                SupplierLedgerModel.operation_date <= date_to,
            )
        )
        count_stmt = select(func.count(SupplierLedgerModel.id)).where(
            and_(
                SupplierLedgerModel.operation_date >= date_from,
                SupplierLedgerModel.operation_date <= date_to,
            )
        )
        return await self._paginate(stmt, count_stmt, page, size)

    async def search(
        self,
        supplier_id: Optional[UUID] = None,
        operation_type: Optional[OperationType] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntry], int]:
        stmt = select(SupplierLedgerModel)
        count_stmt = select(func.count(SupplierLedgerModel.id))

        conditions = []
        if supplier_id:
            conditions.append(SupplierLedgerModel.supplier_id == supplier_id)
        if operation_type:
            conditions.append(
                SupplierLedgerModel.operation_type == operation_type.value
            )
        if date_from:
            conditions.append(SupplierLedgerModel.operation_date >= date_from)
        if date_to:
            conditions.append(SupplierLedgerModel.operation_date <= date_to)

        if conditions:
            stmt = stmt.where(and_(*conditions))
            count_stmt = count_stmt.where(and_(*conditions))

        return await self._paginate(stmt, count_stmt, page, size)

    async def get_supplier_balance(self, supplier_id: UUID) -> float:
        result = await self.session.execute(
            select(func.coalesce(func.sum(SupplierLedgerModel.amount), 0)).where(
                SupplierLedgerModel.supplier_id == supplier_id
            )
        )
        return float(result.scalar() or 0.0)

    async def get_all_supplier_balances(self) -> list[dict]:
        # Отримуємо всіх активних постачальників
        suppliers_result = await self.session.execute(
            select(SupplierModel).where(SupplierModel.is_active == True)
        )
        suppliers = suppliers_result.scalars().all()

        balances = []
        for supplier in suppliers:
            balance = await self.get_supplier_balance(supplier.id)

            # Отримуємо дату останньої операції
            last_op_result = await self.session.execute(
                select(SupplierLedgerModel.operation_date)
                .where(SupplierLedgerModel.supplier_id == supplier.id)
                .order_by(desc(SupplierLedgerModel.operation_date))
                .limit(1)
            )
            last_op_date = last_op_result.scalar_one_or_none()

            balances.append({
                "supplier_id": supplier.id,
                "supplier_name": supplier.name,
                "balance": balance,
                "last_operation_date": last_op_date,
            })

        return balances

    async def delete(self, entry_id: UUID) -> None:
        result = await self.session.execute(
            select(SupplierLedgerModel).where(SupplierLedgerModel.id == entry_id)
        )
        model = result.scalar_one_or_none()
        if model:
            await self.session.delete(model)
            await self.session.flush()

    async def count(self) -> int:
        result = await self.session.execute(
            select(func.count(SupplierLedgerModel.id))
        )
        return result.scalar() or 0

    # ─── Допоміжні методи ──────────────────────────────────────────────────

    async def _paginate(
        self,
        stmt,
        count_stmt,
        page: int,
        size: int,
    ) -> tuple[list[LedgerEntry], int]:
        total_result = await self.session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            stmt.offset(offset)
            .limit(size)
            .order_by(desc(SupplierLedgerModel.operation_date))
        )

        result = await self.session.execute(stmt)
        models = result.scalars().all()

        return [self._to_domain(m) for m in models], total

    # ─── Маппінг ────────────────────────────────────────────────────────────

    def _to_domain(self, model: SupplierLedgerModel | None) -> LedgerEntry | None:
        if model is None:
            return None
        return LedgerEntry(
            id=model.id,
            supplier_id=model.supplier_id,
            operation_type=OperationType(model.operation_type),
            amount=float(model.amount or 0),
            balance_after=float(model.balance_after or 0),
            document_id=model.document_id,
            document_number=model.document_number or "",
            notes=model.notes or "",
        )

    def _to_model(self, domain: LedgerEntry) -> SupplierLedgerModel:
        return SupplierLedgerModel(
            id=domain.id,
            supplier_id=domain.supplier_id,
            operation_type=domain.operation_type.value,
            amount=domain.amount,
            balance_after=domain.balance_after,
            document_id=domain.document_id,
            document_number=domain.document_number or None,
            notes=domain.notes or None,
        )
