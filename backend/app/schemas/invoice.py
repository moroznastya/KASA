"""
Pydantic схеми для моделей Invoice та InvoiceItem (Прибуткова накладна).
"""

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field

from app.infrastructure.persistence.models.invoice import InvoiceStatus, PaymentMethod


class ProductBrief(BaseModel):
    """Скорочена інформація про товар для відображення в позиціях."""
    id: UUID
    title: str
    barcode: Optional[str] = None
    price: Optional[Decimal] = Field(None, description="Поточна роздрібна ціна товару")
    markup: Optional[Decimal] = Field(None, description="Поточна націнка товару (%)")
    cost_price: Optional[Decimal] = Field(None, description="Поточна собівартість товару (з ПДВ)")

    model_config = ConfigDict(from_attributes=True)


class InvoiceItemCreate(BaseModel):
    """Схема створення позиції накладної."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    price: Decimal = Field(..., max_digits=10, decimal_places=2, description="Ціна за одиницю (грн)")
    total: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума (грн)")
    cost_price: Optional[Decimal] = Field(None, max_digits=10, decimal_places=2, description="Собівартість за одиницю (з ПДВ)")
    markup_percent: Optional[Decimal] = Field(None, max_digits=5, decimal_places=2, description="Відсоток націнки")


class InvoiceItemResponse(BaseModel):
    """Схема відповіді з даними позиції накладної."""
    id: UUID
    invoice_id: UUID
    product_id: UUID
    product: Optional[ProductBrief] = Field(None, description="Інформація про товар")
    quantity: Decimal
    price: Decimal
    total: Decimal
    cost_price: Optional[Decimal] = Field(None, description="Собівартість за одиницю (з ПДВ)")
    markup_percent: Optional[Decimal] = Field(None, description="Відсоток націнки")
    previous_price: Optional[Decimal] = Field(None, description="Ціна товару до створення накладної")
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class InvoiceCreate(BaseModel):
    """Схема створення нової прибуткової накладної."""
    number: Optional[str] = Field(None, max_length=50, description="Номер накладної (якщо не вказано — генерується автоматично)")
    supplier_id: UUID = Field(..., description="ID постачальника")
    invoice_date: datetime = Field(..., description="Дата накладної")
    payment_method: Optional[PaymentMethod] = Field(None, description="Спосіб оплати з постачальником")
    is_fiscal: bool = Field(False, description="Фіскальна накладна")
    notes: Optional[str] = Field(None, description="Нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: list[InvoiceItemCreate] = Field(default_factory=list, description="Позиції накладної")


class InvoiceUpdate(BaseModel):
    """Схема оновлення накладної. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер накладної")
    supplier_id: Optional[UUID] = Field(None, description="ID постачальника")
    invoice_date: Optional[datetime] = Field(None, description="Дата накладної")
    payment_method: Optional[PaymentMethod] = Field(None, description="Спосіб оплати з постачальником")
    is_fiscal: Optional[bool] = Field(None, description="Фіскальна накладна")
    notes: Optional[str] = Field(None, description="Нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: Optional[list[InvoiceItemCreate]] = Field(None, description="Позиції накладної")


class InvoiceResponse(BaseModel):
    """Схема відповіді з даними накладної."""
    id: UUID
    number: str
    supplier_id: UUID
    supplier_name: Optional[str] = Field(None, description="Назва постачальника")
    invoice_date: datetime
    status: InvoiceStatus
    payment_method: Optional[PaymentMethod] = None
    is_fiscal: bool = False
    notes: Optional[str] = None
    total_amount: Optional[Decimal] = None
    created_at: datetime
    updated_at: datetime
    items: list[InvoiceItemResponse] = []

    model_config = ConfigDict(from_attributes=True)


class InvoiceConfirmRequest(BaseModel):
    """Схема підтвердження накладної (зміна статусу)."""
    status: InvoiceStatus = Field(..., description="Новий статус (confirmed / cancelled)")


class InvoicePaymentInfo(BaseModel):
    """Інформація про оплату накладної."""
    invoice_id: UUID = Field(..., description="ID накладної")
    invoice_number: str = Field(..., description="Номер накладної")
    invoice_date: datetime = Field(..., description="Дата накладної")
    total_amount: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума накладної (грн)")
    paid_amount: Decimal = Field(..., max_digits=12, decimal_places=2, description="Вже сплачено (грн)")
    remaining: Decimal = Field(..., max_digits=12, decimal_places=2, description="Залишок до сплати (грн)")

    model_config = ConfigDict(from_attributes=True)
