"""
Pydantic схеми для моделі Debtor (Боржник).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict


class DebtorCreate(BaseModel):
    """Схема створення боржника."""
    name: str = Field(..., min_length=1, max_length=255, description="Ім'я боржника")
    phone: Optional[str] = Field(None, max_length=50, description="Номер телефону")
    notes: Optional[str] = Field(None, description="Нотатки")


class DebtorUpdate(BaseModel):
    """Схема оновлення боржника."""
    name: Optional[str] = Field(None, min_length=1, max_length=255, description="Ім'я боржника")
    phone: Optional[str] = Field(None, max_length=50, description="Номер телефону")
    notes: Optional[str] = Field(None, description="Нотатки")


class DebtorPayRequest(BaseModel):
    """Схема запиту на погашення боргу."""
    amount: Decimal = Field(..., gt=0, max_digits=12, decimal_places=2, description="Сума для погашення")


class DebtorResponse(BaseModel):
    """Схема відповіді з даними боржника."""
    id: UUID
    name: str
    phone: Optional[str] = None
    notes: Optional[str] = None
    total_debt: Decimal = Decimal("0.00")
    created_at: datetime
    updated_at: datetime

    model_config = ConfigDict(from_attributes=True)


class DebtorSearchParams(BaseModel):
    """Параметри пошуку боржників."""
    query: str = Field(..., min_length=1, description="Пошуковий запит")
