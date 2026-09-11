"""
Pydantic схеми для моделей Inventory та InventoryItem (Інвентаризація).
"""

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field, field_validator

from app.infrastructure.persistence.models.inventory import InventoryStatus


class ProductBrief(BaseModel):
    """Коротка інформація про товар для відповіді."""
    id: UUID
    title: str
    barcode: Optional[str] = None

    model_config = ConfigDict(from_attributes=True)


class InventoryItemCreate(BaseModel):
    """Схема створення позиції інвентаризації."""
    product_id: UUID = Field(..., description="ID товару")
    actual_quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Фактична кількість")
    accounting_quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Облікова кількість")
    difference: Decimal = Field(..., max_digits=10, decimal_places=3, description="Різниця (actual - accounting)")
    cost_price: Decimal = Field(..., max_digits=12, decimal_places=2, description="Собівартість одиниці товару")
    price: Decimal = Field(..., max_digits=12, decimal_places=2, description="Ціна продажу одиниці товару")


class InventoryItemResponse(BaseModel):
    """Схема відповіді з даними позиції інвентаризації."""
    id: UUID
    inventory_id: UUID
    product_id: UUID
    product: Optional[ProductBrief] = Field(None, description="Інформація про товар")
    actual_quantity: Decimal
    accounting_quantity: Decimal
    difference: Decimal
    cost_price: Decimal
    price: Decimal
    total_cost: Decimal = Field(default=0, max_digits=14, decimal_places=2, description="Сума собівартості (actual_quantity * cost_price)")
    total_selling: Decimal = Field(default=0, max_digits=14, decimal_places=2, description="Сума продажу (actual_quantity * price)")
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class InventorySummary(BaseModel):
    """Підсумки інвентаризації."""
    total_cost: Decimal = Field(default=0, description="Загальна сума собівартості (∑ actual_quantity * cost_price)")
    total_selling: Decimal = Field(default=0, description="Загальна сума продажу (∑ actual_quantity * price)")
    total_deviation: Decimal = Field(default=0, description="Загальна сума відхилення (∑ difference * cost_price)")


class InventoryCreate(BaseModel):
    """Схема створення нової інвентаризації."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа (якщо не вказано — генерується автоматично)")
    location: Optional[str] = Field(None, max_length=255, description="Локація (магазин/склад)")
    inventory_date: datetime = Field(..., description="Дата інвентаризації")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: list[InventoryItemCreate] = Field(default_factory=list, description="Позиції інвентаризації")

    @field_validator('inventory_date', mode='after')
    @classmethod
    def remove_timezone(cls, v: datetime) -> datetime:
        """Прибирає timezone з datetime для сумісності з TIMESTAMP WITHOUT TIME ZONE."""
        if v.tzinfo is not None:
            return v.replace(tzinfo=None)
        return v


class InventoryUpdate(BaseModel):
    """Схема оновлення інвентаризації. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа")
    location: Optional[str] = Field(None, max_length=255, description="Локація")
    inventory_date: Optional[datetime] = Field(None, description="Дата інвентаризації")
    notes: Optional[str] = Field(None, description="Нотатки")
    items: Optional[list[InventoryItemCreate]] = Field(None, description="Позиції інвентаризації")

    @field_validator('inventory_date', mode='after')
    @classmethod
    def remove_timezone(cls, v: Optional[datetime]) -> Optional[datetime]:
        """Прибирає timezone з datetime для сумісності з TIMESTAMP WITHOUT TIME ZONE."""
        if v is not None and v.tzinfo is not None:
            return v.replace(tzinfo=None)
        return v


class InventoryResponse(BaseModel):
    """Схема відповіді з даними інвентаризації."""
    id: UUID
    number: str
    location: str
    inventory_date: datetime
    status: InventoryStatus
    notes: Optional[str] = None
    created_at: datetime
    updated_at: datetime
    items: list[InventoryItemResponse] = []
    summary: InventorySummary = Field(default_factory=InventorySummary, description="Підсумки")

    model_config = ConfigDict(from_attributes=True)


class InventoryConfirmRequest(BaseModel):
    """Схема підтвердження інвентаризації."""
    status: InventoryStatus = Field(..., description="Новий статус (confirmed / cancelled)")
