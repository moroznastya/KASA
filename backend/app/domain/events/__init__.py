"""Domain Events для Torgashka POS."""

from .base_event import BaseDomainEvent
from .invoice_events import InvoiceApproved, InvoiceCreated, InvoiceDeleted, InvoiceUpdated
from .ledger_events import LedgerEntryCreated
from .product_events import ProductCreated, ProductDeleted, ProductUpdated, StockChanged
from .receipt_events import ReceiptCreated, ReceiptRefunded
from .user_events import UserCreated, UserLoggedIn

__all__ = [
    "BaseDomainEvent",
    "InvoiceApproved",
    "InvoiceCreated",
    "InvoiceDeleted",
    "InvoiceUpdated",
    "LedgerEntryCreated",
    "ProductCreated",
    "ProductDeleted",
    "ProductUpdated",
    "ReceiptCreated",
    "ReceiptRefunded",
    "StockChanged",
    "UserCreated",
    "UserLoggedIn",
]
