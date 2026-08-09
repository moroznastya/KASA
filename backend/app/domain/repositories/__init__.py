"""Repository Interfaces (Ports) для доменного шару Torgashka POS."""

from .i_product_repository import IProductRepository
from .i_invoice_repository import IInvoiceRepository
from .i_receipt_repository import IReceiptRepository
from .i_user_repository import IUserRepository
from .i_supplier_repository import ISupplierRepository
from .i_category_repository import ICategoryRepository
from .i_ledger_repository import ILedgerRepository
from .i_unit_of_work import IUnitOfWork

__all__ = [
    "IProductRepository",
    "IInvoiceRepository",
    "IReceiptRepository",
    "IUserRepository",
    "ISupplierRepository",
    "ICategoryRepository",
    "ILedgerRepository",
    "IUnitOfWork",
]
