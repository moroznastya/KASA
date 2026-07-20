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


@dataclass
class InvoiceCreateDTO:
    """DTO для створення нової прибуткової накладної."""
    number: str
    supplier_id: UUID
    items: list[InvoiceItemDTO] = field(default_factory=list)
    notes: str = ""


@dataclass
class InvoiceConfirmDTO:
    """DTO для підтвердження прибуткової накладної."""
    invoice_id: UUID
