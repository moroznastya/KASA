"""
Pydantic схеми для моделі Supplier (Постачальник).
"""

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class SupplierCreate(BaseModel):
    """Схема створення нового постачальника."""
    name: str = Field(..., max_length=255, description="Назва постачальника")
    edrpou: Optional[str] = Field(None, max_length=10, description="Код ЄДРПОУ / ІПН")
    phone: Optional[str] = Field(None, max_length=20, description="Номер телефону")
    email: Optional[str] = Field(None, max_length=255, description="Електронна пошта")
    address: Optional[str] = Field(None, description="Юридична / фактична адреса")
    notes: Optional[str] = Field(None, description="Додаткові нотатки")


class SupplierUpdate(BaseModel):
    """Схема оновлення постачальника. Всі поля опціональні."""
    name: Optional[str] = Field(None, max_length=255, description="Назва постачальника")
    edrpou: Optional[str] = Field(None, max_length=10, description="Код ЄДРПОУ / ІПН")
    phone: Optional[str] = Field(None, max_length=20, description="Номер телефону")
    email: Optional[str] = Field(None, max_length=255, description="Електронна пошта")
    address: Optional[str] = Field(None, description="Юридична / фактична адреса")
    notes: Optional[str] = Field(None, description="Додаткові нотатки")


class SupplierResponse(BaseModel):
    """Схема відповіді з даними постачальника."""
    id: UUID
    name: str
    edrpou: Optional[str] = None
    phone: Optional[str] = None
    email: Optional[str] = None
    address: Optional[str] = None
    notes: Optional[str] = None
    current_balance: Decimal = Field(Decimal("0.00"), description="Поточний борг перед постачальником (грн)")
    created_at: datetime
    updated_at: datetime

    model_config = ConfigDict(from_attributes=True)
