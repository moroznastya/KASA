"""
Infrastructure Layer: ORM Models.

Реекспорт SQLAlchemy моделей з app.models для використання
в Infrastructure Layer. Це дозволяє репозиторіям та Unit of Work
імпортувати моделі без циркулярних залежностей.

Всі моделі визначені в app.models/ та імпортуються звідти.
"""

# ─── Імпортуємо всі моделі з app.models ─────────────────────────────────────
from app.models.product import Product as ProductModel
from app.models.category import Category as CategoryModel
from app.models.supplier import Supplier as SupplierModel
from app.models.user import User as UserModel
from app.models.invoice import Invoice as InvoiceModel
from app.models.invoice import InvoiceItem as InvoiceItemModel
from app.models.receipt import Receipt as ReceiptModel
from app.models.receipt import ReceiptItem as ReceiptItemModel
from app.models.supplier_ledger import SupplierLedger as SupplierLedgerModel
from app.models.barcode import Barcode as BarcodeModel
from app.models.product_image import ProductImage as ProductImageModel
from app.models.transfer import Transfer as TransferModel
from app.models.transfer import TransferItem as TransferItemModel
from app.models.write_off import WriteOff as WriteOffModel
from app.models.write_off import WriteOffItem as WriteOffItemModel
from app.models.return_invoice import ReturnInvoice as ReturnInvoiceModel
from app.models.return_invoice import ReturnInvoiceItem as ReturnInvoiceItemModel

# ─── Базовий клас ───────────────────────────────────────────────────────────
from app.database import Base

__all__ = [
    "Base",
    "ProductModel",
    "CategoryModel",
    "SupplierModel",
    "UserModel",
    "InvoiceModel",
    "InvoiceItemModel",
    "ReceiptModel",
    "ReceiptItemModel",
    "SupplierLedgerModel",
    "BarcodeModel",
    "ProductImageModel",
    "TransferModel",
    "TransferItemModel",
    "WriteOffModel",
    "WriteOffItemModel",
    "ReturnInvoiceModel",
    "ReturnInvoiceItemModel",
]
