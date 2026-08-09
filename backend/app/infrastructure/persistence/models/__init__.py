"""
Infrastructure Layer: SQLAlchemy ORM Models.

Всі моделі даних Torgashka POS визначені тут.
Порядок імпорту важливий — спочатку батьківські моделі (без FK),
потім дочірні (з FK).
"""

# ── Базовий клас ───────────────────────────────
from app.database import Base  # noqa: F401

# ── Довідники (без зовнішніх ключів) ──────────
from app.infrastructure.persistence.models.user import User, UserRole  # noqa: F401
from app.infrastructure.persistence.models.category import Category  # noqa: F401
from app.infrastructure.persistence.models.supplier import Supplier  # noqa: F401
from app.infrastructure.persistence.models.debtor import Debtor  # noqa: F401

# ── Товари ─────────────────────────────────────
from app.infrastructure.persistence.models.product import Product  # noqa: F401
from app.infrastructure.persistence.models.barcode import Barcode  # noqa: F401
from app.infrastructure.persistence.models.product_image import ProductImage  # noqa: F401

# ── Документи ──────────────────────────────────
from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem  # noqa: F401
from app.infrastructure.persistence.models.transfer import Transfer, TransferItem  # noqa: F401
from app.infrastructure.persistence.models.reasons import WriteOffReason  # noqa: F401
from app.infrastructure.persistence.models.write_off import WriteOff, WriteOffItem  # noqa: F401
from app.infrastructure.persistence.models.return_invoice import ReturnInvoice, ReturnInvoiceItem  # noqa: F401
from app.infrastructure.persistence.models.inventory import Inventory, InventoryItem, InventoryStatus  # noqa: F401
from app.infrastructure.persistence.models.purchase_order import PurchaseOrder, PurchaseOrderItem  # noqa: F401

# ── Продажі ────────────────────────────────────
from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem  # noqa: F401

# ── Взаєморозрахунки ───────────────────────────
from app.infrastructure.persistence.models.supplier_ledger import SupplierLedger  # noqa: F401

# ── Системні налаштування ──────────────────────
from app.infrastructure.persistence.models.system_setting import SystemSetting  # noqa: F401

# ── Облік робочого часу ────────────────────────
from app.infrastructure.persistence.models.work_session import WorkSession  # noqa: F401

# ── Шаблони друку ──────────────────────────────
from app.infrastructure.persistence.models.print_template import PrintTemplate  # noqa: F401

# ── Права доступу ──────────────────────────────
from app.infrastructure.persistence.models.permission import Permission  # noqa: F401

# ── ПРРО (фіскалізація) ────────────────────────
from app.infrastructure.persistence.models.prro import (  # noqa: F401
    PrroSetting,
    PrroShift,
    PrroShiftStatus,
    PrroQueueItem,
    PrroQueueStatus,
)

# ── Аліаси для зворотної сумісності (використовуються в репозиторіях) ─────
ProductModel = Product  # noqa: F401
CategoryModel = Category  # noqa: F401
SupplierModel = Supplier  # noqa: F401
UserModel = User  # noqa: F401
InvoiceModel = Invoice  # noqa: F401
InvoiceItemModel = InvoiceItem  # noqa: F401
ReceiptModel = Receipt  # noqa: F401
SupplierLedgerModel = SupplierLedger  # noqa: F401

# ── Список усіх моделей для зручності ─────────
__all__ = [
    "Base",
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
    "Inventory", "InventoryItem", "InventoryStatus",
    "PurchaseOrder", "PurchaseOrderItem",
    # Продажі
    "Receipt", "ReceiptItem",
    # Взаєморозрахунки
    "SupplierLedger",
    # Системні налаштування
    "SystemSetting",
    # Облік робочого часу
    "WorkSession",
    # Шаблони друку
    "PrintTemplate",
    # Права доступу
    "Permission",
    # ПРРО (фіскалізація)
    "PrroSetting",
    "PrroShift", "PrroShiftStatus",
    "PrroQueueItem", "PrroQueueStatus",
    # Аліаси
    "ProductModel",
    "CategoryModel",
    "SupplierModel",
    "UserModel",
    "InvoiceModel",
    "InvoiceItemModel",
    "ReceiptModel",
    "SupplierLedgerModel",
]
