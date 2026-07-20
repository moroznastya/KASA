"""
Ініціалізація всіх Pydantic схем Kasa POS.

Експортує всі Create/Update/Response схеми для зручного імпорту.
"""

from app.schemas.product import (
    ProductCreate,
    ProductUpdate,
    ProductResponse,
    ProductListResponse,
    ProductSearchParams,
)
from app.schemas.category import (
    CategoryCreate,
    CategoryUpdate,
    CategoryResponse,
    CategoryTreeResponse,
)
from app.schemas.supplier import (
    SupplierCreate,
    SupplierUpdate,
    SupplierResponse,
)
from app.schemas.user import (
    UserCreate,
    UserUpdate,
    UserResponse,
    UserLoginRequest,
    UserPinLoginRequest,
    UserTokenResponse,
)
from app.schemas.invoice import (
    InvoiceCreate,
    InvoiceUpdate,
    InvoiceResponse,
    InvoiceItemCreate,
    InvoiceItemResponse,
    InvoiceConfirmRequest,
)
from app.schemas.transfer import (
    TransferCreate,
    TransferUpdate,
    TransferResponse,
    TransferItemCreate,
    TransferItemResponse,
    TransferConfirmRequest,
)
from app.schemas.write_off import (
    WriteOffCreate,
    WriteOffUpdate,
    WriteOffResponse,
    WriteOffItemCreate,
    WriteOffItemResponse,
)
from app.schemas.return_invoice import (
    ReturnInvoiceCreate,
    ReturnInvoiceUpdate,
    ReturnInvoiceResponse,
    ReturnInvoiceItemCreate,
    ReturnInvoiceItemResponse,
    ReturnInvoiceConfirmRequest,
)
from app.schemas.receipt import (
    ReceiptCreate,
    ReceiptResponse,
    ReceiptItemCreate,
    ReceiptItemResponse,
    ReceiptHistoryParams,
)
from app.schemas.ledger import (
    SupplierLedgerCreate,
    SupplierLedgerResponse,
    SupplierLedgerBalanceResponse,
)

__all__ = [
    # Product
    "ProductCreate", "ProductUpdate", "ProductResponse", "ProductListResponse", "ProductSearchParams",
    # Category
    "CategoryCreate", "CategoryUpdate", "CategoryResponse", "CategoryTreeResponse",
    # Supplier
    "SupplierCreate", "SupplierUpdate", "SupplierResponse",
    # User
    "UserCreate", "UserUpdate", "UserResponse", "UserLoginRequest", "UserPinLoginRequest", "UserTokenResponse",
    # Invoice
    "InvoiceCreate", "InvoiceUpdate", "InvoiceResponse", "InvoiceItemCreate", "InvoiceItemResponse", "InvoiceConfirmRequest",
    # Transfer
    "TransferCreate", "TransferUpdate", "TransferResponse", "TransferItemCreate", "TransferItemResponse", "TransferConfirmRequest",
    # WriteOff
    "WriteOffCreate", "WriteOffUpdate", "WriteOffResponse", "WriteOffItemCreate", "WriteOffItemResponse",
    # ReturnInvoice
    "ReturnInvoiceCreate", "ReturnInvoiceUpdate", "ReturnInvoiceResponse", "ReturnInvoiceItemCreate", "ReturnInvoiceItemResponse", "ReturnInvoiceConfirmRequest",
    # Receipt
    "ReceiptCreate", "ReceiptResponse", "ReceiptItemCreate", "ReceiptItemResponse", "ReceiptHistoryParams",
    # Ledger
    "SupplierLedgerCreate", "SupplierLedgerResponse", "SupplierLedgerBalanceResponse",
]
