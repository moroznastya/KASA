"""Domain Events: Invoice (прибуткові накладні)."""

from __future__ import annotations

from dataclasses import dataclass
from uuid import UUID
from decimal import Decimal

from .base_event import BaseDomainEvent


@dataclass(kw_only=True)
class InvoiceCreated(BaseDomainEvent):
    """Створено прибуткову накладну."""
    invoice_id: UUID
    supplier_id: UUID
    total_amount: Decimal
    status: str = "pending"


@dataclass(kw_only=True)
class InvoiceUpdated(BaseDomainEvent):
    """Оновлено прибуткову накладну."""
    invoice_id: UUID
    changes: dict


@dataclass(kw_only=True)
class InvoiceDeleted(BaseDomainEvent):
    """Видалено прибуткову накладну."""
    invoice_id: UUID


@dataclass(kw_only=True)
class InvoiceApproved(BaseDomainEvent):
    """Затверджено прибуткову накладну (товари оприбутковано)."""
    invoice_id: UUID
    items_count: int
