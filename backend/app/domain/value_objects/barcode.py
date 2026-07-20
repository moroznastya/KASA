"""
Value Object: Barcode (Штрих-код).

Підтримує валідацію EAN-13 формату.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Optional


class BarcodeType(Enum):
    """Тип штрих-коду."""
    EAN13 = "ean13"
    EAN8 = "ean8"
    CODE128 = "code128"
    CODE39 = "code39"
    UPC_A = "upc_a"
    UNKNOWN = "unknown"


@dataclass(frozen=True)
class Barcode:
    """
    Штрих-код товару.

    Валідація:
    - EAN-13: рівно 13 цифр
    - EAN-8: рівно 8 цифр
    - Інші типи: перевіряються за довжиною

    Автоматично визначає тип штрих-коду.
    """

    value: str

    def __post_init__(self) -> None:
        if not self.value or not self.value.strip():
            raise ValueError("Barcode cannot be empty")
        # Очищуємо від пробілів
        object.__setattr__(self, "value", self.value.strip())
        # Валідація за типом
        if not self._validate():
            raise ValueError(
                f"Invalid barcode value: '{self.value}'. "
                f"Detected type: {self.type.value}. "
                f"Expected format: {self._expected_format()}"
            )

    @property
    def type(self) -> BarcodeType:
        """Визначає тип штрих-коду на основі довжини."""
        digits = self._digits_only()
        if len(digits) == 13 and digits.isdigit():
            return BarcodeType.EAN13
        elif len(digits) == 8 and digits.isdigit():
            return BarcodeType.EAN8
        elif len(digits) == 12 and digits.isdigit():
            return BarcodeType.UPC_A
        elif self.value.isascii() and len(self.value) <= 50:
            return BarcodeType.CODE128
        else:
            return BarcodeType.UNKNOWN

    def _digits_only(self) -> str:
        return "".join(ch for ch in self.value if ch.isdigit())

    def _validate(self) -> bool:
        """Перевіряє валідність штрих-коду."""
        btype = self.type
        if btype == BarcodeType.EAN13:
            return len(self._digits_only()) == 13 and self._check_ean13_checksum()
        elif btype == BarcodeType.EAN8:
            return len(self._digits_only()) == 8
        elif btype == BarcodeType.UPC_A:
            return len(self._digits_only()) == 12
        elif btype == BarcodeType.CODE128:
            return 1 <= len(self.value) <= 50
        return False

    def _check_ean13_checksum(self) -> bool:
        """Перевіряє контрольну суму EAN-13."""
        digits = self._digits_only()
        if len(digits) != 13:
            return False
        total = 0
        for i, digit in enumerate(digits[:12]):
            num = int(digit)
            total += num * (3 if i % 2 == 1 else 1)
        checksum = (10 - (total % 10)) % 10
        return checksum == int(digits[12])

    def _expected_format(self) -> str:
        """Повертає очікуваний формат для поточного типу."""
        formats = {
            BarcodeType.EAN13: "13 digits",
            BarcodeType.EAN8: "8 digits",
            BarcodeType.UPC_A: "12 digits",
            BarcodeType.CODE128: "1-50 ASCII characters",
            BarcodeType.UNKNOWN: "unknown format",
        }
        return formats.get(self.type, "unknown")

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"Barcode(value='{self.value}', type='{self.type.value}')"

    @classmethod
    def from_ean13(cls, value: str) -> Barcode:
        """Створює Barcode з явним EAN-13."""
        return cls(value)

    @classmethod
    def from_code128(cls, value: str) -> Barcode:
        """Створює Barcode з явним Code128."""
        return cls(value)
