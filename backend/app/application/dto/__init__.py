"""Data Transfer Objects для Application Layer."""

from .invoice_dto import InvoiceConfirmDTO, InvoiceCreateDTO, InvoiceDTO
from .ledger_dto import LedgerCreateDTO, LedgerEntryDTO
from .product_dto import ProductCreateDTO, ProductDTO, ProductUpdateDTO
from .prro_dto import (
    CloseShiftRequestDTO,
    FiscalizeRequestDTO,
    FiscalizeResponseDTO,
    OpenShiftRequestDTO,
    PrroQueueItemDTO,
    PrroSettingsDTO,
    PrroShiftDTO,
    PrroStatusDTO,
)
from .receipt_dto import ReceiptCreateDTO, ReceiptDTO
from .supplier_dto import SupplierCreateDTO, SupplierDTO
from .user_dto import UserCreateDTO, UserDTO

__all__ = [
    "CloseShiftRequestDTO",
    "FiscalizeRequestDTO",
    "FiscalizeResponseDTO",
    "InvoiceConfirmDTO",
    "InvoiceCreateDTO",
    "InvoiceDTO",
    "LedgerCreateDTO",
    "LedgerEntryDTO",
    "OpenShiftRequestDTO",
    "ProductCreateDTO",
    "ProductDTO",
    "ProductUpdateDTO",
    "PrroQueueItemDTO",
    # ПРРО
    "PrroSettingsDTO",
    "PrroShiftDTO",
    "PrroStatusDTO",
    "ReceiptCreateDTO",
    "ReceiptDTO",
    "SupplierCreateDTO",
    "SupplierDTO",
    "UserCreateDTO",
    "UserDTO",
]
