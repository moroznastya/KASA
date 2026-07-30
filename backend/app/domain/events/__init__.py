"""Domain Events для Kasa POS."""

from .base_event import BaseDomainEvent
from .product_events import ProductCreated, ProductUpdated, ProductDeleted, StockChanged
from .invoice_events import InvoiceCreated, InvoiceUpdated, InvoiceDeleted, InvoiceApproved
from .receipt_events import ReceiptCreated, ReceiptRefunded
from .ledger_events import LedgerEntryCreated
from .user_events import UserLoggedIn, UserCreated

__all__ = [
    "BaseDomainEvent",
    "ProductCreated",
    "ProductUpdated",
    "ProductDeleted",
    "StockChanged",
    "InvoiceCreated",
    "InvoiceUpdated",
    "InvoiceDeleted",
    "InvoiceApproved",
    "ReceiptCreated",
    "ReceiptRefunded",
    "LedgerEntryCreated",
    "UserLoggedIn",
    "UserCreated",
]
