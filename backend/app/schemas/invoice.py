"""
Pydantic схеми для моделей Invoice та InvoiceItem (Прибуткова накладна).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.models.invoice import InvoiceStatus


class InvoiceItemCreate(BaseModel):
    """Схема створення позиції накладної."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    price: Decimal = Field(..., max_digits=10, decimal_places=2, description="Ціна за одиницю (грн)")
    total: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума (грн)")


class InvoiceItemResponse(BaseModel):
    """Схема відповіді з даними позиції накладної."""
    id: UUID
    invoice_id: UUID
    product_id: UUID
    quantity: Decimal
    price: Decimal
    total: Decimal
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class InvoiceCreate(BaseModel):
    """Схема створення нової прибуткової накладної."""
    number: str = Field(..., max_length=50, description="Номер накладної")
    supplier_id: UUID = Field(..., description="ID постачальника")
    invoice_date: datetime = Field(..., description="Дата накладної")
    notes: Optional[str] = Field(None, description="Нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: list[InvoiceItemCreate] = Field(default_factory=list, description="Позиції накладної")


class InvoiceUpdate(BaseModel):
    """Схема оновлення накладної. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер накладної")
    supplier_id: Optional[UUID] = Field(None, description="ID постачальника")
    invoice_date: Optional[datetime] = Field(None, description="Дата накладної")
    notes: Optional[str] = Field(None, description="Нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: Optional[list[InvoiceItemCreate]] = Field(None, description="Позиції накладної")


class InvoiceResponse(BaseModel):
    """Схема відповіді з даними накладної."""
    id: UUID
    number: str
    supplier_id: UUID
    invoice_date: datetime
    status: InvoiceStatus
    notes: Optional[str] = None
    total_amount: Optional[Decimal] = None
    created_at: datetime
    updated_at: datetime
    items: list[InvoiceItemResponse] = []

    model_config = ConfigDict(from_attributes=True)


class InvoiceConfirmRequest(BaseModel):
    """Схема підтвердження накладної (зміна статусу)."""
    status: InvoiceStatus = Field(..., description="Новий статус (confirmed / cancelled)")
