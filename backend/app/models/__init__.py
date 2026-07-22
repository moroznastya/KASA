"""
Ініціалізація всіх моделей даних Kasa POS.

Імпортує всі моделі для реєстрації в SQLAlchemy MetaData.
Порядок імпорту важливий — спочатку батьківські моделі (без FK),
потім дочірні (з FK).
"""

# ── Довідники (без зовнішніх ключів) ──────────
from app.models.user import User, UserRole  # noqa: F401
from app.models.category import Category  # noqa: F401
from app.models.supplier import Supplier  # noqa: F401
from app.models.debtor import Debtor  # noqa: F401

# ── Товари ─────────────────────────────────────
from app.models.product import Product  # noqa: F401
from app.models.barcode import Barcode  # noqa: F401
from app.models.product_image import ProductImage  # noqa: F401

# ── Документи ──────────────────────────────────
from app.models.invoice import Invoice, InvoiceItem  # noqa: F401
from app.models.transfer import Transfer, TransferItem  # noqa: F401
from app.models.write_off import WriteOff, WriteOffItem  # noqa: F401
from app.models.return_invoice import ReturnInvoice, ReturnInvoiceItem  # noqa: F401
from app.models.purchase_order import PurchaseOrder, PurchaseOrderItem  # noqa: F401

# ── Продажі ────────────────────────────────────
from app.models.receipt import Receipt, ReceiptItem  # noqa: F401

# ── Взаєморозрахунки ───────────────────────────
from app.models.supplier_ledger import SupplierLedger  # noqa: F401

# ── Список усіх моделей для зручності ──────────
__all__ = [
    # Довідники
    "User", "UserRole",
    "Category",
    "Supplier",
    "Debtor",
    # Товари
    "Product",
    "Barcode",
    "ProductImage",
    # Документи
    "Invoice", "InvoiceItem",
    "Transfer", "TransferItem",
    "WriteOff", "WriteOffItem",
    "ReturnInvoice", "ReturnInvoiceItem",
    "PurchaseOrder", "PurchaseOrderItem",
    # Продажі
    "Receipt", "ReceiptItem",
    # Взаєморозрахунки
    "SupplierLedger",
]
