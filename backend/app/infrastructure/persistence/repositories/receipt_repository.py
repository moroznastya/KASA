"""
Repository Implementation: SQLAlchemyReceiptRepository.

Реалізація IReceiptRepository з використанням SQLAlchemy.
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.repositories import IReceiptRepository
from app.infrastructure.persistence.models.receipt import (
    Receipt,
    ReceiptItem,
    ReceiptPaymentMethod,
)


class SQLAlchemyReceiptRepository(IReceiptRepository):
    """
    SQLAlchemy реалізація репозиторію чеків продажу.

    Працює з моделями Receipt та ReceiptItem.
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, receipt: Receipt) -> Receipt:
        """Зберігає новий чек."""
        self._session.add(receipt)
        await self._session.flush()
        return receipt

    async def find_by_id(self, receipt_id: UUID) -> Optional[Receipt]:
        """Знаходить чек за ID."""
        stmt = select(Receipt).where(Receipt.id == receipt_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_number(self, number: str) -> Optional[Receipt]:
        """Знаходить чек за номером."""
        stmt = select(Receipt).where(Receipt.receipt_number == number)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        """Знаходить чеки за діапазоном дат з пагінацією."""
        base_stmt = select(Receipt).where(
            Receipt.created_at >= date_from,
            Receipt.created_at <= date_to,
        )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = base_stmt.offset(offset).limit(size).order_by(Receipt.created_at.desc())
        result = await self._session.execute(stmt)
        receipts = list(result.scalars().all())

        return receipts, total

    async def search(
        self,
        query: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        payment_method: Optional[str] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        """Розширений пошук чеків."""
        base_stmt = select(Receipt)

        if query:
            like_pattern = f"%{query}%"
            base_stmt = base_stmt.where(
                or_(
                    Receipt.receipt_number.ilike(like_pattern),
                    Receipt.notes.ilike(like_pattern),
                )
            )
        if date_from is not None:
            base_stmt = base_stmt.where(Receipt.created_at >= date_from)
        if date_to is not None:
            base_stmt = base_stmt.where(Receipt.created_at <= date_to)
        if payment_method is not None:
            base_stmt = base_stmt.where(
                Receipt.payment_method == ReceiptPaymentMethod(payment_method)
            )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = base_stmt.offset(offset).limit(size).order_by(Receipt.created_at.desc())
        result = await self._session.execute(stmt)
        receipts = list(result.scalars().all())

        return receipts, total

    async def delete(self, receipt_id: UUID) -> None:
        """Видаляє чек за ID."""
        receipt = await self.find_by_id(receipt_id)
        if receipt is not None:
            await self._session.delete(receipt)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість чеків."""
        stmt = select(func.count()).select_from(Receipt)
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def get_daily_total(self, date: datetime) -> float:
        """
        Повертає загальну суму продажів за день.

        Враховує тільки чеки продажу (sale), без повернень.
        """
        start_of_day = datetime(date.year, date.month, date.day)
        end_of_day = datetime(
            date.year, date.month, date.day, 23, 59, 59
        )

        stmt = select(func.coalesce(func.sum(Receipt.total_amount), 0)).where(
            Receipt.created_at >= start_of_day,
            Receipt.created_at <= end_of_day,
            Receipt.is_return.is_(False),
        )
        result = await self._session.execute(stmt)
        return float(result.scalar() or 0.0)
