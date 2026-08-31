"""
Ініціалізація всіх Pydantic схем Torgashka POS.

Експортує всі Create/Update/Response схеми для зручного імпорту.
"""

from app.schemas.category import (
    CategoryCreate,
    CategoryResponse,
    CategoryTreeResponse,
    CategoryUpdate,
)
from app.schemas.invoice import (
    InvoiceConfirmRequest,
    InvoiceCreate,
    InvoiceItemCreate,
    InvoiceItemResponse,
    InvoiceResponse,
    InvoiceUpdate,
)
from app.schemas.ledger import (
    SupplierLedgerBalanceResponse,
    SupplierLedgerCreate,
    SupplierLedgerResponse,
)
from app.schemas.product import (
    ProductCreate,
    ProductListResponse,
    ProductResponse,
    ProductSearchParams,
    ProductUpdate,
)
from app.schemas.receipt import (
    DebtPaymentInfo,
    ReceiptCreate,
    ReceiptHistoryParams,
    ReceiptItemCreate,
    ReceiptItemResponse,
    ReceiptResponse,
)
from app.schemas.return_invoice import (
    ReturnInvoiceConfirmRequest,
    ReturnInvoiceCreate,
    ReturnInvoiceItemCreate,
    ReturnInvoiceItemResponse,
    ReturnInvoiceResponse,
    ReturnInvoiceUpdate,
)
from app.schemas.supplier import (
    SupplierCreate,
    SupplierResponse,
    SupplierUpdate,
)
from app.schemas.transfer import (
    TransferConfirmRequest,
    TransferCreate,
    TransferItemCreate,
    TransferItemResponse,
    TransferResponse,
    TransferUpdate,
)
from app.schemas.user import (
    UserCreate,
    UserLoginRequest,
    UserPinLoginRequest,
    UserResponse,
    UserTokenResponse,
    UserUpdate,
)
from app.schemas.write_off import (
    WriteOffCreate,
    WriteOffItemCreate,
    WriteOffItemResponse,
    WriteOffResponse,
    WriteOffUpdate,
)

__all__ = [
    # Category
    "CategoryCreate",
    "CategoryResponse",
    "CategoryTreeResponse",
    "CategoryUpdate",
    "DebtPaymentInfo",
    "InvoiceConfirmRequest",
    # Invoice
    "InvoiceCreate",
    "InvoiceItemCreate",
    "InvoiceItemResponse",
    "InvoiceResponse",
    "InvoiceUpdate",
    # Product
    "ProductCreate",
    "ProductListResponse",
    "ProductResponse",
    "ProductSearchParams",
    "ProductUpdate",
    # Receipt
    "ReceiptCreate",
    "ReceiptHistoryParams",
    "ReceiptItemCreate",
    "ReceiptItemResponse",
    "ReceiptResponse",
    "ReturnInvoiceConfirmRequest",
    # ReturnInvoice
    "ReturnInvoiceCreate",
    "ReturnInvoiceItemCreate",
    "ReturnInvoiceItemResponse",
    "ReturnInvoiceResponse",
    "ReturnInvoiceUpdate",
    # Supplier
    "SupplierCreate",
    "SupplierLedgerBalanceResponse",
    # Ledger
    "SupplierLedgerCreate",
    "SupplierLedgerResponse",
    "SupplierResponse",
    "SupplierUpdate",
    "TransferConfirmRequest",
    # Transfer
    "TransferCreate",
    "TransferItemCreate",
    "TransferItemResponse",
    "TransferResponse",
    "TransferUpdate",
    # User
    "UserCreate",
    "UserLoginRequest",
    "UserPinLoginRequest",
    "UserResponse",
    "UserTokenResponse",
    "UserUpdate",
    # WriteOff
    "WriteOffCreate",
    "WriteOffItemCreate",
    "WriteOffItemResponse",
    "WriteOffResponse",
    "WriteOffUpdate",
]
