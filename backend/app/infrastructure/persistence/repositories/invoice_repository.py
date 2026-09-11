"""
Repository Implementation: SQLAlchemyInvoiceRepository.

Реалізація IInvoiceRepository з використанням SQLAlchemy.

Оптимізація N+1:
  - invoice → items (to-many)        → selectinload
  - invoice → items → product (to-one) → joinedload (вкладений)
  - invoice → supplier / creator (to-one) → joinedload
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlalchemy import func, or_, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import joinedload, selectinload

from app.domain.repositories import IInvoiceRepository
from app.infrastructure.persistence.models.invoice import (
    Invoice,
    InvoiceItem,
    InvoiceStatus,
)
from app.infrastructure.persistence.models.supplier_ledger import (
    LedgerOperationType,
    SupplierLedger,
)

# Спільні опції eager-loading для накладної з повним вмістом
_INVOICE_DETAIL_OPTIONS = (
    joinedload(Invoice.supplier),
    joinedload(Invoice.creator),
    selectinload(Invoice.items).joinedload(InvoiceItem.product),
)

# Спільні опції eager-loading для списків накладних
_INVOICE_LIST_OPTIONS = (
    joinedload(Invoice.supplier),
    selectinload(Invoice.items).joinedload(InvoiceItem.product),
)


class SQLAlchemyInvoiceRepository(IInvoiceRepository):
    """
    SQLAlchemy реалізація репозиторію прибуткових накладних.

    Працює з моделями Invoice та InvoiceItem.
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, invoice: Invoice) -> Invoice:
        """Зберігає нову накладну."""
        self._session.add(invoice)
        await self._session.flush()
        return invoice

    async def update(self, invoice: Invoice) -> Invoice:
        """Оновлює існуючу накладну."""
        merged = await self._session.merge(invoice)
        await self._session.flush()
        return merged

    async def find_by_id(self, invoice_id: UUID) -> Optional[Invoice]:
        """Знаходить накладну за ID (з позиціями, товарами, постачальником, автором)."""
        stmt = (
            select(Invoice)
            .where(Invoice.id == invoice_id)
            .options(*_INVOICE_DETAIL_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_number(self, number: str) -> Optional[Invoice]:
        """Знаходить накладну за номером (з позиціями, товарами, постачальником)."""
        stmt = (
            select(Invoice)
            .where(Invoice.number == number)
            .options(*_INVOICE_DETAIL_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_supplier(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        """Знаходить накладні постачальника з пагінацією (з позиціями та товарами)."""
        base_stmt = select(Invoice).where(Invoice.supplier_id == supplier_id)

        # Підрахунок
        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        # Пагінація
        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_INVOICE_LIST_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(Invoice.created_at.desc())
        )
        result = await self._session.execute(stmt)
        invoices = list(result.scalars().all())

        return invoices, total

    async def find_by_status(
        self,
        status: InvoiceStatus,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        """Знаходить накладні за статусом з пагінацією (з позиціями та товарами)."""
        base_stmt = select(Invoice).where(Invoice.status == status)

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_INVOICE_LIST_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(Invoice.created_at.desc())
        )
        result = await self._session.execute(stmt)
        invoices = list(result.scalars().all())

        return invoices, total

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Invoice], int]:
        """Знаходить накладні за діапазоном дат (з позиціями та товарами)."""
        base_stmt = select(Invoice).where(
            Invoice.invoice_date >= date_from,
            Invoice.invoice_date <= date_to,
        )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_INVOICE_LIST_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(Invoice.invoice_date.desc())
        )
        result = await self._session.execute(stmt)
        invoices = list(result.scalars().all())

        return invoices, total

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
        """Розширений пошук накладних з фільтрацією (з позиціями та товарами)."""
        base_stmt = select(Invoice)

        if query:
            like_pattern = f"%{query}%"
            base_stmt = base_stmt.where(
                or_(
                    Invoice.number.ilike(like_pattern),
                    Invoice.notes.ilike(like_pattern),
                )
            )
        if supplier_id is not None:
            base_stmt = base_stmt.where(Invoice.supplier_id == supplier_id)
        if status is not None:
            base_stmt = base_stmt.where(Invoice.status == status)
        if date_from is not None:
            base_stmt = base_stmt.where(Invoice.invoice_date >= date_from)
        if date_to is not None:
            base_stmt = base_stmt.where(Invoice.invoice_date <= date_to)

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_INVOICE_LIST_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(Invoice.created_at.desc())
        )
        result = await self._session.execute(stmt)
        invoices = list(result.scalars().all())

        return invoices, total

    async def delete(self, invoice_id: UUID) -> None:
        """Видаляє накладну за ID."""
        invoice = await self.find_by_id(invoice_id)
        if invoice is not None:
            await self._session.delete(invoice)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість накладних."""
        stmt = select(func.count()).select_from(Invoice)
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def get_payment_info(self, invoice_id: UUID) -> dict:
        """
        Повертає інформацію про оплату накладної.

        Рахує paid_amount як суму PAYMENT та RETURN записів у
        SupplierLedger з цим document_id (від'ємні суми беруться
        за модулем — вони зменшують борг).

        Returns:
            dict: {invoice_id, invoice_number, invoice_date, total_amount,
                   paid_amount, remaining} (Decimal значення).
        """
        from decimal import Decimal

        # Знаходимо накладну
        invoice = await self.find_by_id(invoice_id)
        if not invoice:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")

        # Сума PAYMENT + RETURN записів з цим document_id
        ledger_result = await self._session.execute(
            select(SupplierLedger.amount)
            .where(SupplierLedger.document_id == invoice_id)
            .where(
                SupplierLedger.operation_type.in_([
                    LedgerOperationType.PAYMENT,
                    LedgerOperationType.RETURN,
                ])
            )
        )
        ledger_rows = ledger_result.all()

        paid_amount = sum(abs(Decimal(str(row[0]))) for row in ledger_rows)
        paid_amount = Decimal(str(paid_amount)).quantize(Decimal("0.01"))

        total_amount = Decimal(str(invoice.total_amount or 0)).quantize(Decimal("0.01"))
        remaining = (total_amount - paid_amount).quantize(Decimal("0.01"))

        return {
            "invoice_id": invoice.id,
            "invoice_number": invoice.number,
            "invoice_date": invoice.invoice_date,
            "total_amount": total_amount,
            "paid_amount": paid_amount,
            "remaining": remaining,
        }
