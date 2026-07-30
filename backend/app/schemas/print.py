"""
Pydantic схеми для рендеру цінників та етикеток.

Містить вхідні/вихідні схеми для:
  - PriceTagRenderRequest / PriceTagRenderResponse (цінники на A4)
  - LabelRenderRequest / LabelRenderResponse (етикетки на термопринтер)
  - InvoicePrintRequest / InvoicePrintResponse (друк з накладної)
  - PriceChangeItem (зміна цін в накладній)
"""

from __future__ import annotations

from decimal import Decimal
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


# ─── Схеми для друку цінників/етикеток з накладної ──────────────────────────


class InvoicePrintRequest(BaseModel):
    """Запит на друк цінників/етикеток з накладної."""
    print_type: Literal["price_tag", "label"] = Field(
        ..., description="Тип друку: price_tag (A4) або label (термопринтер)"
    )
    only_changed: bool = Field(
        False, description="True — друкувати тільки товари зі змінною ціною"
    )
    template_id: UUID = Field(
        ..., description="ID шаблону друку (з PrintTemplate)"
    )
    width_mm: float = Field(40, ge=10, le=200, description="Ширина цінника/етикетки в мм")
    height_mm: float = Field(25, ge=10, le=200, description="Висота цінника/етикетки в мм")
    gap_mm: float = Field(3, ge=0, le=20, description="Проміжок між цінниками в мм")
    margin_mm: float = Field(10, ge=0, le=50, description="Поля сторінки в мм (тільки для price_tag)")
    barcode_type: Literal["code128", "qr"] = Field(
        "code128", description="Тип кодування штрих-коду: code128 (лінійний) або qr"
    )
    barcode_height_mm: float = Field(
        12, ge=4, le=40, description="Висота штрих-коду в мм"
    )


class InvoicePrintResponse(BaseModel):
    """Відповідь з HTML для друку."""
    html: str = Field(..., description="Готовий до друку HTML-документ")
    total_labels: int = Field(..., ge=0, description="Загальна кількість цінників/етикеток")
    total_pages: int | None = Field(None, description="Кількість сторінок (тільки для price_tag)")
    changed_count: int | None = Field(None, description="Кількість товарів зі змінною ціною")
    total_count: int = Field(..., ge=0, description="Загальна кількість товарів у накладній")


class PriceChangeItem(BaseModel):
    """Інформація про зміну ціни товару в накладній."""
    product_id: UUID = Field(..., description="ID товару")
    title: str = Field(..., description="Назва товару")
    barcode: str | None = Field(None, description="Штрих-код товару")
    article: str | None = Field(None, description="Артикул товару")
    invoice_price: str = Field(..., description="Ціна в накладній")
    current_price: str = Field(..., description="Поточна роздрібна ціна")
    changed: bool = Field(..., description="True — ціна змінилась")
    difference: str = Field(..., description="Різниця між цінами")
