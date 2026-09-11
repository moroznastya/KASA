"""Domain Events: Receipt (чеки продажу)."""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from uuid import UUID

from .base_event import BaseDomainEvent


@dataclass(kw_only=True)
class ReceiptCreated(BaseDomainEvent):
    """Створено чек продажу."""
    receipt_id: UUID
    cashier_id: UUID
    total_amount: Decimal
    payment_method: str


@dataclass(kw_only=True)
class ReceiptRefunded(BaseDomainEvent):
    """Повернення за чеком."""
    receipt_id: UUID
    original_receipt_id: UUID
    refund_amount: Decimal
