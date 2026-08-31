"""
Сервіс для роботи з журналом взаєморозрахунків (SupplierLedger).

Забезпечує:
  - Створення записів при операціях (накладні, оплати, повернення)
  - Розрахунок поточного балансу постачальника
  - Перегляд історії операцій
"""

from datetime import UTC, datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from fastapi import HTTPException, status
from sqlalchemy import desc, func, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.infrastructure.persistence.models.supplier import Supplier
from app.infrastructure.persistence.models.supplier_ledger import LedgerOperationType, SupplierLedger


def _naive_dt(dt: datetime) -> datetime:
    """
    Конвертує datetime в offset-naive (без часового поясу).

    - Якщо dt з часовим поясом (offset-aware) — конвертує в UTC і прибирає tzinfo.
    - Якщо dt вже offset-naive — повертає як є.
    """
    if dt.tzinfo is not None:
        return dt.astimezone(UTC).replace(tzinfo=None)
    return dt


class LedgerService:
    """
    Сервіс для управління взаєморозрахунками з постачальниками.

    Кожна операція (прибуткова накладна, оплата, повернення)
    створює запис у журналі та оновлює баланс постачальника.
    """

    def __init__(self, session: AsyncSession):
        """Ініціалізація сервісу з асинхронною сесією БД."""
        self.session = session

    # ─── Створення запису ────────────────────────────────────────────────────

    async def create_ledger_entry(
        self,
        supplier_id: UUID,
        operation_type: str,
        amount: Decimal,
        operation_date: datetime,
        document_id: Optional[UUID] = None,
        document_number: Optional[str] = None,
        notes: Optional[str] = None,
    ) -> SupplierLedger:
        """
        Створює новий запис у журналі взаєморозрахунків.

        Args:
            supplier_id: ID постачальника.
            operation_type: Тип операції (invoice, payment, return, correction).
            amount: Сума операції (додатна — збільшення боргу, від'ємна — зменшення).
            operation_date: Дата операції.
            document_id: ID документа (опціонально).
            document_number: Номер документа (опціонально).
            notes: Нотатки (опціонально).

        Returns:
            Створений об'єкт SupplierLedger.

        Raises:
            HTTPException 404: Якщо постачальника не знайдено.
        """
        # Перевіряємо існування постачальника
        result = await self.session.execute(
            select(Supplier).where(Supplier.id == supplier_id)
        )
        supplier = result.scalar_one_or_none()
        if not supplier:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Постачальника з ID '{supplier_id}' не знайдено",
            )

        # Валідуємо тип операції
        try:
            op_type = LedgerOperationType(operation_type)
        except ValueError:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Невідомий тип операції: '{operation_type}'",
            )

        # Конвертуємо amount в Decimal (може прийти як str, float, int)
        if not isinstance(amount, Decimal):
            amount = Decimal(str(amount))

        # Конвертуємо operation_date в offset-naive (БД очікує TIMESTAMP WITHOUT TIME ZONE)
        operation_date = _naive_dt(operation_date)

        # Отримуємо поточний баланс
        current_balance = await self.get_supplier_balance(supplier_id)

        # Розраховуємо новий баланс
        balance_after = current_balance + amount

        # Створюємо запис
        entry = SupplierLedger(
            supplier_id=supplier_id,
            operation_type=op_type,
            document_id=document_id,
            document_number=document_number,
            amount=amount,
            balance_after=balance_after,
            operation_date=operation_date,
            notes=notes,
        )
        self.session.add(entry)
        await self.session.flush()
        return entry

    # ─── Отримання балансу ───────────────────────────────────────────────────

    async def get_supplier_balance(self, supplier_id: UUID) -> Decimal:
        """
        Отримує поточний баланс постачальника.

        Баланс розраховується як сума всіх операцій.
        Додатне значення — борг перед постачальником.

        Args:
            supplier_id: ID постачальника.

        Returns:
            Поточний баланс (Decimal).
        """
        result = await self.session.execute(
            select(func.coalesce(func.sum(SupplierLedger.amount), 0)).where(
                SupplierLedger.supplier_id == supplier_id
            )
        )
        balance = result.scalar() or Decimal("0.00")
        return Decimal(str(balance))

    # ─── Історія операцій ────────────────────────────────────────────────────

    async def get_ledger_history(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[SupplierLedger], int]:
        """
        Отримує історію операцій для постачальника з пагінацією.

        Args:
            supplier_id: ID постачальника.
            page: Номер сторінки.
            size: Розмір сторінки.

        Returns:
            Кортеж (список записів, загальна кількість).
        """
        # Перевіряємо існування постачальника
        result = await self.session.execute(
            select(Supplier).where(Supplier.id == supplier_id)
        )
        if not result.scalar_one_or_none():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Постачальника з ID '{supplier_id}' не знайдено",
            )

        # Загальна кількість
        count_result = await self.session.execute(
            select(func.count(SupplierLedger.id)).where(
                SupplierLedger.supplier_id == supplier_id
            )
        )
        total = count_result.scalar() or 0

        # Запит з пагінацією
        offset = (page - 1) * size
        query = (
            select(SupplierLedger)
            .where(SupplierLedger.supplier_id == supplier_id)
            .order_by(desc(SupplierLedger.operation_date))
            .offset(offset)
            .limit(size)
        )
        result = await self.session.execute(query)
        entries = list(result.scalars().all())

        return entries, total

    # ─── Отримання балансу з назвою постачальника ────────────────────────────

    async def get_supplier_balance_with_name(
        self,
        supplier_id: UUID,
    ) -> tuple[Decimal, str, Optional[datetime]]:
        """
        Отримує баланс постачальника разом з назвою та датою останньої операції.

        Args:
            supplier_id: ID постачальника.

        Returns:
            Кортеж (баланс, назва постачальника, дата останньої операції).
        """
        # Отримуємо постачальника
        result = await self.session.execute(
            select(Supplier).where(Supplier.id == supplier_id)
        )
        supplier = result.scalar_one_or_none()
        if not supplier:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Постачальника з ID '{supplier_id}' не знайдено",
            )

        # Отримуємо баланс
        balance = await self.get_supplier_balance(supplier_id)

        # Отримуємо дату останньої операції
        last_op_result = await self.session.execute(
            select(SupplierLedger.operation_date)
            .where(SupplierLedger.supplier_id == supplier_id)
            .order_by(desc(SupplierLedger.operation_date))
            .limit(1)
        )
        last_date = last_op_result.scalar_one_or_none()

        return balance, supplier.name, last_date
