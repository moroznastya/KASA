"""Use Cases для доменного шару Kasa POS."""

from .category_use_cases import (
    CreateCategoryUseCase,
    UpdateCategoryUseCase,
    DeleteCategoryUseCase,
)
from .product_use_cases import (
    CreateProductUseCase,
    UpdateProductUseCase,
    DeleteProductUseCase,
    SearchProductsUseCase,
    PaginatedResult,
)
from .invoice_use_cases import (
    CreateInvoiceUseCase,
    ConfirmInvoiceUseCase,
    CancelInvoiceUseCase,
    InvoiceItemCreate,
)
from .receipt_use_cases import (
    CreateReceiptUseCase,
    ReturnReceiptUseCase,
    ReceiptItemCreate,
    ReturnItemCreate,
)

__all__ = [
    # Category
    "CreateCategoryUseCase",
    "UpdateCategoryUseCase",
    "DeleteCategoryUseCase",
    # Product
    "CreateProductUseCase",
    "UpdateProductUseCase",
    "DeleteProductUseCase",
    "SearchProductsUseCase",
    "PaginatedResult",
    # Invoice
    "CreateInvoiceUseCase",
    "ConfirmInvoiceUseCase",
    "CancelInvoiceUseCase",
    "InvoiceItemCreate",
    # Receipt
    "CreateReceiptUseCase",
    "ReturnReceiptUseCase",
    "ReceiptItemCreate",
    "ReturnItemCreate",
]
