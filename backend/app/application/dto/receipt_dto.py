"""
DTO для Receipt (Чек продажу).

Використовуються для передачі даних між Application та Presentation шарами.
"""

from dataclasses import dataclass, field
from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class ReceiptItemDTO:
    """DTO позиції чеку продажу."""
    product_id: UUID
    name: str
    quantity: Decimal
    price: Decimal
    tax_rate: int = 20


@dataclass
class ReceiptDTO:
    """Повний DTO чеку продажу для відповіді клієнту."""
    id: UUID
    number: str
    items: list[ReceiptItemDTO] = field(default_factory=list)
    total: Optional[Decimal] = None
    payment_method: str = "cash"
    created_at: Optional[datetime] = None
    cash_amount: Optional[Decimal] = None
    card_amount: Optional[Decimal] = None
    change_amount: Optional[Decimal] = None
    customer_id: Optional[UUID] = None
    notes: str = ""


@dataclass
class ReceiptCreateDTO:
    """DTO для створення нового чеку продажу."""
    items: list[ReceiptItemDTO] = field(default_factory=list)
    payment_method: str = "cash"
    cash_amount: Optional[Decimal] = None
    card_amount: Optional[Decimal] = None
    customer_id: Optional[UUID] = None
    notes: str = ""
