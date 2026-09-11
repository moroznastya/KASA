"""
Pydantic схеми для моделей ReturnInvoice та ReturnInvoiceItem (Повернення постачальнику).
"""

from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field

from app.infrastructure.persistence.models.return_invoice import ReturnActionType, ReturnInvoiceStatus


class ProductBrief(BaseModel):
    """Скорочена інформація про товар для відображення в позиціях."""
    id: UUID
    title: str
    barcode: Optional[str] = None

    model_config = ConfigDict(from_attributes=True)


class ReturnInvoiceItemCreate(BaseModel):
    """Схема створення позиції повернення."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    price: Decimal = Field(..., max_digits=10, decimal_places=2, description="Ціна за одиницю (грн)")
    total: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума (грн)")
    cost_price: Optional[Decimal] = Field(None, max_digits=10, decimal_places=2, description="Собівартість одиниці (грн)")


class ExchangeItemCreate(BaseModel):
    """Схема створення позиції обміну (новий товар замість повернутого)."""
    product_id: UUID = Field(..., description="ID нового товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Кількість")
    price: Decimal = Field(..., max_digits=10, decimal_places=2, description="Ціна за одиницю (грн)")
    total: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума (грн)")


class ReturnInvoiceItemResponse(BaseModel):
    """Схема відповіді з даними позиції повернення."""
    id: UUID
    return_invoice_id: UUID
    product_id: UUID
    product: Optional[ProductBrief] = Field(None, description="Інформація про товар")
    quantity: Decimal
    price: Decimal
    cost_price: Optional[Decimal] = Field(None, description="Собівартість одиниці на момент повернення (грн)")
    markup_percent: Optional[Decimal] = Field(None, description="Відсоток націнки на момент повернення")
    total: Decimal
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class ExchangeInvoiceItemBrief(BaseModel):
    """Скорочена інформація про позицію прибуткової накладної при обміні."""
    id: UUID
    product_id: UUID
    product: Optional[ProductBrief] = Field(None, description="Інформація про товар")
    quantity: Decimal
    price: Decimal
    total: Decimal

    model_config = ConfigDict(from_attributes=True)


class ExchangeInvoiceBrief(BaseModel):
    """Скорочена інформація про прибуткову накладну, створену при обміні."""
    id: UUID
    number: str
    total_amount: Optional[Decimal] = None
    items: list[ExchangeInvoiceItemBrief] = []

    model_config = ConfigDict(from_attributes=True)


class ReturnInvoiceCreate(BaseModel):
    """Схема створення нового повернення постачальнику."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа (якщо не вказано, генерується автоматично)")
    supplier_id: UUID = Field(..., description="ID постачальника")
    return_date: datetime = Field(..., description="Дата повернення")
    return_action: ReturnActionType = Field(
        ReturnActionType.DEDUCT_FROM_DEBT,
        description="Дія при підтвердженні: deduct_from_debt / add_to_cash / exchange",
    )
    is_fiscal: bool = Field(False, description="Фіскальний документ")
    notes: Optional[str] = Field(None, description="Причина повернення / нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: list[ReturnInvoiceItemCreate] = Field(default_factory=list, description="Позиції повернення")
    # Поля для обміну на інший товар
    exchange_items: Optional[list[ExchangeItemCreate]] = Field(
        None,
        description="Товари для обміну (обов'язково, якщо return_action = exchange)",
    )
    # Опціональна прив'язка до прибуткової накладної
    source_invoice_id: Optional[UUID] = Field(
        None,
        description="ID прибуткової накладної, до якої відноситься повернення (опціонально)",
    )


class ReturnInvoiceUpdate(BaseModel):
    """Схема оновлення повернення. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер документа")
    supplier_id: Optional[UUID] = Field(None, description="ID постачальника")
    return_date: Optional[datetime] = Field(None, description="Дата повернення")
    return_action: Optional[ReturnActionType] = Field(None, description="Дія при підтвердженні")
    is_fiscal: Optional[bool] = Field(None, description="Фіскальний документ")
    notes: Optional[str] = Field(None, description="Причина повернення / нотатки")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: Optional[list[ReturnInvoiceItemCreate]] = Field(None, description="Позиції повернення")
    exchange_items: Optional[list[ExchangeItemCreate]] = Field(
        None,
        description="Товари для обміну (обов'язково, якщо return_action = exchange)",
    )
    source_invoice_id: Optional[UUID] = Field(
        None,
        description="ID прибуткової накладної, до якої відноситься повернення (опціонально)",
    )


class ReturnInvoiceResponse(BaseModel):
    """Схема відповіді з даними повернення."""
    id: UUID
    number: str
    supplier_id: UUID
    supplier_name: Optional[str] = Field(None, description="Назва постачальника")
    return_date: datetime
    status: ReturnInvoiceStatus
    return_action: ReturnActionType = ReturnActionType.DEDUCT_FROM_DEBT
    is_fiscal: bool = False
    notes: Optional[str] = None
    total_amount: Optional[Decimal] = None
    exchange_invoice_id: Optional[UUID] = Field(None, description="ID прибуткової накладної при обміні")
    exchange_invoice: Optional[ExchangeInvoiceBrief] = Field(None, description="Прибуткова накладна при обміні")
    source_invoice_id: Optional[UUID] = Field(None, description="ID прибуткової накладної, до якої відноситься повернення")
    created_at: datetime
    updated_at: datetime
    items: list[ReturnInvoiceItemResponse] = []

    model_config = ConfigDict(from_attributes=True)


class ReturnInvoiceConfirmRequest(BaseModel):
    """Схема підтвердження повернення (зміна статусу)."""
    status: ReturnInvoiceStatus = Field(..., description="Новий статус (confirmed / cancelled)")
    exchange_items: Optional[list[ExchangeItemCreate]] = Field(
        None,
        description="Товари для обміну (обов'язково, якщо return_action = exchange)",
    )
