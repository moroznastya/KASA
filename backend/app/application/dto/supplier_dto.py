"""
DTO для Supplier (Постачальник).

Використовуються для передачі даних між Application та Presentation шарами.
"""

from dataclasses import dataclass, field
from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class SupplierDTO:
    """Повний DTO постачальника для відповіді клієнту."""
    id: UUID
    name: str
    balance: Optional[Decimal] = None
    contact_person: str = ""
    phone: str = ""
    email: str = ""
    address: str = ""
    edrpou: str = ""
    is_active: bool = True
    created_at: Optional[datetime] = None
    notes: str = ""


@dataclass
class SupplierCreateDTO:
    """DTO для створення нового постачальника."""
    name: str
    contact_person: str = ""
    phone: str = ""
    email: str = ""
    address: str = ""
    edrpou: str = ""
    is_active: bool = True
    notes: str = ""
