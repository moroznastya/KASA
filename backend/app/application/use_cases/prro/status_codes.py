"""Спільна таблиця кодів статусів фіскального сервера ДПС (-1..-16).

Єдине джерело істини для Python — 1:1 Rust
`frontend/src-tauri/crates/torgashka-prro/src/prro/status_codes.rs`.

Кожен код → (символьне ім'я, український опис, зрозумілий користувачу).
Джерело кодів: docs/scr/_site_text.txt (рядки 586-601, 646-661, 675-679),
docs/scr/site.html:1041 («-13 ERROR_NOT_REGISTERED_RRO не зареєстровано ПРРО»).
"""

from __future__ import annotations

from typing import Optional

# code → (ім'я, опис українською)
DPS_STATUS_CODES: dict[int, tuple[str, str]] = {
    1: ("OK", "Успішно"),
    -1: ("ERROR_VEREFY", "Помилка перевірки даних"),
    -2: ("ERROR_CHECK", "Помилка перевірки чека"),
    -3: ("ERROR_SAVE", "Помилка збереження даних на сервері ДПС"),
    -4: ("ERROR_UNKNOWN", "Невідома помилка фіскального сервера"),
    -5: ("ERROR_TYPE", "Неправильний тип чека"),
    -6: ("ERROR_NOT_PREV_ZREPORT", "Не знайдено попередній Z-звіт"),
    -7: ("ERROR_XML", "Помилка формування XML"),
    -8: ("ERROR_XML_DATE", "Помилка дати у XML"),
    -9: ("ERROR_XML_CHK", "Помилка чека у XML"),
    -10: ("ERROR_XML_ZREPORT", "Помилка Z-звіту у XML"),
    -11: ("ERROR_OFFLINE_168", "Пристрій працює офлайн (понад 168 годин)"),
    -12: ("ERROR_BAD_HASH_PREV", "Невірний хеш попереднього чека"),
    -13: ("ERROR_NOT_REGISTERED_RRO", "ПРРО не зареєстровано"),
    -14: ("ERROR_NOT_REGISTERED_SIGNER", "Підписувача не зареєстровано"),
    -15: ("ERROR_NOT_OPEN_SHIFT", "Зміну не відкрито"),
    -16: ("ERROR_OFFLINE_ID", "Пристрій офлайн (не отримано ідентифікатор)"),
}


def status_name(status: int) -> str:
    """Ім'я коду статусу: ERROR_SAVE / OK; для невідомих — STATUS_{n}."""
    entry = DPS_STATUS_CODES.get(status)
    if entry is not None:
        return entry[0]
    return f"STATUS_{abs(status)}" if status < 0 else f"STATUS_{status}"


def status_description_uk(status: int) -> Optional[str]:
    """Опис статусу українською або None для невідомого коду."""
    entry = DPS_STATUS_CODES.get(status)
    return entry[1] if entry is not None else None


def status_error_text(status: int) -> str:
    """Повний текст статусу для користувача:
    `status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)`."""
    entry = DPS_STATUS_CODES.get(status)
    if entry is not None:
        return f"status={status} ({entry[0]}: {entry[1]})"
    return f"status={status} (STATUS_{status}: невідомий статус)"
