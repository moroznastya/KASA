"""
Value Object: Money (Гроші).

Безпечний тип для роботи з грошовими сумами.
Використовує Decimal для уникнення помилок округлення.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal, ROUND_HALF_UP
from typing import Union


@dataclass(frozen=True)
class Money:
    """
    Грошова сума з валютою.

    Валідація:
    - Сума не може бути від'ємною
    - Максимум 2 знаки після коми
    - Валюта — рядок (за замовчуванням "UAH")

    Підтримує арифметичні операції: +, -, *, /, порівняння.
    """

    amount: Decimal
    currency: str = "UAH"

    def __post_init__(self) -> None:
        if not isinstance(self.amount, Decimal):
            object.__setattr__(self, "amount", Decimal(str(self.amount)))
        if self.amount < 0:
            raise ValueError(f"Money amount cannot be negative: {self.amount}")
        if self.amount.as_tuple().exponent < -2:
            raise ValueError(
                f"Money amount cannot have more than 2 decimal places: {self.amount}"
            )

    def __add__(self, other: Money) -> Money:
        if self.currency != other.currency:
            raise ValueError(
                f"Cannot add different currencies: {self.currency} vs {other.currency}"
            )
        return Money(self.amount + other.amount, self.currency)

    def __sub__(self, other: Money) -> Money:
        if self.currency != other.currency:
            raise ValueError(
                f"Cannot subtract different currencies: {self.currency} vs {other.currency}"
            )
        result = self.amount - other.amount
        if result < 0:
            raise ValueError("Resulting money amount cannot be negative")
        return Money(result, self.currency)

    def __mul__(self, factor: Union[int, float, Decimal]) -> Money:
        factor = Decimal(str(factor)) if not isinstance(factor, Decimal) else factor
        result = (self.amount * factor).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
        return Money(result, self.currency)

    def __truediv__(self, divisor: Union[int, float, Decimal]) -> Money:
        divisor = Decimal(str(divisor)) if not isinstance(divisor, Decimal) else divisor
        if divisor == 0:
            raise ZeroDivisionError("Cannot divide Money by zero")
        result = (self.amount / divisor).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
        return Money(result, self.currency)

    def __neg__(self) -> Money:
        raise ValueError("Cannot negate Money amount")

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Money):
            return NotImplemented
        return self.amount == other.amount and self.currency == other.currency

    def __lt__(self, other: Money) -> bool:
        if self.currency != other.currency:
            raise ValueError("Cannot compare different currencies")
        return self.amount < other.amount

    def __le__(self, other: Money) -> bool:
        if self.currency != other.currency:
            raise ValueError("Cannot compare different currencies")
        return self.amount <= other.amount

    def __gt__(self, other: Money) -> bool:
        if self.currency != other.currency:
            raise ValueError("Cannot compare different currencies")
        return self.amount > other.amount

    def __ge__(self, other: Money) -> bool:
        if self.currency != other.currency:
            raise ValueError("Cannot compare different currencies")
        return self.amount >= other.amount

    def __str__(self) -> str:
        return f"{self.amount:.2f} {self.currency}"

    def __repr__(self) -> str:
        return f"Money(amount={self.amount:.2f}, currency='{self.currency}')"

    def is_positive(self) -> bool:
        """Чи є сума додатною (> 0)."""
        return self.amount > 0

    def is_zero(self) -> bool:
        """Чи дорівнює сума нулю."""
        return self.amount == 0

    def with_currency(self, currency: str) -> Money:
        """Повертає нову суму з іншою валютою."""
        return Money(self.amount, currency)

    @classmethod
    def zero(cls, currency: str = "UAH") -> Money:
        """Створює нульову суму."""
        return cls(Decimal("0.00"), currency)

    @classmethod
    def from_float(cls, amount: float, currency: str = "UAH") -> Money:
        """Створює Money з float (з округленням до 2 знаків)."""
        return cls(Decimal(str(amount)).quantize(Decimal("0.01")), currency)
