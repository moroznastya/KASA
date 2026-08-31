"""
Domain Entity: Supplier (Постачальник).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime
from uuid import UUID, uuid4

from ..value_objects.money import Money


@dataclass
class Supplier:
    """
    Постачальник товарів.

    Відповідає за:
    - Ідентифікацію постачальника
    - Контактну інформацію
    - Фінансовий баланс (взаєморозрахунки)
    """

    id: UUID = field(default_factory=uuid4)
    name: str = ""
    balance: Money = field(default_factory=lambda: Money.zero())
    contact_person: str = ""
    phone: str = ""
    email: str = ""
    address: str = ""
    edrpou: str = ""
    is_active: bool = True
    created_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    notes: str = ""

    def update_balance(self, amount: Money) -> None:
        """
        Оновлює баланс постачальника.

        Args:
            amount: Сума для додавання до балансу (додатна — збільшення боргу).

        Raises:
            ValueError: Якщо валюти не співпадають.
        """
        if self.balance.currency != amount.currency:
            raise ValueError(
                f"Currency mismatch: {self.balance.currency} vs {amount.currency}"
            )
        self.balance = self.balance + amount

    def reduce_balance(self, amount: Money) -> None:
        """
        Зменшує баланс постачальника (оплата).

        Args:
            amount: Сума оплати.

        Raises:
            ValueError: Якщо сума оплати перевищує баланс.
        """
        if amount > self.balance:
            raise ValueError(
                f"Payment amount {amount} exceeds balance {self.balance}"
            )
        self.balance = self.balance - amount

    def deactivate(self) -> None:
        """Деактивує постачальника."""
        self.is_active = False

    def activate(self) -> None:
        """Активує постачальника."""
        self.is_active = True

    @property
    def has_debt(self) -> bool:
        """Чи має постачальник борг (додатній баланс)."""
        return self.balance.is_positive()

    @property
    def balance_is_zero(self) -> bool:
        """Чи нульовий баланс."""
        return self.balance.is_zero()

    def __str__(self) -> str:
        return f"Supplier(id={self.id}, name='{self.name}', balance={self.balance})"

    def __repr__(self) -> str:
        return (
            f"Supplier(id={self.id}, name='{self.name}', "
            f"balance={self.balance}, active={self.is_active})"
        )
