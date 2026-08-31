"""
Infrastructure Layer: SQLAlchemy ORM Models.

Всі моделі даних Torgashka POS визначені тут.
Порядок імпорту важливий — спочатку батьківські моделі (без FK),
потім дочірні (з FK).
"""

# ── Базовий клас ───────────────────────────────
from app.database import Base
from app.infrastructure.persistence.models.barcode import Barcode
from app.infrastructure.persistence.models.category import Category
from app.infrastructure.persistence.models.debtor import Debtor
from app.infrastructure.persistence.models.inventory import Inventory, InventoryItem, InventoryStatus

# ── Документи ──────────────────────────────────
from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem

# ── Права доступу ──────────────────────────────
from app.infrastructure.persistence.models.permission import Permission

# ── Шаблони друку ──────────────────────────────
from app.infrastructure.persistence.models.print_template import PrintTemplate

# ── Товари ─────────────────────────────────────
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.product_image import ProductImage

# ── ПРРО (фіскалізація) ────────────────────────
from app.infrastructure.persistence.models.prro import (
    PrroQueueItem,
    PrroQueueStatus,
    PrroSetting,
    PrroShift,
    PrroShiftStatus,
)
from app.infrastructure.persistence.models.purchase_order import PurchaseOrder, PurchaseOrderItem
from app.infrastructure.persistence.models.reasons import WriteOffReason  # noqa: F401

# ── Продажі ────────────────────────────────────
from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem
from app.infrastructure.persistence.models.return_invoice import ReturnInvoice, ReturnInvoiceItem
from app.infrastructure.persistence.models.supplier import Supplier

# ── Взаєморозрахунки ───────────────────────────
from app.infrastructure.persistence.models.supplier_ledger import SupplierLedger

# ── Системні налаштування ──────────────────────
from app.infrastructure.persistence.models.system_setting import SystemSetting
from app.infrastructure.persistence.models.transfer import Transfer, TransferItem

# ── Довідники (без зовнішніх ключів) ──────────
from app.infrastructure.persistence.models.user import User, UserRole

# ── Облік робочого часу ────────────────────────
from app.infrastructure.persistence.models.work_session import WorkSession
from app.infrastructure.persistence.models.write_off import WriteOff, WriteOffItem

# ── Аліаси для зворотної сумісності (використовуються в репозиторіях) ─────
ProductModel = Product
CategoryModel = Category
SupplierModel = Supplier
UserModel = User
InvoiceModel = Invoice
InvoiceItemModel = InvoiceItem
ReceiptModel = Receipt
SupplierLedgerModel = SupplierLedger

# ── Список усіх моделей для зручності ─────────
__all__ = [
    "Barcode",
    "Base",
    "Category",
    "CategoryModel",
    "Debtor",
    "Inventory",
    "InventoryItem",
    "InventoryStatus",
    # Документи
    "Invoice",
    "InvoiceItem",
    "InvoiceItemModel",
    "InvoiceModel",
    # Права доступу
    "Permission",
    # Шаблони друку
    "PrintTemplate",
    # Товари
    "Product",
    "ProductImage",
    # Аліаси
    "ProductModel",
    "PrroQueueItem",
    "PrroQueueStatus",
    # ПРРО (фіскалізація)
    "PrroSetting",
    "PrroShift",
    "PrroShiftStatus",
    "PurchaseOrder",
    "PurchaseOrderItem",
    # Продажі
    "Receipt",
    "ReceiptItem",
    "ReceiptModel",
    "ReturnInvoice",
    "ReturnInvoiceItem",
    "Supplier",
    # Взаєморозрахунки
    "SupplierLedger",
    "SupplierLedgerModel",
    "SupplierModel",
    # Системні налаштування
    "SystemSetting",
    "Transfer",
    "TransferItem",
    # Довідники
    "User",
    "UserModel",
    "UserRole",
    # Облік робочого часу
    "WorkSession",
    "WriteOff",
    "WriteOffItem",
]
