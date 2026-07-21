"""
Модель Permission — перелік доступних прав доступу в системі.

Кожне право відповідає певному модулю або функції системи.
Права зберігаються в полі `permissions` моделі User у вигляді JSONB списку.
"""

from enum import Enum as PyEnum


class Permission(str, PyEnum):
    """
    Перелік прав доступу в системі Kasa.

    Кожне право дозволяє виконувати певні дії в системі.
    Права можна комбінувати — користувач може мати будь-який набір прав.
    """

    # ── Товари та категорії ─────────────────────
    PRODUCTS_VIEW = "products:view"
    """Перегляд списку товарів та карток товарів."""
    PRODUCTS_CREATE = "products:create"
    """Створення нових товарів."""
    PRODUCTS_EDIT = "products:edit"
    """Редагування існуючих товарів."""
    PRODUCTS_DELETE = "products:delete"
    """Видалення товарів."""

    CATEGORIES_VIEW = "categories:view"
    """Перегляд списку категорій."""
    CATEGORIES_CREATE = "categories:create"
    """Створення нових категорій."""
    CATEGORIES_EDIT = "categories:edit"
    """Редагування категорій."""
    CATEGORIES_DELETE = "categories:delete"
    """Видалення категорій."""

    # ── Постачальники ───────────────────────────
    SUPPLIERS_VIEW = "suppliers:view"
    """Перегляд списку постачальників."""
    SUPPLIERS_CREATE = "suppliers:create"
    """Створення нових постачальників."""
    SUPPLIERS_EDIT = "suppliers:edit"
    """Редагування постачальників."""
    SUPPLIERS_DELETE = "suppliers:delete"
    """Видалення постачальників."""

    # ── Документи (накладні, переміщення, списання) ──
    DOCUMENTS_VIEW = "documents:view"
    """Перегляд списку документів."""
    INVOICES_CREATE = "invoices:create"
    """Створення прибуткових накладних."""
    INVOICES_EDIT = "invoices:edit"
    """Редагування прибуткових накладних."""
    INVOICES_DELETE = "invoices:delete"
    """Видалення прибуткових накладних."""
    INVOICES_CONFIRM = "invoices:confirm"
    """Підтвердження прибуткових накладних."""

    TRANSFERS_CREATE = "transfers:create"
    """Створення переміщень."""
    TRANSFERS_EDIT = "transfers:edit"
    """Редагування переміщень."""
    TRANSFERS_DELETE = "transfers:delete"
    """Видалення переміщень."""
    TRANSFERS_CONFIRM = "transfers:confirm"
    """Підтвердження переміщень."""

    WRITE_OFFS_CREATE = "write-offs:create"
    """Створення списань."""
    WRITE_OFFS_EDIT = "write-offs:edit"
    """Редагування списань."""
    WRITE_OFFS_DELETE = "write-offs:delete"
    """Видалення списань."""
    WRITE_OFFS_CONFIRM = "write-offs:confirm"
    """Підтвердження списань."""

    RETURNS_CREATE = "returns:create"
    """Створення повернень постачальнику."""
    RETURNS_EDIT = "returns:edit"
    """Редагування повернень."""
    RETURNS_DELETE = "returns:delete"
    """Видалення повернень."""
    RETURNS_CONFIRM = "returns:confirm"
    """Підтвердження повернень."""

    # ── POS-каса ─────────────────────────────────
    POS_ACCESS = "pos:access"
    """Доступ до POS-каси (пробивання чеків)."""
    RECEIPTS_VIEW = "receipts:view"
    """Перегляд чеків продажу."""
    RECEIPTS_CANCEL = "receipts:cancel"
    """Скасування чеків продажу."""

    # ── Боржники ────────────────────────────────
    DEBTORS_VIEW = "debtors:view"
    """Перегляд списку боржників."""
    DEBTORS_CREATE = "debtors:create"
    """Створення записів боржників."""
    DEBTORS_EDIT = "debtors:edit"
    """Редагування записів боржників."""
    DEBTORS_DELETE = "debtors:delete"
    """Видалення записів боржників."""
    DEBTORS_PAY = "debtors:pay"
    """Прийом оплати від боржників."""

    # ── Взаєморозрахунки ────────────────────────
    LEDGER_VIEW = "ledger:view"
    """Перегляд взаєморозрахунків з постачальниками."""
    LEDGER_CREATE = "ledger:create"
    """Створення записів у взаєморозрахунках."""

    # ── Звіти ────────────────────────────────────
    REPORTS_VIEW = "reports:view"
    """Перегляд звітів."""
    REPORTS_STATS = "reports:stats"
    """Перегляд статистики (денна виручка тощо)."""

    # ── Користувачі ─────────────────────────────
    USERS_VIEW = "users:view"
    """Перегляд списку користувачів."""
    USERS_CREATE = "users:create"
    """Створення нових користувачів."""
    USERS_EDIT = "users:edit"
    """Редагування користувачів."""
    USERS_DELETE = "users:delete"
    """Видалення користувачів."""
    USERS_MANAGE_PERMISSIONS = "users:manage-permissions"
    """Управління правами доступу користувачів."""


# ── Українські назви для прав ────────────────────────────────────────────────

PERMISSION_LABELS: dict[str, str] = {
    Permission.PRODUCTS_VIEW.value: "Перегляд товарів",
    Permission.PRODUCTS_CREATE.value: "Створення товарів",
    Permission.PRODUCTS_EDIT.value: "Редагування товарів",
    Permission.PRODUCTS_DELETE.value: "Видалення товарів",
    Permission.CATEGORIES_VIEW.value: "Перегляд категорій",
    Permission.CATEGORIES_CREATE.value: "Створення категорій",
    Permission.CATEGORIES_EDIT.value: "Редагування категорій",
    Permission.CATEGORIES_DELETE.value: "Видалення категорій",
    Permission.SUPPLIERS_VIEW.value: "Перегляд постачальників",
    Permission.SUPPLIERS_CREATE.value: "Створення постачальників",
    Permission.SUPPLIERS_EDIT.value: "Редагування постачальників",
    Permission.SUPPLIERS_DELETE.value: "Видалення постачальників",
    Permission.DOCUMENTS_VIEW.value: "Перегляд документів",
    Permission.INVOICES_CREATE.value: "Створення накладних",
    Permission.INVOICES_EDIT.value: "Редагування накладних",
    Permission.INVOICES_DELETE.value: "Видалення накладних",
    Permission.INVOICES_CONFIRM.value: "Підтвердження накладних",
    Permission.TRANSFERS_CREATE.value: "Створення переміщень",
    Permission.TRANSFERS_EDIT.value: "Редагування переміщень",
    Permission.TRANSFERS_DELETE.value: "Видалення переміщень",
    Permission.TRANSFERS_CONFIRM.value: "Підтвердження переміщень",
    Permission.WRITE_OFFS_CREATE.value: "Створення списань",
    Permission.WRITE_OFFS_EDIT.value: "Редагування списань",
    Permission.WRITE_OFFS_DELETE.value: "Видалення списань",
    Permission.WRITE_OFFS_CONFIRM.value: "Підтвердження списань",
    Permission.RETURNS_CREATE.value: "Створення повернень",
    Permission.RETURNS_EDIT.value: "Редагування повернень",
    Permission.RETURNS_DELETE.value: "Видалення повернень",
    Permission.RETURNS_CONFIRM.value: "Підтвердження повернень",
    Permission.POS_ACCESS.value: "Доступ до POS-каси",
    Permission.RECEIPTS_VIEW.value: "Перегляд чеків",
    Permission.RECEIPTS_CANCEL.value: "Скасування чеків",
    Permission.DEBTORS_VIEW.value: "Перегляд боржників",
    Permission.DEBTORS_CREATE.value: "Створення боржників",
    Permission.DEBTORS_EDIT.value: "Редагування боржників",
    Permission.DEBTORS_DELETE.value: "Видалення боржників",
    Permission.DEBTORS_PAY.value: "Прийом оплати від боржників",
    Permission.LEDGER_VIEW.value: "Перегляд взаєморозрахунків",
    Permission.LEDGER_CREATE.value: "Створення записів у взаєморозрахунках",
    Permission.REPORTS_VIEW.value: "Перегляд звітів",
    Permission.REPORTS_STATS.value: "Перегляд статистики",
    Permission.USERS_VIEW.value: "Перегляд користувачів",
    Permission.USERS_CREATE.value: "Створення користувачів",
    Permission.USERS_EDIT.value: "Редагування користувачів",
    Permission.USERS_DELETE.value: "Видалення користувачів",
    Permission.USERS_MANAGE_PERMISSIONS.value: "Управління правами доступу",
}


# ── Групи прав для ролей ─────────────────────────────────────────────────────

# Права за замовчуванням для адміністратора (всі права)
ADMIN_PERMISSIONS = [p.value for p in Permission]

# Права за замовчуванням для касира
CASHIER_PERMISSIONS = [
    Permission.PRODUCTS_VIEW.value,
    Permission.CATEGORIES_VIEW.value,
    Permission.SUPPLIERS_VIEW.value,
    Permission.DOCUMENTS_VIEW.value,
    Permission.INVOICES_CREATE.value,
    Permission.POS_ACCESS.value,
    Permission.RECEIPTS_VIEW.value,
    Permission.DEBTORS_VIEW.value,
    Permission.DEBTORS_CREATE.value,
    Permission.DEBTORS_PAY.value,
    Permission.LEDGER_VIEW.value,
    Permission.REPORTS_STATS.value,
]


# ── Мапінг прав на модулі для фронтенду ──────────────────────────────────────

PERMISSION_GROUPS = {
    "Товари": {
        "icon": "Package",
        "permissions": [
            Permission.PRODUCTS_VIEW,
            Permission.PRODUCTS_CREATE,
            Permission.PRODUCTS_EDIT,
            Permission.PRODUCTS_DELETE,
        ],
    },
    "Категорії": {
        "icon": "Tags",
        "permissions": [
            Permission.CATEGORIES_VIEW,
            Permission.CATEGORIES_CREATE,
            Permission.CATEGORIES_EDIT,
            Permission.CATEGORIES_DELETE,
        ],
    },
    "Постачальники": {
        "icon": "Truck",
        "permissions": [
            Permission.SUPPLIERS_VIEW,
            Permission.SUPPLIERS_CREATE,
            Permission.SUPPLIERS_EDIT,
            Permission.SUPPLIERS_DELETE,
        ],
    },
    "Прибуткові накладні": {
        "icon": "FileText",
        "permissions": [
            Permission.DOCUMENTS_VIEW,
            Permission.INVOICES_CREATE,
            Permission.INVOICES_EDIT,
            Permission.INVOICES_DELETE,
            Permission.INVOICES_CONFIRM,
        ],
    },
    "Переміщення": {
        "icon": "ArrowRightLeft",
        "permissions": [
            Permission.TRANSFERS_CREATE,
            Permission.TRANSFERS_EDIT,
            Permission.TRANSFERS_DELETE,
            Permission.TRANSFERS_CONFIRM,
        ],
    },
    "Списання": {
        "icon": "Trash2",
        "permissions": [
            Permission.WRITE_OFFS_CREATE,
            Permission.WRITE_OFFS_EDIT,
            Permission.WRITE_OFFS_DELETE,
            Permission.WRITE_OFFS_CONFIRM,
        ],
    },
    "Повернення постачальнику": {
        "icon": "Undo2",
        "permissions": [
            Permission.RETURNS_CREATE,
            Permission.RETURNS_EDIT,
            Permission.RETURNS_DELETE,
            Permission.RETURNS_CONFIRM,
        ],
    },
    "POS-каса": {
        "icon": "ShoppingCart",
        "permissions": [
            Permission.POS_ACCESS,
            Permission.RECEIPTS_VIEW,
            Permission.RECEIPTS_CANCEL,
        ],
    },
    "Боржники": {
        "icon": "Users",
        "permissions": [
            Permission.DEBTORS_VIEW,
            Permission.DEBTORS_CREATE,
            Permission.DEBTORS_EDIT,
            Permission.DEBTORS_DELETE,
            Permission.DEBTORS_PAY,
        ],
    },
    "Взаєморозрахунки": {
        "icon": "BookOpen",
        "permissions": [
            Permission.LEDGER_VIEW,
            Permission.LEDGER_CREATE,
        ],
    },
    "Звіти": {
        "icon": "BarChart3",
        "permissions": [
            Permission.REPORTS_VIEW,
            Permission.REPORTS_STATS,
        ],
    },
    "Користувачі": {
        "icon": "UserCog",
        "permissions": [
            Permission.USERS_VIEW,
            Permission.USERS_CREATE,
            Permission.USERS_EDIT,
            Permission.USERS_DELETE,
            Permission.USERS_MANAGE_PERMISSIONS,
        ],
    },
}
