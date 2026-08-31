"""
Pydantic схеми для моделей WriteOff та WriteOffItem (Списання товару).

reason — рядок: назва причини з персистентного довідника write_off_reasons.
"""

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class WriteOffItemCreate(BaseModel):
    """Схема створення позиції списання."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    cost_price: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Собівартість одиниці товару")
    price: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Ціна продажу одиниці товару")


class WriteOffItemResponse(BaseModel):
    """Схема відповіді з даними позиції списання."""
    id: UUID
    write_off_id: UUID
    product_id: UUID
    quantity: Decimal
    cost_price: Decimal = Field(default=0, description="Собівартість одиниці товару")
    price: Decimal = Field(default=0, description="Ціна продажу одиниці товару")
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class WriteOffCreate(BaseModel):
    """Схема створення нового списання."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа (якщо не вказано — генерується автоматично)")
    reason: str = Field(..., min_length=2, max_length=100, description="Причина списання (назва з довідника write_off_reasons)")
    write_off_date: datetime = Field(..., description="Дата списання")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: list[WriteOffItemCreate] = Field(default_factory=list, description="Позиції списання")


class WriteOffUpdate(BaseModel):
    """Схема оновлення списання. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа")
    reason: Optional[str] = Field(None, min_length=2, max_length=100, description="Причина списання (назва з довідника)")
    write_off_date: Optional[datetime] = Field(None, description="Дата списання")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: Optional[list[WriteOffItemCreate]] = Field(None, description="Позиції списання")


class WriteOffResponse(BaseModel):
    """Схема відповіді з даними списання."""
    id: UUID
    number: str
    reason: str
    write_off_date: datetime
    notes: Optional[str] = None
    status: str = "confirmed"
    total_amount: Optional[Decimal] = None
    created_at: datetime
    updated_at: datetime
    items: list[WriteOffItemResponse] = []

    model_config = ConfigDict(from_attributes=True)
