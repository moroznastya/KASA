"""
Доменні події модуля Invoice (Прибуткові накладні).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from uuid import UUID

from .base_event import DomainEvent


@dataclass(frozen=True)
class InvoiceConfirmed(DomainEvent):
    """
    Подія: прибуткову накладну підтверджено.

    Публікується після успішного підтвердження накладної.
    Слухачі: StockModule (збільшення залишків), LedgerModule (створення запису).
    """

    invoice_id: UUID = field(default_factory=UUID)
    invoice_number: str = ""
    supplier_id: UUID = field(default_factory=UUID)
    total_amount: Decimal = Decimal("0")
    currency: str = "UAH"
    item_count: int = 0


@dataclass(frozen=True)
class InvoiceCancelled(DomainEvent):
    """
    Подія: прибуткову накладну скасовано.

    Публікується після скасування підтвердженої накладної.
    Слухачі: StockModule (зменшення залишків), LedgerModule (створення запису).
    """

    invoice_id: UUID = field(default_factory=UUID)
    invoice_number: str = ""
    supplier_id: UUID = field(default_factory=UUID)
    total_amount: Decimal = Decimal("0")
    currency: str = "UAH"
