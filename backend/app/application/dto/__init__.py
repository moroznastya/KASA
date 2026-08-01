"""Data Transfer Objects для Application Layer."""

from .product_dto import ProductDTO, ProductCreateDTO, ProductUpdateDTO
from .invoice_dto import InvoiceDTO, InvoiceCreateDTO, InvoiceConfirmDTO
from .receipt_dto import ReceiptDTO, ReceiptCreateDTO
from .user_dto import UserDTO, UserCreateDTO
from .supplier_dto import SupplierDTO, SupplierCreateDTO
from .ledger_dto import LedgerEntryDTO, LedgerCreateDTO
from .prro_dto import (
    PrroSettingsDTO,
    PrroShiftDTO,
    FiscalizeRequestDTO,
    FiscalizeResponseDTO,
    PrroStatusDTO,
    OpenShiftRequestDTO,
    CloseShiftRequestDTO,
    PrroQueueItemDTO,
)

__all__ = [
    "ProductDTO",
    "ProductCreateDTO",
    "ProductUpdateDTO",
    "InvoiceDTO",
    "InvoiceCreateDTO",
    "InvoiceConfirmDTO",
    "ReceiptDTO",
    "ReceiptCreateDTO",
    "UserDTO",
    "UserCreateDTO",
    "SupplierDTO",
    "SupplierCreateDTO",
    "LedgerEntryDTO",
    "LedgerCreateDTO",
    # ПРРО
    "PrroSettingsDTO",
    "PrroShiftDTO",
    "FiscalizeRequestDTO",
    "FiscalizeResponseDTO",
    "PrroStatusDTO",
    "OpenShiftRequestDTO",
    "CloseShiftRequestDTO",
    "PrroQueueItemDTO",
]
