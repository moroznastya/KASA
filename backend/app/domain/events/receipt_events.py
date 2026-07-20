"""
Доменні події модуля Receipt (Чеки продажу).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from uuid import UUID

from .base_event import DomainEvent


@dataclass(frozen=True)
class ReceiptCreated(DomainEvent):
    """
    Подія: чек продажу створено.

    Публікується після успішного створення чеку продажу.
    Слухачі: StockModule (зменшення залишків), ReportsModule (оновлення звітів).
    """

    receipt_id: UUID = field(default_factory=UUID)
    receipt_number: str = ""
    total_amount: Decimal = Decimal("0")
    currency: str = "UAH"
    payment_method: str = "cash"
    item_count: int = 0
