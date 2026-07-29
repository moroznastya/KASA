"""
Pydantic схеми для моделей Receipt та ReceiptItem (Чек продажу / повернення).
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.models.receipt import ReceiptType, ReceiptPaymentMethod


class DebtPaymentInfo(BaseModel):
    """Інформація про оплату боргу через касу."""
    debtor_id: UUID = Field(..., description="ID боржника")
    amount: Decimal = Field(..., gt=0, max_digits=12, decimal_places=2, description="Сума оплати боргу")


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
    product_barcode: Optional[str] = Field(None, description="Штрих-код товару")
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
    original_receipt_id: Optional[UUID] = Field(None, description="ID оригінального чеку (для повернень)")
    return_reason: Optional[str] = Field(None, max_length=500, description="Причина повернення")
    items: list[ReceiptItemCreate] = Field(default_factory=list, description="Позиції чеку")
    debt_payment: Optional[DebtPaymentInfo] = Field(None, description="Оплата боргу через касу")
    payment_method: Optional[ReceiptPaymentMethod] = Field(None, description="Спосіб оплати: cash, card, mixed")


class ReceiptResponse(BaseModel):
    """Схема відповіді з даними чеку."""
    id: UUID
    receipt_number: str
    receipt_type: ReceiptType
    cashier_id: UUID
    total_amount: Decimal
    paid_amount: Optional[Decimal] = Field(None, description="Фактично сплачена сума")
    change_amount: Optional[Decimal] = Field(None, description="Сума здачі (грн)")
    debtor_id: Optional[UUID] = Field(None, description="ID боржника")
    is_return: bool
    notes: Optional[str] = None
    created_at: datetime
    items: list[ReceiptItemResponse] = []
    total_profit: Optional[Decimal] = Field(None, description="Загальний чистий прибуток по чеку")
    vat_amount: Optional[Decimal] = Field(None, description="Загальна сума ПДВ по чеку")
    cashier_name: str = Field("", description="Ім'я касира")
    payment_method: Optional[str] = Field(None, description="Спосіб оплати: cash, card, mixed")

    model_config = ConfigDict(from_attributes=True)


class ReceiptHistoryParams(BaseModel):
    """Параметри для перегляду історії чеків."""
    cashier_id: Optional[UUID] = Field(None, description="Фільтр за касиром")
    receipt_type: Optional[ReceiptType] = Field(None, description="Фільтр за типом")
    date_from: Optional[datetime] = Field(None, description="Початкова дата")
    date_to: Optional[datetime] = Field(None, description="Кінцева дата")
    page: int = Field(1, ge=1, description="Номер сторінки")
    size: int = Field(20, ge=1, le=100, description="Розмір сторінки")


# ─── Схеми для повернення товарів ───────────────────────────────

class ReceiptSearchResult(BaseModel):
    """Результат пошуку чеків для повернення."""
    id: UUID
    receipt_number: str
    receipt_type: ReceiptType
    total_amount: Decimal
    created_at: datetime
    cashier_name: str = ""
    items_count: int = 0


class ProductBriefInfo(BaseModel):
    """Коротка інформація про товар для повернення."""
    id: UUID
    title: str
    barcode: Optional[str] = None
    price: Decimal
    unit: str


class RecentSaleInfo(BaseModel):
    """Інформація про один продаж товару (для повернення без чеку)."""
    receipt_id: UUID
    receipt_number: str
    created_at: datetime
    quantity: Decimal
    price: Decimal


class ProductRecentSalesResponse(BaseModel):
    """Відповідь з інформацією про останні продажі товару."""
    product: ProductBriefInfo
    total_sold: Decimal
    total_returned: Decimal
    returnable: Decimal
    recent_sales: list[RecentSaleInfo]


class ProductRecentSalesListResponse(BaseModel):
    """Відповідь зі списком товарів, знайдених за запитом."""
    items: list[ProductRecentSalesResponse]
    total: int
