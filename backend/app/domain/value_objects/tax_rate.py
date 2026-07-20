"""
Value Object: TaxRate (Ставка ПДВ).

Безпечний тип для роботи з податковими ставками.
Підтримує стандартні ставки ПДВ: 0%, 7%, 20%.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from typing import Union


@dataclass(frozen=True)
class TaxRate:
    """
    Ставка податку (ПДВ).

    Валідація:
    - Ставка має бути одним з дозволених значень: 0, 7, 20 (відсотки)
    - Значення зберігається як Decimal (0.0, 0.07, 0.20)

    Підтримує розрахунок суми ПДВ та ціни з ПДВ.
    """

    value: Decimal

    # Дозволені ставки ПДВ в Україні (у відсотках)
    ALLOWED_RATES_PERCENT = {0, 7, 20}

    def __post_init__(self) -> None:
        if not isinstance(self.value, Decimal):
            object.__setattr__(self, "value", Decimal(str(self.value)))
        # Перевіряємо, чи це відсоток (0-100) або десятковий дріб (0.0-1.0)
        percent = self._to_percent()
        if percent not in self.ALLOWED_RATES_PERCENT:
            raise ValueError(
                f"Invalid tax rate: {self.value}. "
                f"Allowed rates: {self.ALLOWED_RATES_PERCENT}% "
                f"(got {percent}%)"
            )

    def _to_percent(self) -> int:
        """Конвертує в відсотки."""
        val = float(self.value)
        if val <= 1.0:
            return int(val * 100)
        return int(val)

    @property
    def percent(self) -> int:
        """Повертає ставку у відсотках."""
        return self._to_percent()

    @property
    def rate(self) -> Decimal:
        """Повертає ставку як десятковий дріб (для розрахунків)."""
        return Decimal(str(self.percent / 100))

    def apply_to(self, amount: Decimal) -> Decimal:
        """
        Розраховує суму ПДВ для заданої суми.

        Args:
            amount: Сума без ПДВ.

        Returns:
            Сума ПДВ.
        """
        return (amount * self.rate).quantize(Decimal("0.01"))

    def calculate_gross(self, net_amount: Decimal) -> Decimal:
        """
        Розраховує суму з ПДВ.

        Args:
            net_amount: Сума без ПДВ.

        Returns:
            Сума з ПДВ.
        """
        return (net_amount * (Decimal("1") + self.rate)).quantize(Decimal("0.01"))

    def extract_net(self, gross_amount: Decimal) -> Decimal:
        """
        Розраховує суму без ПДВ із суми з ПДВ.

        Args:
            gross_amount: Сума з ПДВ.

        Returns:
            Сума без ПДВ.
        """
        return (gross_amount / (Decimal("1") + self.rate)).quantize(Decimal("0.01"))

    def __str__(self) -> str:
        return f"{self.percent}%"

    def __repr__(self) -> str:
        return f"TaxRate(value={self.percent}%)"

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, TaxRate):
            return NotImplemented
        return self.percent == other.percent

    @classmethod
    def zero(cls) -> TaxRate:
        """Створює ставку 0%."""
        return cls(Decimal("0"))

    @classmethod
    def reduced(cls) -> TaxRate:
        """Створює ставку 7%."""
        return cls(Decimal("7"))

    @classmethod
    def standard(cls) -> TaxRate:
        """Створює ставку 20%."""
        return cls(Decimal("20"))

    @classmethod
    def from_percent(cls, percent: int) -> TaxRate:
        """Створює ставку з відсотків."""
        return cls(Decimal(str(percent)))
