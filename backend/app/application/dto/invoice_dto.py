"""
DTO для Invoice (Прибуткова накладна).

Використовуються для передачі даних між Application та Presentation шарами.
"""

from dataclasses import dataclass, field
from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class InvoiceItemDTO:
    """DTO позиції прибуткової накладної."""
    product_id: UUID
    quantity: Decimal
    price: Decimal
    tax_rate: int = 20
    name: str = ""


@dataclass
class InvoiceDTO:
    """Повний DTO прибуткової накладної для відповіді клієнту."""
    id: UUID
    number: str
    supplier_id: UUID
    items: list[InvoiceItemDTO] = field(default_factory=list)
    total: Optional[Decimal] = None
    status: str = "draft"
    created_at: Optional[datetime] = None
    confirmed_at: Optional[datetime] = None
    notes: str = ""
    is_fiscal: bool = False
    """Ознака: фіскальна накладна (товар оприбутковується у fiscal_stock)."""


@dataclass
class InvoiceCreateDTO:
    """DTO для створення нової прибуткової накладної."""
    number: str
    supplier_id: UUID
    items: list[InvoiceItemDTO] = field(default_factory=list)
    notes: str = ""
    is_fiscal: bool = False
    """Фіскальна накладна: при підтвердженні збільшує fiscal_stock товарів."""


@dataclass
class InvoiceConfirmDTO:
    """DTO для підтвердження прибуткової накладної."""
    invoice_id: UUID


@dataclass
class InvoiceUpdateDTO:
    """DTO для оновлення існуючої прибуткової накладної (тільки чернетки)."""
    number: Optional[str] = None
    supplier_id: Optional[UUID] = None
    notes: Optional[str] = None
    is_fiscal: Optional[bool] = None
    invoice_date: Optional[datetime] = None
    items: Optional[list[InvoiceItemDTO]] = None
    """Якщо передано — повністю замінює позиції накладної."""
