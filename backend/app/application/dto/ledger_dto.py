"""
DTO для LedgerEntry (Запис журналу взаєморозрахунків).

Використовуються для передачі даних між Application та Presentation шарами.
"""

from dataclasses import dataclass, field
from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class LedgerEntryDTO:
    """Повний DTO запису журналу взаєморозрахунків для відповіді клієнту."""
    id: UUID
    supplier_id: UUID
    amount: Decimal
    operation_type: str = "invoice"
    balance_after: Optional[Decimal] = None
    created_at: Optional[datetime] = None
    document_id: Optional[UUID] = None
    document_number: str = ""
    notes: str = ""


@dataclass
class LedgerCreateDTO:
    """DTO для створення нового запису в журналі взаєморозрахунків."""
    supplier_id: UUID
    amount: Decimal
    operation_type: str = "invoice"
    document_id: Optional[UUID] = None
    document_number: str = ""
    notes: str = ""
