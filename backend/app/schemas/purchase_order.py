"""
Pydantic схеми для моделей PurchaseOrder та PurchaseOrderItem (Замовлення постачальнику).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.infrastructure.persistence.models.purchase_order import PurchaseOrderStatus


class ProductBrief(BaseModel):
    """Скорочена інформація про товар для відображення в позиціях."""
    id: UUID
    title: str
    barcode: Optional[str] = None

    model_config = ConfigDict(from_attributes=True)


class PurchaseOrderItemCreate(BaseModel):
    """Схема створення позиції замовлення."""
    product_id: UUID = Field(..., description="ID товару")
    quantity: Decimal = Field(..., max_digits=10, decimal_places=3, description="Замовлена кількість")
    price: Decimal = Field(..., max_digits=10, decimal_places=2, description="Ціна за одиницю (грн)")
    total: Decimal = Field(..., max_digits=12, decimal_places=2, description="Загальна сума (грн)")


class PurchaseOrderItemResponse(BaseModel):
    """Схема відповіді з даними позиції замовлення."""
    id: UUID
    purchase_order_id: UUID
    product_id: UUID
    product: Optional[ProductBrief] = Field(None, description="Інформація про товар")
    quantity: Decimal
    price: Decimal
    total: Decimal
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)


class InvoiceBrief(BaseModel):
    """Скорочена інформація про прибуткову накладну, створену із замовлення."""
    id: UUID
    number: str
    total_amount: Optional[Decimal] = None

    model_config = ConfigDict(from_attributes=True)


class PurchaseOrderCreate(BaseModel):
    """Схема створення нового замовлення постачальнику."""
    number: Optional[str] = Field(None, max_length=50, description="Номер замовлення (якщо не вказано, генерується автоматично)")
    supplier_id: UUID = Field(..., description="ID постачальника")
    order_date: datetime = Field(..., description="Дата замовлення")
    expected_date: Optional[datetime] = Field(None, description="Очікувана дата поставки")
    is_fiscal: bool = Field(False, description="Фіскальний документ")
    notes: Optional[str] = Field(None, description="Нотатки до замовлення")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: list[PurchaseOrderItemCreate] = Field(default_factory=list, description="Позиції замовлення")


class PurchaseOrderUpdate(BaseModel):
    """Схема оновлення замовлення. Всі поля опціональні."""
    number: Optional[str] = Field(None, max_length=50, description="Номер замовлення")
    supplier_id: Optional[UUID] = Field(None, description="ID постачальника")
    order_date: Optional[datetime] = Field(None, description="Дата замовлення")
    expected_date: Optional[datetime] = Field(None, description="Очікувана дата поставки")
    is_fiscal: Optional[bool] = Field(None, description="Фіскальний документ")
    notes: Optional[str] = Field(None, description="Нотатки до замовлення")
    total_amount: Optional[Decimal] = Field(None, max_digits=12, decimal_places=2, description="Загальна сума")
    items: Optional[list[PurchaseOrderItemCreate]] = Field(None, description="Позиції замовлення")


class PurchaseOrderResponse(BaseModel):
    """Схема відповіді з даними замовлення."""
    id: UUID
    number: str
    supplier_id: UUID
    supplier_name: Optional[str] = Field(None, description="Назва постачальника")
    order_date: datetime
    expected_date: Optional[datetime] = None
    status: PurchaseOrderStatus
    is_fiscal: bool = False
    notes: Optional[str] = None
    total_amount: Optional[Decimal] = None
    invoice_id: Optional[UUID] = Field(None, description="ID прибуткової накладної, створеної при підтвердженні")
    invoice: Optional[InvoiceBrief] = Field(None, description="Прибуткова накладна, створена при підтвердженні")
    created_at: datetime
    updated_at: datetime
    items: list[PurchaseOrderItemResponse] = []

    model_config = ConfigDict(from_attributes=True)


class PurchaseOrderConfirmRequest(BaseModel):
    """Схема підтвердження замовлення (зміна статусу)."""
    status: PurchaseOrderStatus = Field(..., description="Новий статус (confirmed / cancelled)")
