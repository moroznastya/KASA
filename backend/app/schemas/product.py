"""
Pydantic схеми для моделі Product (Товар).

Містить Create, Update, Response схеми та параметри пошуку.
Всі фінансові поля використовують Decimal для уникнення помилок округлення.
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict


# ─── Create Schema ───────────────────────────────────────────────────────────
class ProductCreate(BaseModel):
    """Схема створення нового товару."""
    barcode: Optional[str] = Field(None, max_length=50, description="Основний штрих-код товару")
    sku: Optional[str] = Field(None, max_length=100, description="Артикул товару")
    title: str = Field(..., max_length=255, description="Назва товару")
    description: Optional[str] = Field(None, description="Опис товару")
    price: Optional[Decimal] = Field(None, max_digits=10, decimal_places=2, description="Роздрібна ціна (грн)")
    cost_price: Optional[Decimal] = Field(None, max_digits=10, decimal_places=2, description="Собівартість (грн)")
    stock: Optional[Decimal] = Field(None, max_digits=10, decimal_places=3, description="Початковий залишок")
    uktzed: Optional[str] = Field(None, max_length=10, description="Код УКТЗЕД")
    scan_excise: bool = Field(False, description="Чи сканувати акцизну марку")
    tax_rate: Optional[Decimal] = Field(Decimal("20.00"), max_digits=5, decimal_places=2, description="Ставка ПДВ (%)")
    tax_group: Optional[str] = Field("А", max_length=2, description="Група оподаткування")
    is_weight: bool = Field(False, description="Ваговий товар")
    unit: Optional[str] = Field("шт", max_length=10, description="Одиниця виміру")
    category_id: Optional[UUID] = Field(None, description="ID категорії")
    supplier_id: Optional[UUID] = Field(None, description="ID постачальника")


class ProductUpdate(BaseModel):
    """Схема оновлення товару. Всі поля опціональні."""
    barcode: Optional[str] = Field(None, max_length=50, description="Основний штрих-код товару")
    sku: Optional[str] = Field(None, max_length=100, description="Артикул товару")
    title: Optional[str] = Field(None, max_length=255, description="Назва товару")
    description: Optional[str] = Field(None, description="Опис товару")
    price: Optional[Decimal] = Field(None, max_digits=10, decimal_places=2, description="Роздрібна ціна (грн)")
    cost_price: Optional[Decimal] = Field(None, max_digits=10, decimal_places=2, description="Собівартість (грн)")
    stock: Optional[Decimal] = Field(None, max_digits=10, decimal_places=3, description="Залишок")
    uktzed: Optional[str] = Field(None, max_length=10, description="Код УКТЗЕД")
    scan_excise: Optional[bool] = Field(None, description="Чи сканувати акцизну марку")
    tax_rate: Optional[Decimal] = Field(None, max_digits=5, decimal_places=2, description="Ставка ПДВ (%)")
    tax_group: Optional[str] = Field(None, max_length=2, description="Група оподаткування")
    is_weight: Optional[bool] = Field(None, description="Ваговий товар")
    unit: Optional[str] = Field(None, max_length=10, description="Одиниця виміру")
    category_id: Optional[UUID] = Field(None, description="ID категорії")
    supplier_id: Optional[UUID] = Field(None, description="ID постачальника")


class ProductResponse(BaseModel):
    """Схема відповіді з даними товару."""
    id: UUID
    barcode: Optional[str] = None
    sku: Optional[str] = None
    title: str
    description: Optional[str] = None
    price: Optional[Decimal] = None
    cost_price: Optional[Decimal] = None
    stock: Optional[Decimal] = None
    uktzed: Optional[str] = None
    scan_excise: bool = False
    tax_rate: Optional[Decimal] = None
    tax_group: Optional[str] = None
    is_weight: bool = False
    unit: Optional[str] = None
    category_id: Optional[UUID] = None
    supplier_id: Optional[UUID] = None
    created_at: datetime
    updated_at: datetime

    model_config = ConfigDict(from_attributes=True)


class ProductListResponse(BaseModel):
    """Схема відповіді зі списком товарів."""
    items: list[ProductResponse]
    total: int = Field(..., description="Загальна кількість товарів")
    page: int = Field(1, description="Поточна сторінка")
    size: int = Field(20, description="Розмір сторінки")


class ProductSearchParams(BaseModel):
    """Параметри пошуку товарів."""
    query: Optional[str] = Field(None, description="Пошуковий запит (назва, штрих-код, артикул)")
    barcode: Optional[str] = Field(None, description="Пошук за штрих-кодом")
    category_id: Optional[UUID] = Field(None, description="Фільтр за категорією")
    supplier_id: Optional[UUID] = Field(None, description="Фільтр за постачальником")
    min_price: Optional[Decimal] = Field(None, description="Мінімальна ціна")
    max_price: Optional[Decimal] = Field(None, description="Максимальна ціна")
    is_weight: Optional[bool] = Field(None, description="Фільтр вагових товарів")
    page: int = Field(1, ge=1, description="Номер сторінки")
    size: int = Field(20, ge=1, le=100, description="Розмір сторінки")
