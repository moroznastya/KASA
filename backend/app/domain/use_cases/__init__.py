"""Use Cases для доменного шару Torgashka POS."""

from .category_use_cases import (
    CreateCategoryUseCase,
    DeleteCategoryUseCase,
    UpdateCategoryUseCase,
)
from .invoice_use_cases import (
    CancelInvoiceUseCase,
    ConfirmInvoiceUseCase,
    CreateInvoiceUseCase,
    InvoiceItemCreate,
)
from .product_use_cases import (
    CreateProductUseCase,
    DeleteProductUseCase,
    PaginatedResult,
    SearchProductsUseCase,
    UpdateProductUseCase,
)
from .receipt_use_cases import (
    CreateReceiptUseCase,
    ReceiptItemCreate,
    ReturnItemCreate,
    ReturnReceiptUseCase,
)

__all__ = [
    "CancelInvoiceUseCase",
    "ConfirmInvoiceUseCase",
    # Category
    "CreateCategoryUseCase",
    # Invoice
    "CreateInvoiceUseCase",
    # Product
    "CreateProductUseCase",
    # Receipt
    "CreateReceiptUseCase",
    "DeleteCategoryUseCase",
    "DeleteProductUseCase",
    "InvoiceItemCreate",
    "PaginatedResult",
    "ReceiptItemCreate",
    "ReturnItemCreate",
    "ReturnReceiptUseCase",
    "SearchProductsUseCase",
    "UpdateCategoryUseCase",
    "UpdateProductUseCase",
]
