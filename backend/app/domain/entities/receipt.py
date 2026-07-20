"""
Domain Entity: Receipt (Чек продажу).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
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
    """

    id: UUID = field(default_factory=uuid4)
    number: str = ""
    items: list[ReceiptItem] = field(default_factory=list)
    total: Optional[Money] = None
    payment_method: PaymentMethod = PaymentMethod.CASH
    created_at: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    cash_amount: Optional[Money] = None
    card_amount: Optional[Money] = None
    change_amount: Optional[Money] = None
    customer_id: Optional[UUID] = None
    notes: str = ""

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
            f"payment={self.payment_method.value})"
        )
