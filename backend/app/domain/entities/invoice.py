"""
Domain Entity: Invoice (Прибуткова накладна).

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


class InvoiceStatus(Enum):
    """Статус прибуткової накладної."""
    DRAFT = "draft"
    CONFIRMED = "confirmed"
    CANCELLED = "cancelled"


@dataclass
class InvoiceItem:
    """Позиція прибуткової накладної."""

    product_id: UUID
    quantity: Quantity
    price: Money
    tax_rate: TaxRate = field(default_factory=TaxRate.standard)
    name: str = ""

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
class Invoice:
    """
    Прибуткова накладна (Invoice aggregate root).

    Відповідає за:
    - Облік надходження товарів від постачальника
    - Розрахунок загальної суми
    - Управління статусом (чернетка → підтверджено → скасовано)
    """

    id: UUID = field(default_factory=uuid4)
    number: str = ""
    supplier_id: UUID = field(default_factory=uuid4)
    items: list[InvoiceItem] = field(default_factory=list)
    total: Optional[Money] = None
    status: InvoiceStatus = InvoiceStatus.DRAFT
    created_at: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    confirmed_at: Optional[datetime] = None
    notes: str = ""
    # ── Фіскальне оприбуткування ───────────────────────────────────────────
    is_fiscal: bool = False
    """Ознака: товар з накладної надходить у фіскальний залишок (fiscal_stock)."""

    def add_item(self, item: InvoiceItem) -> None:
        """
        Додає позицію до накладної.

        Args:
            item: Позиція накладної.

        Raises:
            ValueError: Якщо накладна вже підтверджена.
        """
        if self.status != InvoiceStatus.DRAFT:
            raise ValueError("Cannot add items to a non-draft invoice")
        self.items.append(item)
        self._recalculate_total()

    def remove_item(self, product_id: UUID) -> None:
        """
        Видаляє позицію з накладної.

        Args:
            product_id: ID товару для видалення.

        Raises:
            ValueError: Якщо накладна вже підтверджена.
        """
        if self.status != InvoiceStatus.DRAFT:
            raise ValueError("Cannot remove items from a non-draft invoice")
        self.items = [item for item in self.items if item.product_id != product_id]
        self._recalculate_total()

    def confirm(self) -> None:
        """
        Підтверджує накладну.

        Змінює статус на CONFIRMED.
        Публікує подію InvoiceConfirmed.

        Raises:
            ValueError: Якщо накладна не в статусі DRAFT.
        """
        if self.status != InvoiceStatus.DRAFT:
            raise ValueError(
                f"Cannot confirm invoice in status: {self.status.value}"
            )
        if not self.items:
            raise ValueError("Cannot confirm invoice with no items")
        self.status = InvoiceStatus.CONFIRMED
        self.confirmed_at = datetime.now(timezone.utc)
        self._recalculate_total()

    def cancel(self) -> None:
        """
        Скасовує накладну.

        Змінює статус на CANCELLED.
        Публікує подію InvoiceCancelled.

        Raises:
            ValueError: Якщо накладна не в статусі CONFIRMED.
        """
        if self.status != InvoiceStatus.CONFIRMED:
            raise ValueError(
                f"Cannot cancel invoice in status: {self.status.value}"
            )
        self.status = InvoiceStatus.CANCELLED

    def _recalculate_total(self) -> None:
        """Перераховує загальну суму накладної."""
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
    def is_confirmed(self) -> bool:
        """Чи підтверджена накладна."""
        return self.status == InvoiceStatus.CONFIRMED

    @property
    def is_cancelled(self) -> bool:
        """Чи скасована накладна."""
        return self.status == InvoiceStatus.CANCELLED

    @property
    def is_draft(self) -> bool:
        """Чи є накладна чернеткою."""
        return self.status == InvoiceStatus.DRAFT

    def __str__(self) -> str:
        return f"Invoice(id={self.id}, number='{self.number}', status={self.status.value})"

    def __repr__(self) -> str:
        return (
            f"Invoice(id={self.id}, number='{self.number}', "
            f"items={len(self.items)}, total={self.total}, "
            f"status={self.status.value})"
        )
