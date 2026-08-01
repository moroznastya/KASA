"""
Application Layer: SettingsValueValidator — валідація значень налаштувань.

Захищає від збереження некоректних значень через PUT /settings/{key}:
  - price_tag_width=-500
  - print_copies=999999
  - barcode_type="<script>..."

Правила валідації за ключем:
  - price_tag_width, price_tag_height, label_width, label_height → int, 10..200
  - price_tag_gap, label_gap                                 → int, 0..20
  - price_tag_margin                                         → int, 0..50
  - print_copies                                             → int, 1..100
  - barcode_type                                             → whitelist: ["code128", "qr"]
  - auto_cut_paper, show_logo                                → bool ("true"/"false"/"1"/"0")
  - Інші ключі                                               → без обмежень

Значення нормалізується перед зберіганням (value зберігається як Text):
  - int  → str(int_value)
  - bool → "true" / "false"
"""

from __future__ import annotations

# ─── Таблиці валідації ───────────────────────────────────────────────────────

# Цілочисельні ключі: key → (мінімум, максимум)
_INT_RANGE_RULES: dict[str, tuple[int, int]] = {
    "price_tag_width": (10, 200),
    "price_tag_height": (10, 200),
    "label_width": (10, 200),
    "label_height": (10, 200),
    "price_tag_gap": (0, 20),
    "label_gap": (0, 20),
    "price_tag_margin": (0, 50),
    "print_copies": (1, 100),
}

# Булеві ключі (приймають "true"/"false"/"1"/"0", нормалізуються до "true"/"false")
_BOOL_KEYS: frozenset[str] = frozenset({"auto_cut_paper", "show_logo"})

# Whitelist значень: key → допустимі значення
_WHITELIST_RULES: dict[str, frozenset[str]] = {
    "barcode_type": frozenset({"code128", "qr"}),
}


# ─── Допоміжні функції валідації ─────────────────────────────────────────────

def _validate_int(key: str, value: str, min_value: int, max_value: int) -> str:
    """
    Валідує цілочисельне значення та нормалізує його до рядка.

    Args:
        key: ключ налаштування (для повідомлення).
        value: значення як рядок.
        min_value: мінімально допустиме значення.
        max_value: максимально допустиме значення.

    Returns:
        Нормалізований рядок (str(int_value)).

    Raises:
        ValueError: якщо значення не є цілим числом або виходить за діапазон.
    """
    try:
        int_value = int(value)
    except (ValueError, TypeError):
        raise ValueError(
            f"Налаштування '{key}' має бути цілим числом від {min_value} до {max_value}."
        )

    if not (min_value <= int_value <= max_value):
        raise ValueError(
            f"Налаштування '{key}' має бути в діапазоні від {min_value} до {max_value}."
        )

    # Нормалізуємо: int → str (value зберігається як Text)
    return str(int_value)


def _validate_bool(key: str, value: str) -> str:
    """
    Валідує булеве значення та нормалізує його до "true"/"false".

    Args:
        key: ключ налаштування (для повідомлення).
        value: значення як рядок.

    Returns:
        "true" або "false".

    Raises:
        ValueError: якщо значення не є допустимим булевим ("true"/"false"/"1"/"0").
    """
    normalized = value.lower()
    if normalized in ("true", "1"):
        return "true"
    if normalized in ("false", "0"):
        return "false"
    raise ValueError(
        f"Налаштування '{key}' має бути булевим значенням: true, false, 1 або 0."
    )


def _validate_whitelist(key: str, value: str, allowed: frozenset[str]) -> str:
    """
    Валідує значення за whitelist-списком.

    Args:
        key: ключ налаштування (для повідомлення).
        value: значення як рядок.
        allowed: множина допустимих значень.

    Returns:
        Оригінальне значення (без змін).

    Raises:
        ValueError: якщо значення не входить у whitelist.
    """
    if value not in allowed:
        allowed_str = ", ".join(f"'{item}'" for item in sorted(allowed))
        raise ValueError(
            f"Налаштування '{key}' має бути одним із значень: {allowed_str}."
        )
    return value


# ─── Головна функція валідації ───────────────────────────────────────────────

def validate_and_normalize_setting_value(key: str, value: str | None) -> str | None:
    """
    Валідує та нормалізує значення налаштування за ключем.

    Args:
        key: ключ налаштування.
        value: значення налаштування (як рядок з запиту).

    Returns:
        Нормалізоване значення для зберігання (str) або None (якщо value=None).

    Raises:
        ValueError: якщо значення не проходить валідацію.
            Повідомлення українською, готове для HTTP 422 detail.
    """
    # None дозволено — зберігається як NULL (без валідації)
    if value is None:
        return None

    value_stripped = value.strip()

    # ── Цілочисельні налаштування ─────────────────────────────────────────
    if key in _INT_RANGE_RULES:
        min_value, max_value = _INT_RANGE_RULES[key]
        return _validate_int(key, value_stripped, min_value, max_value)

    # ── Булеві налаштування ───────────────────────────────────────────────
    if key in _BOOL_KEYS:
        return _validate_bool(key, value_stripped)

    # ── Whitelist значень ─────────────────────────────────────────────────
    if key in _WHITELIST_RULES:
        return _validate_whitelist(key, value_stripped, _WHITELIST_RULES[key])

    # ── Інші ключі — без обмежень ─────────────────────────────────────────
    return value
