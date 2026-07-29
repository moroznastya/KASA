"""
Pydantic схеми для моделі SupplierLedger (Журнал взаєморозрахунків).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.models.supplier_ledger import LedgerOperationType


class SupplierLedgerCreate(BaseModel):
    """Схема створення запису в журналі взаєморозрахунків."""
    supplier_id: UUID = Field(..., description="ID постачальника")
    operation_type: LedgerOperationType = Field(..., description="Тип операції")
    document_id: Optional[UUID] = Field(None, description="ID накладної (якщо оплата привязана до конкретної накладної)")
    document_number: Optional[str] = Field(None, max_length=50, description="Номер накладної (для відображення)")
    amount: Decimal = Field(..., max_digits=12, decimal_places=2, description="Сума операції (грн)")
    balance_after: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Баланс після операції (грн). Якщо не вказано, розраховується автоматично.")
    operation_date: datetime = Field(..., description="Дата операції")
    notes: Optional[str] = Field(None, description="Нотатки")


class SupplierLedgerResponse(BaseModel):
    """Схема відповіді з даними запису взаєморозрахунків."""
    id: UUID
    supplier_id: UUID
    operation_type: LedgerOperationType
    document_id: Optional[UUID] = None
    document_number: Optional[str] = None
    amount: Decimal
    balance_after: Decimal
    operation_date: datetime
    notes: Optional[str] = None
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class SupplierLedgerBalanceResponse(BaseModel):
    """Схема відповіді з балансом постачальника."""
    supplier_id: UUID = Field(..., description="ID постачальника")
    supplier_name: str = Field(..., description="Назва постачальника")
    current_balance: Decimal = Field(..., description="Поточний баланс (грн)")
    last_updated: Optional[datetime] = Field(None, description="Дата останньої операції")
