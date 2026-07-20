"""
Доменні події модуля Ledger (Взаєморозрахунки).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from uuid import UUID

from .base_event import DomainEvent


@dataclass(frozen=True)
class LedgerEntryCreated(DomainEvent):
    """
    Подія: створено запис у журналі взаєморозрахунків.

    Публікується після створення нового запису в ledger.
    Слухачі: ReportsModule (оновлення звітів), NotificationModule (сповіщення).
    """

    entry_id: UUID = field(default_factory=UUID)
    supplier_id: UUID = field(default_factory=UUID)
    amount: Decimal = Decimal("0")
    currency: str = "UAH"
    operation_type: str = ""
    balance_after: Decimal = Decimal("0")
    document_id: UUID | None = None
