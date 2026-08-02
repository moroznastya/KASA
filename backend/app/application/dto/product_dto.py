"""
DTO для Product (Товар).

Використовуються для передачі даних між Application та Presentation шарами.
"""

from dataclasses import dataclass, field
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class ProductDTO:
    """Повний DTO товару для відповіді клієнту."""
    id: UUID
    name: str
    barcode: Optional[str] = None
    price: Optional[Decimal] = None
    cost_price: Optional[Decimal] = None
    stock: Optional[Decimal] = None
    category_id: Optional[UUID] = None
    supplier_id: Optional[UUID] = None
    tax_rate: int = 20
    sku: str = ""
    unit: str = "шт"
    is_active: bool = True
    description: str = ""

    @property
    def quantity(self) -> Optional[Decimal]:
        """Аліас для сумісності з ProductResponse (API v2 використовує quantity)."""
        return self.stock


@dataclass
class ProductCreateDTO:
    """DTO для створення нового товару."""
    name: str
    barcode: Optional[str] = None
    price: Optional[Decimal] = None
    cost_price: Optional[Decimal] = None
    stock: Optional[Decimal] = None
    category_id: Optional[UUID] = None
    supplier_id: Optional[UUID] = None
    tax_rate: int = 20
    sku: str = ""
    unit: str = "шт"
    is_active: bool = True
    description: str = ""


@dataclass
class ProductUpdateDTO:
    """DTO для оновлення існуючого товару. Всі поля опціональні."""
    name: Optional[str] = None
    barcode: Optional[str] = None
    price: Optional[Decimal] = None
    cost_price: Optional[Decimal] = None
    stock: Optional[Decimal] = None
    category_id: Optional[UUID] = None
    supplier_id: Optional[UUID] = None
    tax_rate: Optional[int] = None
    sku: Optional[str] = None
    unit: Optional[str] = None
    is_active: Optional[bool] = None
    description: Optional[str] = None
