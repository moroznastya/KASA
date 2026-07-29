"""
Domain Value Object: Rounding — логіка заокруглення цін/сум.

Підтримувані варіанти:
- 1   — до 1 коп. (без заокруглення)
- 10  — до 10 коп.
- 50  — до 50 коп.
- 100 — до 1 грн.
- 500 — до 5 грн.
"""

from __future__ import annotations

from decimal import Decimal, ROUND_HALF_UP


def round_amount(amount: Decimal, rounding_code: int = 1) -> Decimal:
    """
    Заокруглює суму відповідно до коду заокруглення.

    Args:
        amount: Сума для заокруглення.
        rounding_code: Код заокруглення (1, 10, 50, 100, 500).

    Returns:
        Заокруглена сума.

    Приклади:
        round_amount(Decimal("47.33"), 1)    -> Decimal("47.33")  — без змін
        round_amount(Decimal("47.33"), 10)   -> Decimal("47.30")  — до 10 коп
        round_amount(Decimal("47.37"), 10)   -> Decimal("47.40")  — до 10 коп
        round_amount(Decimal("47.33"), 50)   -> Decimal("47.50")  — до 50 коп
        round_amount(Decimal("47.73"), 50)   -> Decimal("48.00")  — до 50 коп
        round_amount(Decimal("47.33"), 100)  -> Decimal("47")     — до 1 грн
        round_amount(Decimal("47.73"), 100)  -> Decimal("48")     — до 1 грн
        round_amount(Decimal("47.33"), 500)  -> Decimal("45")     — до 5 грн
        round_amount(Decimal("48.33"), 500)  -> Decimal("50")     — до 5 грн
    """
    if rounding_code == 1:
        # Без заокруглення
        return amount.quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)

    if rounding_code == 10:
        # До 10 коп — округлюємо до 1 знаку після коми
        return amount.quantize(Decimal("0.1"), rounding=ROUND_HALF_UP)

    if rounding_code == 50:
        # До 50 коп — множимо на 2, округлюємо до цілого, ділимо на 2
        doubled = amount * 2
        rounded_doubled = doubled.quantize(Decimal("1"), rounding=ROUND_HALF_UP)
        return (rounded_doubled / 2).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)

    if rounding_code == 100:
        # До 1 грн — округлюємо до цілого
        return amount.quantize(Decimal("1"), rounding=ROUND_HALF_UP)

    if rounding_code == 500:
        # До 5 грн — ділимо на 5, округлюємо, множимо на 5
        divided = amount / 5
        rounded_divided = divided.quantize(Decimal("1"), rounding=ROUND_HALF_UP)
        return (rounded_divided * 5).quantize(Decimal("1"), rounding=ROUND_HALF_UP)

    # Невідомий код — без змін
    return amount.quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
