"""
Value Object: Quantity (Кількість).

Безпечний тип для роботи з кількостями товарів.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal, ROUND_HALF_UP
from typing import Union


@dataclass(frozen=True)
class Quantity:
    """
    Кількість товару з одиницею виміру.

    Валідація:
    - Кількість не може бути від'ємною
    - Максимум 3 знаки після коми
    - Одиниця виміру — рядок (за замовчуванням "шт")

    Підтримує арифметичні операції: +, -, *, /, порівняння.
    """

    value: Decimal
    unit: str = "шт"

    def __post_init__(self) -> None:
        if not isinstance(self.value, Decimal):
            object.__setattr__(self, "value", Decimal(str(self.value)))
        if self.value < 0:
            raise ValueError(f"Quantity cannot be negative: {self.value}")
        if self.value.as_tuple().exponent < -3:
            raise ValueError(
                f"Quantity cannot have more than 3 decimal places: {self.value}"
            )

    def __add__(self, other: Quantity) -> Quantity:
        if self.unit != other.unit:
            raise ValueError(
                f"Cannot add different units: {self.unit} vs {other.unit}"
            )
        return Quantity(self.value + other.value, self.unit)

    def __sub__(self, other: Quantity) -> Quantity:
        if self.unit != other.unit:
            raise ValueError(
                f"Cannot subtract different units: {self.unit} vs {other.unit}"
            )
        result = self.value - other.value
        if result < 0:
            raise ValueError("Resulting quantity cannot be negative")
        return Quantity(result, self.unit)

    def __mul__(self, factor: Union[int, float, Decimal]) -> Quantity:
        factor = Decimal(str(factor)) if not isinstance(factor, Decimal) else factor
        result = (self.value * factor).quantize(Decimal("0.001"), rounding=ROUND_HALF_UP)
        return Quantity(result, self.unit)

    def __truediv__(self, divisor: Union[int, float, Decimal]) -> Quantity:
        divisor = Decimal(str(divisor)) if not isinstance(divisor, Decimal) else divisor
        if divisor == 0:
            raise ZeroDivisionError("Cannot divide Quantity by zero")
        result = (self.value / divisor).quantize(Decimal("0.001"), rounding=ROUND_HALF_UP)
        return Quantity(result, self.unit)

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Quantity):
            return NotImplemented
        return self.value == other.value and self.unit == other.unit

    def __lt__(self, other: Quantity) -> bool:
        if self.unit != other.unit:
            raise ValueError("Cannot compare different units")
        return self.value < other.value

    def __le__(self, other: Quantity) -> bool:
        if self.unit != other.unit:
            raise ValueError("Cannot compare different units")
        return self.value <= other.value

    def __gt__(self, other: Quantity) -> bool:
        if self.unit != other.unit:
            raise ValueError("Cannot compare different units")
        return self.value > other.value

    def __ge__(self, other: Quantity) -> bool:
        if self.unit != other.unit:
            raise ValueError("Cannot compare different units")
        return self.value >= other.value

    def __str__(self) -> str:
        return f"{self.value} {self.unit}"

    def __repr__(self) -> str:
        return f"Quantity(value={self.value}, unit='{self.unit}')"

    def is_positive(self) -> bool:
        """Чи є кількість додатною (> 0)."""
        return self.value > 0

    def is_zero(self) -> bool:
        """Чи дорівнює кількість нулю."""
        return self.value == 0

    def with_unit(self, unit: str) -> Quantity:
        """Повертає нову кількість з іншою одиницею виміру."""
        return Quantity(self.value, unit)

    @classmethod
    def zero(cls, unit: str = "шт") -> Quantity:
        """Створює нульову кількість."""
        return cls(Decimal("0"), unit)

    @classmethod
    def from_float(cls, value: float, unit: str = "шт") -> Quantity:
        """Створює Quantity з float (з округленням до 3 знаків)."""
        return cls(Decimal(str(value)).quantize(Decimal("0.001")), unit)
