"""
Repository Implementation: SQLAlchemyLedgerRepository.

Реалізація ILedgerRepository з використанням SQLAlchemy.

Оптимізація N+1:
  - ledger → supplier (to-one) → joinedload (LEFT OUTER JOIN, 1 запит)
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import joinedload

from app.domain.repositories import ILedgerRepository
from app.infrastructure.persistence.models.supplier import Supplier
from app.infrastructure.persistence.models.supplier_ledger import (
    LedgerOperationType,
    SupplierLedger,
)

# Спільні опції eager-loading для записів журналу
_LEDGER_OPTIONS = (
    joinedload(SupplierLedger.supplier),
)


class SQLAlchemyLedgerRepository(ILedgerRepository):
    """
    SQLAlchemy реалізація репозиторію журналу взаєморозрахунків.

    Працює з моделями SupplierLedger та Supplier.
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, entry: SupplierLedger) -> SupplierLedger:
        """Зберігає новий запис у журналі."""
        self._session.add(entry)
        await self._session.flush()
        return entry

    async def find_by_id(self, entry_id: UUID) -> Optional[SupplierLedger]:
        """Знаходить запис за ID (з постачальником)."""
        stmt = (
            select(SupplierLedger)
            .where(SupplierLedger.id == entry_id)
            .options(*_LEDGER_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_supplier(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[SupplierLedger], int]:
        """Знаходить записи для постачальника (з постачальником)."""
        base_stmt = select(SupplierLedger).where(
            SupplierLedger.supplier_id == supplier_id
        )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_LEDGER_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(SupplierLedger.operation_date.desc())
        )
        result = await self._session.execute(stmt)
        entries = list(result.scalars().all())

        return entries, total

    async def find_by_operation_type(
        self,
        operation_type: LedgerOperationType,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[SupplierLedger], int]:
        """Знаходить записи за типом операції (з постачальником)."""
        base_stmt = select(SupplierLedger).where(
            SupplierLedger.operation_type == operation_type
        )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_LEDGER_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(SupplierLedger.operation_date.desc())
        )
        result = await self._session.execute(stmt)
        entries = list(result.scalars().all())

        return entries, total

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[SupplierLedger], int]:
        """Знаходить записи за діапазоном дат (з постачальником)."""
        base_stmt = select(SupplierLedger).where(
            SupplierLedger.operation_date >= date_from,
            SupplierLedger.operation_date <= date_to,
        )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_LEDGER_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(SupplierLedger.operation_date.desc())
        )
        result = await self._session.execute(stmt)
        entries = list(result.scalars().all())

        return entries, total

    async def search(
        self,
        supplier_id: Optional[UUID] = None,
        operation_type: Optional[LedgerOperationType] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[SupplierLedger], int]:
        """Розширений пошук записів журналу (з постачальником)."""
        base_stmt = select(SupplierLedger)

        if supplier_id is not None:
            base_stmt = base_stmt.where(
                SupplierLedger.supplier_id == supplier_id
            )
        if operation_type is not None:
            base_stmt = base_stmt.where(
                SupplierLedger.operation_type == operation_type
            )
        if date_from is not None:
            base_stmt = base_stmt.where(
                SupplierLedger.operation_date >= date_from
            )
        if date_to is not None:
            base_stmt = base_stmt.where(
                SupplierLedger.operation_date <= date_to
            )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_LEDGER_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(SupplierLedger.operation_date.desc())
        )
        result = await self._session.execute(stmt)
        entries = list(result.scalars().all())

        return entries, total

    async def get_supplier_balance(self, supplier_id: UUID) -> float:
        """
        Повертає поточний баланс постачальника.

        Бере останній balance_after для даного постачальника.
        """
        stmt = (
            select(SupplierLedger.balance_after)
            .where(SupplierLedger.supplier_id == supplier_id)
            .order_by(SupplierLedger.operation_date.desc())
            .limit(1)
        )
        result = await self._session.execute(stmt)
        balance = result.scalar_one_or_none()
        return float(balance) if balance is not None else 0.0

    async def get_all_supplier_balances(self) -> list[dict]:
        """
        Повертає баланси всіх постачальників.

        Використовує підзапит для отримання останнього запису кожного
        постачальника.
        """
        # Підзапит для останнього запису по кожному постачальнику
        latest_entry_subq = (
            select(
                SupplierLedger.supplier_id,
                func.max(SupplierLedger.operation_date).label("max_date"),
            )
            .group_by(SupplierLedger.supplier_id)
            .subquery()
        )

        # Основний запит: останній balance_after + ім'я постачальника
        stmt = (
            select(
                Supplier.id.label("supplier_id"),
                Supplier.name.label("supplier_name"),
                SupplierLedger.balance_after.label("balance"),
                SupplierLedger.operation_date.label("last_operation_date"),
            )
            .join(
                latest_entry_subq,
                Supplier.id == latest_entry_subq.c.supplier_id,
            )
            .join(
                SupplierLedger,
                (SupplierLedger.supplier_id == latest_entry_subq.c.supplier_id)
                & (
                    SupplierLedger.operation_date
                    == latest_entry_subq.c.max_date
                ),
            )
            .order_by(Supplier.name)
        )
        result = await self._session.execute(stmt)
        rows = result.fetchall()

        return [
            {
                "supplier_id": row.supplier_id,
                "supplier_name": row.supplier_name,
                "balance": float(row.balance) if row.balance else 0.0,
                "last_operation_date": row.last_operation_date,
            }
            for row in rows
        ]

    async def delete(self, entry_id: UUID) -> None:
        """Видаляє запис за ID."""
        entry = await self.find_by_id(entry_id)
        if entry is not None:
            await self._session.delete(entry)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість записів."""
        stmt = select(func.count()).select_from(SupplierLedger)
        result = await self._session.execute(stmt)
        return result.scalar() or 0
