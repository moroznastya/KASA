"""
Pydantic схеми для моделей ReturnInvoice та ReturnInvoiceItem (Повернення постачальнику).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.models.return_invoice import ReturnInvoiceStatus


class ReturnInvoiceItemCreate(BaseModel):
    """Схема створення позиції повернення."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    price: Decimal = Field(..., max_digits=10, decimal_places=2, description="Ціна за одиницю (грн)")
    total: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума (грн)")


class ReturnInvoiceItemResponse(BaseModel):
    """Схема відповіді з даними позиції повернення."""
    id: UUID
    return_invoice_id: UUID
    product_id: UUID
    quantity: Decimal
    price: Decimal
    total: Decimal
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class ReturnInvoiceCreate(BaseModel):
    """Схема створення нового повернення постачальнику."""
    number: str = Field(..., max_length=50, description="Номер документа")
    supplier_id: UUID = Field(..., description="ID постачальника")
    return_date: datetime = Field(..., description="Дата повернення")
    notes: Optional[str] = Field(None, description="Причина повернення / нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: list[ReturnInvoiceItemCreate] = Field(default_factory=list, description="Позиції повернення")


class ReturnInvoiceUpdate(BaseModel):
    """Схема оновлення повернення. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа")
    supplier_id: Optional[UUID] = Field(None, description="ID постачальника")
    return_date: Optional[datetime] = Field(None, description="Дата повернення")
    notes: Optional[str] = Field(None, description="Причина повернення / нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: Optional[list[ReturnInvoiceItemCreate]] = Field(None, description="Позиції повернення")


class ReturnInvoiceResponse(BaseModel):
    """Схема відповіді з даними повернення."""
    id: UUID
    number: str
    supplier_id: UUID
    return_date: datetime
    status: ReturnInvoiceStatus
    notes: Optional[str] = None
    total_amount: Optional[Decimal] = None
    created_at: datetime
    updated_at: datetime
    items: list[ReturnInvoiceItemResponse] = []

    model_config = ConfigDict(from_attributes=True)


class ReturnInvoiceConfirmRequest(BaseModel):
    """Схема підтвердження повернення (зміна статусу)."""
    status: ReturnInvoiceStatus = Field(..., description="Новий статус (confirmed / cancelled)")
