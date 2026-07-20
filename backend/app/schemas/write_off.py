"""
Pydantic схеми для моделей WriteOff та WriteOffItem (Списання товару).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.models.write_off import WriteOffReason


class WriteOffItemCreate(BaseModel):
    """Схема створення позиції списання."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")


class WriteOffItemResponse(BaseModel):
    """Схема відповіді з даними позиції списання."""
    id: UUID
    write_off_id: UUID
    product_id: UUID
    quantity: Decimal
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class WriteOffCreate(BaseModel):
    """Схема створення нового списання."""
    number: str = Field(..., max_length=50, description="Номер документа")
    reason: WriteOffReason = Field(..., description="Причина списання")
    write_off_date: datetime = Field(..., description="Дата списання")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: list[WriteOffItemCreate] = Field(default_factory=list, description="Позиції списання")


class WriteOffUpdate(BaseModel):
    """Схема оновлення списання. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа")
    reason: Optional[WriteOffReason] = Field(None, description="Причина списання")
    write_off_date: Optional[datetime] = Field(None, description="Дата списання")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: Optional[list[WriteOffItemCreate]] = Field(None, description="Позиції списання")


class WriteOffResponse(BaseModel):
    """Схема відповіді з даними списання."""
    id: UUID
    number: str
    reason: WriteOffReason
    write_off_date: datetime
    notes: Optional[str] = None
    status: str = "confirmed"
    total_amount: Optional[Decimal] = None
    created_at: datetime
    updated_at: datetime
    items: list[WriteOffItemResponse] = []

    model_config = ConfigDict(from_attributes=True)
