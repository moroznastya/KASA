"""
Pydantic схеми для рендеру цінників та етикеток.

Містить вхідні/вихідні схеми для:
  - PriceTagRenderRequest / PriceTagRenderResponse (цінники на A4)
  - LabelRenderRequest / LabelRenderResponse (етикетки на термопринтер)
"""

from __future__ import annotations

from typing import Optional, Literal
from uuid import UUID

from pydantic import BaseModel, Field


class PriceTagProduct(BaseModel):
    """Товар для друку цінника / етикетки."""
    id: UUID = Field(..., description="ID товару")
    title: str = Field(..., description="Назва товару")
    price: str = Field(..., description="Ціна товару (рядок для гнучкості)")
    barcode: Optional[str] = Field(None, description="Штрих-код")
    article: Optional[str] = Field(None, description="Артикул (SKU)")
    category: Optional[str] = Field(None, description="Категорія")
    created_date: Optional[str] = Field(None, description="Дата створення цінника")
    copies: int = Field(1, ge=1, le=999, description="Кількість копій цього товару")


class PriceTagRenderRequest(BaseModel):
    """Запит на рендер цінників на A4."""
    template_id: UUID = Field(..., description="ID шаблону цінника")
    products: list[PriceTagProduct] = Field(..., min_length=1, description="Список товарів для друку")
    width_mm: float = Field(40, ge=10, le=200, description="Ширина одного цінника в мм")
    height_mm: float = Field(25, ge=10, le=200, description="Висота одного цінника в мм")
    gap_mm: float = Field(3, ge=0, le=20, description="Проміжок між цінниками в мм")
    margin_mm: float = Field(10, ge=0, le=50, description="Поля сторінки в мм")
    barcode_type: Literal["code128", "qr"] = Field(
        "code128", description="Тип кодування штрих-коду: code128 (лінійний) або qr"
    )
    barcode_height_mm: float = Field(
        12, ge=4, le=40, description="Висота штрих-коду в мм (для code128) або розмір (для QR)"
    )


class PriceTagRenderResponse(BaseModel):
    """Відповідь після рендеру цінників."""
    html: str = Field(..., description="Готовий до друку HTML-документ")
    total_pages: int = Field(..., ge=0, description="Кількість сторінок A4")
    total_labels: int = Field(..., ge=0, description="Загальна кількість цінників")


class LabelRenderRequest(BaseModel):
    """Запит на рендер етикеток для термопринтера."""
    template_id: UUID = Field(..., description="ID шаблону етикетки")
    products: list[PriceTagProduct] = Field(..., min_length=1, description="Список товарів для друку")
    width_mm: float = Field(58, ge=20, le=120, description="Ширина термопаперу в мм")
    height_mm: float = Field(40, ge=10, le=200, description="Висота однієї етикетки в мм")
    gap_mm: float = Field(2, ge=0, le=20, description="Проміжок між етикетками в мм")
    barcode_type: Literal["code128", "qr"] = Field(
        "code128", description="Тип кодування штрих-коду: code128 (лінійний) або qr"
    )
    barcode_height_mm: float = Field(
        12, ge=4, le=40, description="Висота штрих-коду в мм (для code128) або розмір (для QR)"
    )


class LabelRenderResponse(BaseModel):
    """Відповідь після рендеру етикеток."""
    html: str = Field(..., description="Готовий до друку HTML-документ")
    total_labels: int = Field(..., ge=0, description="Загальна кількість етикеток")
