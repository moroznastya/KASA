"""
Pydantic схеми для моделей Receipt та ReceiptItem (Чек продажу / повернення).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.models.receipt import ReceiptType


class ReceiptItemCreate(BaseModel):
    """Схема створення позиції чеку."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    price: Decimal = Field(..., max_digits=10, decimal_places=2, description="Ціна за одиницю (грн)")
    total: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума (грн)")


class ReceiptItemResponse(BaseModel):
    """Схема відповіді з даними позиції чеку."""
    id: UUID
    receipt_id: UUID
    product_id: UUID
    product_name: str = Field("", description="Назва товару")
    quantity: Decimal
    price: Decimal
    total: Decimal
    purchase_price: Optional[Decimal] = Field(None, description="Собівартість товару на момент продажу")
    profit: Optional[Decimal] = Field(None, description="Прибуток = total - (purchase_price * quantity)")
    vat_amount: Optional[Decimal] = Field(None, description="Сума ПДВ для цієї позиції")
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class ReceiptCreate(BaseModel):
    """Схема створення нового чеку (продаж або повернення)."""
    receipt_number: Optional[str] = Field(None, max_length=50, description="Номер чеку")
    receipt_type: ReceiptType = Field(ReceiptType.SALE, description="Тип чеку: sale або return")
    cashier_id: Optional[UUID] = Field(None, description="ID касира")
    total_amount: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума чеку (грн)")
    paid_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Фактично сплачена сума (грн)")
    debtor_id: Optional[UUID] = Field(None, description="ID боржника (якщо покупка в борг)")
    is_return: bool = Field(False, description="Чи є поверненням")
    notes: Optional[str] = Field(None, description="Нотатки до чеку")
    items: list[ReceiptItemCreate] = Field(default_factory=list, description="Позиції чеку")


class ReceiptResponse(BaseModel):
    """Схема відповіді з даними чеку."""
    id: UUID
    receipt_number: str
    receipt_type: ReceiptType
    cashier_id: UUID
    total_amount: Decimal
    paid_amount: Optional[Decimal] = Field(None, description="Фактично сплачена сума")
    debtor_id: Optional[UUID] = Field(None, description="ID боржника")
    is_return: bool
    notes: Optional[str] = None
    created_at: datetime
    items: list[ReceiptItemResponse] = []
    total_profit: Optional[Decimal] = Field(None, description="Загальний чистий прибуток по чеку")
    vat_amount: Optional[Decimal] = Field(None, description="Загальна сума ПДВ по чеку")

    model_config = ConfigDict(from_attributes=True)


class ReceiptHistoryParams(BaseModel):
    """Параметри для перегляду історії чеків."""
    cashier_id: Optional[UUID] = Field(None, description="Фільтр за касиром")
    receipt_type: Optional[ReceiptType] = Field(None, description="Фільтр за типом")
    date_from: Optional[datetime] = Field(None, description="Початкова дата")
    date_to: Optional[datetime] = Field(None, description="Кінцева дата")
    page: int = Field(1, ge=1, description="Номер сторінки")
    size: int = Field(20, ge=1, le=100, description="Розмір сторінки")
