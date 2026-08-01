"""
Генерація URL перевірки фіскального чеку (QR-код для друку).

Посилання веде на кабінет платника податків ДПС України:

    https://cabinet.tax.gov.ua/cashregs/check?mac=<MAC>&date=YYYYMMDD&time=HHmm\
&id=<fiscal_number>&sm=<сума.грн>&fn=<prro_fn>

Параметри:
  - mac  — MAC/підпис фіскального чеку (з CheckResponse.id_sign / data_sign);
           якщо відсутній — використовується хеш фіскального номера;
  - date — дата фіскалізації у форматі YYYYMMDD;
  - time — час фіскалізації у форматі HHmm;
  - id   — фіскальний номер чеку, присвоєний податковою;
  - sm   — сума чеку в гривнях (з двома знаками після коми);
  - fn   — фіскальний номер ПРРО.

QR-код на основі цього URL генерується на фронтенді (React/Tauri,
бібліотека qrcode) — тут формується лише саме посилання.
"""

from __future__ import annotations

import hashlib
from datetime import datetime
from decimal import Decimal, ROUND_HALF_UP

# Базове посилання кабінету платника податків ДПС
FISCAL_CHECK_BASE_URL = "https://cabinet.tax.gov.ua/cashregs/check"


def _fallback_mac(fiscal_number: str) -> str:
    """
    Формує MAC-замінник з фіскального номера чеку (SHA-1 hex).

    Використовується, якщо у відповіді ПРРО відсутні id_sign / data_sign.

    Args:
        fiscal_number: фіскальний номер чеку.

    Returns:
        str — 40-символьний hex-хеш.
    """
    return hashlib.sha1(str(fiscal_number).encode("utf-8")).hexdigest()


def build_fiscal_check_url(
    *,
    fiscal_number: str,
    amount: Decimal | float | str,
    prro_fn: str,
    sent_at: datetime,
    mac: str | None = None,
) -> str | None:
    """
    Формує URL перевірки фіскального чеку (для QR-коду).

    Args:
        fiscal_number: фіскальний номер чеку (присвоєний податковою).
        amount: сума чеку в гривнях (Decimal/float/str).
        prro_fn: фіскальний номер ПРРО.
        sent_at: дата/час успішної фіскалізації чеку.
        mac: MAC/підпис чеку (id_sign / data_sign з CheckResponse);
            якщо None або порожній — використовується хеш fiscal_number.

    Returns:
        str — готове посилання, або None, якщо недостатньо даних
        (немає фіскального номера, ПРРО або дати фіскалізації).
    """
    if not fiscal_number or not prro_fn or sent_at is None:
        return None

    mac_value = mac or _fallback_mac(fiscal_number)
    sm = Decimal(str(amount)).quantize(
        Decimal("0.01"), rounding=ROUND_HALF_UP
    )

    from urllib.parse import urlencode

    params = {
        "mac": mac_value,
        "date": sent_at.strftime("%Y%m%d"),
        "time": sent_at.strftime("%H%M"),
        "id": str(fiscal_number),
        "sm": f"{sm:.2f}",
        "fn": str(prro_fn),
    }
    return f"{FISCAL_CHECK_BASE_URL}?{urlencode(params)}"


__all__ = ["build_fiscal_check_url", "FISCAL_CHECK_BASE_URL"]
