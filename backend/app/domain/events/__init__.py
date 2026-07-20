"""Domain Events для доменного шару Kasa POS."""

from .base_event import DomainEvent
from .product_events import ProductCreated, ProductUpdated, StockChanged
from .invoice_events import InvoiceConfirmed, InvoiceCancelled
from .receipt_events import ReceiptCreated
from .ledger_events import LedgerEntryCreated

__all__ = [
    "DomainEvent",
    "ProductCreated",
    "ProductUpdated",
    "StockChanged",
    "InvoiceConfirmed",
    "InvoiceCancelled",
    "ReceiptCreated",
    "LedgerEntryCreated",
]
