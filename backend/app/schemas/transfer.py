"""
Pydantic схеми для моделей Transfer та TransferItem (Переміщення товару).
"""

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field

from app.infrastructure.persistence.models.transfer import TransferStatus


class TransferItemCreate(BaseModel):
    """Схема створення позиції переміщення."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    cost_price: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Собівартість одиниці товару")
    price: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Ціна продажу одиниці товару")


class TransferItemResponse(BaseModel):
    """Схема відповіді з даними позиції переміщення."""
    id: UUID
    transfer_id: UUID
    product_id: UUID
    quantity: Decimal
    cost_price: Decimal = Field(default=0, description="Собівартість одиниці товару")
    price: Decimal = Field(default=0, description="Ціна продажу одиниці товару")
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class TransferCreate(BaseModel):
    """Схема створення нового переміщення."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа (якщо не вказано — генерується автоматично)")
    from_location: str = Field(..., max_length=255, description="Звідки переміщуємо")
    to_location: str = Field(..., max_length=255, description="Куди переміщуємо")
    transfer_date: datetime = Field(..., description="Дата переміщення")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: list[TransferItemCreate] = Field(default_factory=list, description="Позиції переміщення")


class TransferUpdate(BaseModel):
    """Схема оновлення переміщення. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа")
    from_location: Optional[str] = Field(None, max_length=255, description="Звідки переміщуємо")
    to_location: Optional[str] = Field(None, max_length=255, description="Куди переміщуємо")
    transfer_date: Optional[datetime] = Field(None, description="Дата переміщення")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: Optional[list[TransferItemCreate]] = Field(None, description="Позиції переміщення")


class TransferResponse(BaseModel):
    """Схема відповіді з даними переміщення."""
    id: UUID
    number: str
    from_location: str
    to_location: str
    transfer_date: datetime
    status: TransferStatus
    notes: Optional[str] = None
    created_at: datetime
    updated_at: datetime
    items: list[TransferItemResponse] = []

    model_config = ConfigDict(from_attributes=True)


class TransferConfirmRequest(BaseModel):
    """Схема підтвердження переміщення (зміна статусу)."""
    status: TransferStatus = Field(..., description="Новий статус (confirmed / cancelled)")
