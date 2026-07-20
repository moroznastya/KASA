"""
Ініціалізація сервісів Kasa POS.

Експортує всі сервіси для зручного використання.
"""

from app.services.auth_service import AuthService
from app.services.product_service import ProductService
from app.services.document_service import DocumentService
from app.services.ledger_service import LedgerService

__all__ = [
    "AuthService",
    "ProductService",
    "DocumentService",
    "LedgerService",
]
