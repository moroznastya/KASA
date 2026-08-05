"""
DTO для Receipt (Чек продажу).

Використовуються для передачі даних між Application та Presentation шарами.
"""

from dataclasses import dataclass, field
from datetime import datetime
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class ReceiptItemDTO:
    """DTO позиції чеку продажу."""
    product_id: UUID
    name: str
    quantity: Decimal
    price: Decimal
    tax_rate: int = 20


@dataclass
class ReceiptDTO:
    """Повний DTO чеку продажу для відповіді клієнту."""
    id: UUID
    number: str
    items: list[ReceiptItemDTO] = field(default_factory=list)
    total: Optional[Decimal] = None
    payment_method: str = "cash"
    created_at: Optional[datetime] = None
    cash_amount: Optional[Decimal] = None
    card_amount: Optional[Decimal] = None
    change_amount: Optional[Decimal] = None
    customer_id: Optional[UUID] = None
    notes: str = ""
    # ── Дані банківської транзакції карткового терміналу (ПриватБанк) ────
    terminal_rrn: Optional[str] = None
    """RRN транзакції терміналу (унікальний номер транзакції банку)."""
    terminal_approval_code: Optional[str] = None
    """Код авторизації терміналу."""
    terminal_invoice_number: Optional[str] = None
    """Номер чека терміналу."""
    terminal_transaction_id: Optional[str] = None
    """Ідентифікатор транзакції в банку-емітенті (rrnExt / rid)."""
    terminal_response_code: Optional[str] = None
    """ResponseCode відповіді терміналу ("0000" — успіх тощо)."""
    terminal_status: Optional[str] = None
    """Статус транзакції (approved/declined/partial/cancelled)."""
    terminal_receipt: Optional[str] = None
    """Повний текст чека терміналу (для друку)."""
    terminal_card_pan: Optional[str] = None
    """Маскований номер картки (pan)."""
    terminal_payment_system: Optional[str] = None
    """Міжнародна платіжна система (VISA/MasterCard)."""
    terminal_merchant: Optional[str] = None
    """Номер мерчанта."""
    terminal_created_at: Optional[datetime] = None
    """Дата/час транзакції від терміналу."""
    # ── Фіскалізація ────────────────────────────────────────────────────────
    is_fiscal: bool = False
    """Чек є фіскальним (містить лише товари з фіскальних накладних)."""
    fiscal_status: str = "none"
    """Статус відправки фіскального чеку у податкову (none/pending/sent/failed)."""
    fiscal_number: Optional[str] = None
    """Фіскальний номер чеку, присвоєний податковою."""
    fiscal_serial: Optional[str] = None
    """Фіскальний серійний номер."""
    fiscal_sent_at: Optional[datetime] = None
    """Дата/час успішної відправки у податкову."""
    fiscal_error: Optional[str] = None
    """Текст помилки при відправці у податкову."""
    split_group_id: Optional[UUID] = None
    """ID пов'язаного чеку при розділенні фіскальних/нефіскальних позицій."""


@dataclass
class ReceiptCreateDTO:
    """DTO для створення нового чеку продажу."""
    items: list[ReceiptItemDTO] = field(default_factory=list)
    payment_method: str = "cash"
    cash_amount: Optional[Decimal] = None
    card_amount: Optional[Decimal] = None
    customer_id: Optional[UUID] = None
    cashier_id: Optional[UUID] = None
    """ID касира (береться з JWT-токена запиту)."""
    notes: str = ""
    # ── Дані банківської транзакції карткового терміналу (ПриватБанк) ────
    terminal_rrn: Optional[str] = None
    """RRN транзакції терміналу (унікальний номер транзакції банку)."""
    terminal_approval_code: Optional[str] = None
    """Код авторизації терміналу."""
    terminal_invoice_number: Optional[str] = None
    """Номер чека терміналу."""
    terminal_transaction_id: Optional[str] = None
    """Ідентифікатор транзакції в банку-емітенті (rrnExt / rid)."""
    terminal_response_code: Optional[str] = None
    """ResponseCode відповіді терміналу ("0000" — успіх тощо)."""
    terminal_status: Optional[str] = None
    """Статус транзакції (approved/declined/partial/cancelled)."""
    terminal_receipt: Optional[str] = None
    """Повний текст чека терміналу (для друку)."""
    terminal_card_pan: Optional[str] = None
    """Маскований номер картки (pan)."""
    terminal_payment_system: Optional[str] = None
    """Міжнародна платіжна система (VISA/MasterCard)."""
    terminal_merchant: Optional[str] = None
    """Номер мерчанта."""
    terminal_created_at: Optional[datetime] = None
    """Дата/час транзакції від терміналу."""
    # ── Фіскалізація ────────────────────────────────────────────────────────
    is_fiscal: bool = False
    """Запит явно створює фіскальний чек (за замовчуванням визначається автоматично)."""
    split_group_id: Optional[UUID] = None
    """ID пов'язаного чеку при розділенні фіскальних/нефіскальних позицій."""
