"""Domain Events: Ledger (журнал взаєморозрахунків)."""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from uuid import UUID

from .base_event import BaseDomainEvent


@dataclass(kw_only=True)
class LedgerEntryCreated(BaseDomainEvent):
    """Створено запис у журналі взаєморозрахунків."""
    entry_id: UUID
    supplier_id: UUID
    amount: Decimal
    entry_type: str  # "debit", "credit"
    reference_type: str  # "invoice", "payment"
    reference_id: UUID
