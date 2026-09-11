"""
Domain Entity: Receipt (Чек продажу).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime
from decimal import Decimal
from enum import Enum
from typing import Optional
from uuid import UUID, uuid4

from ..value_objects.money import Money
from ..value_objects.quantity import Quantity
from ..value_objects.tax_rate import TaxRate


class PaymentMethod(Enum):
    """Спосіб оплати."""
    CASH = "cash"
    CARD = "card"
    BANK_TRANSFER = "bank_transfer"
    CREDIT = "credit"
    MIXED = "mixed"


class FiscalStatus(Enum):
    """Статус відправки фіскального чеку у податкову."""
    NONE = "none"          # Чек не фіскальний
    PENDING = "pending"    # Очікує відправки у податкову
    SENT = "sent"          # Успішно відправлено
    FAILED = "failed"      # Помилка при відправці


@dataclass
class ReceiptItem:
    """Позиція чеку продажу."""

    product_id: UUID
    name: str
    quantity: Quantity
    price: Money
    tax_rate: TaxRate = field(default_factory=TaxRate.standard)

    @property
    def total(self) -> Money:
        """Загальна сума позиції (ціна × кількість)."""
        return self.price * float(self.quantity.value)

    @property
    def total_with_tax(self) -> Money:
        """Загальна сума з ПДВ."""
        gross = self.tax_rate.calculate_gross(self.total.amount)
        return Money(gross, self.total.currency)

    @property
    def tax_amount(self) -> Money:
        """Сума ПДВ для позиції."""
        tax = self.tax_rate.apply_to(self.total.amount)
        return Money(tax, self.total.currency)


@dataclass
class Receipt:
    """
    Чек продажу (Receipt aggregate root).

    Відповідає за:
    - Фіксацію факту продажу товарів
    - Розрахунок загальної суми
    - Облік способу оплати
    - Фіскалізацію: формування окремих фіскальних чеків тільки
      з товарів, що надійшли з фіскальних накладних, та відправку їх
      у податкову (is_fiscal, fiscal_status, fiscal_number, ...)
    """

    id: UUID = field(default_factory=uuid4)
    number: str = ""
    items: list[ReceiptItem] = field(default_factory=list)
    total: Optional[Money] = None
    payment_method: PaymentMethod = PaymentMethod.CASH
    receipt_type: str = "sale"
    """Тип чеку: 'sale' (продаж) або 'return' (повернення)."""
    cashier_id: Optional[UUID] = None
    """ID касира, який створив чек."""
    created_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    cash_amount: Optional[Money] = None
    card_amount: Optional[Money] = None
    change_amount: Optional[Money] = None
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
    fiscal_status: FiscalStatus = FiscalStatus.NONE
    """Статус відправки фіскального чеку у податкову."""
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

    def add_item(self, item: ReceiptItem) -> None:
        """
        Додає позицію до чеку.

        Args:
            item: Позиція чеку.
        """
        self.items.append(item)
        self._recalculate_total()

    def remove_item(self, product_id: UUID) -> None:
        """
        Видаляє позицію з чеку.

        Args:
            product_id: ID товару для видалення.
        """
        self.items = [item for item in self.items if item.product_id != product_id]
        self._recalculate_total()

    def set_payment(self, method: PaymentMethod, amount: Money) -> None:
        """
        Встановлює спосіб оплати.

        Args:
            method: Спосіб оплати.
            amount: Сума оплати.

        Raises:
            ValueError: Якщо сума оплати менша за загальну суму.
        """
        if self.total and amount < self.total:
            raise ValueError(
                f"Payment amount {amount} is less than total {self.total}"
            )
        self.payment_method = method
        if method == PaymentMethod.CASH:
            self.cash_amount = amount
            if self.total:
                self.change_amount = amount - self.total
        elif method == PaymentMethod.CARD:
            self.card_amount = amount
        elif method == PaymentMethod.MIXED:
            # Для змішаної оплати суми встановлюються окремо
            pass

    def set_mixed_payment(self, cash: Money, card: Money) -> None:
        """
        Встановлює змішану оплату (готівка + картка).

        Args:
            cash: Сума готівкою.
            card: Сума карткою.

        Raises:
            ValueError: Якщо загальна сума оплати менша за суму чеку.
        """
        total_paid = cash + card
        if self.total and total_paid < self.total:
            raise ValueError(
                f"Total payment {total_paid} is less than receipt total {self.total}"
            )
        self.payment_method = PaymentMethod.MIXED
        self.cash_amount = cash
        self.card_amount = card
        if self.total:
            self.change_amount = total_paid - self.total

    # ── Фіскалізація ─────────────────────────────────────────────────────────

    def mark_as_fiscal(self) -> None:
        """
        Позначає чек як фіскальний (містить товари з фіскальних накладних)
        та переводить його в статус очікування відправки у податкову.
        """
        self.is_fiscal = True
        self.fiscal_status = FiscalStatus.PENDING

    def mark_fiscal_pending(self) -> None:
        """Переводить фіскальний чек у статус очікування відправки."""
        self.fiscal_status = FiscalStatus.PENDING
        self.fiscal_error = None

    def mark_fiscal_sent(self, fiscal_number: str, fiscal_serial: str) -> None:
        """
        Позначає фіскальний чек як успішно відправлений у податкову.

        Args:
            fiscal_number: Фіскальний номер, присвоєний податковою.
            fiscal_serial: Фіскальний серійний номер.
        """
        self.fiscal_status = FiscalStatus.SENT
        self.fiscal_number = fiscal_number
        self.fiscal_serial = fiscal_serial
        self.fiscal_sent_at = datetime.now(UTC)
        self.fiscal_error = None

    def mark_fiscal_failed(self, error: str) -> None:
        """
        Позначає фіскальний чек як помилку при відправці.

        Args:
            error: Текст помилки.
        """
        self.fiscal_status = FiscalStatus.FAILED
        self.fiscal_error = error

    def _recalculate_total(self) -> None:
        """Перераховує загальну суму чеку."""
        if not self.items:
            self.total = Money.zero()
            return
        total_amount = sum(
            (item.total.amount for item in self.items),
            Decimal("0.00"),
        )
        currency = self.items[0].total.currency if self.items else "UAH"
        self.total = Money(total_amount, currency)

    @property
    def total_with_tax(self) -> Money:
        """Загальна сума з ПДВ."""
        if not self.items:
            return Money.zero()
        total = sum(
            (item.total_with_tax.amount for item in self.items),
            Decimal("0.00"),
        )
        currency = self.items[0].total.currency if self.items else "UAH"
        return Money(total, currency)

    @property
    def total_tax(self) -> Money:
        """Загальна сума ПДВ."""
        if not self.items:
            return Money.zero()
        total = sum(
            (item.tax_amount.amount for item in self.items),
            Decimal("0.00"),
        )
        currency = self.items[0].total.currency if self.items else "UAH"
        return Money(total, currency)

    @property
    def item_count(self) -> int:
        """Кількість позицій у чеку."""
        return len(self.items)

    @property
    def total_quantity(self) -> Decimal:
        """Загальна кількість товарів у чеку."""
        return sum((item.quantity.value for item in self.items), Decimal("0"))

    def __str__(self) -> str:
        return f"Receipt(id={self.id}, number='{self.number}', total={self.total})"

    def __repr__(self) -> str:
        return (
            f"Receipt(id={self.id}, number='{self.number}', "
            f"items={len(self.items)}, total={self.total}, "
            f"payment={self.payment_method.value}, "
            f"is_fiscal={self.is_fiscal}, fiscal_status={self.fiscal_status.value})"
        )
