"""Unit tests: генерація URL перевірки фіскального чеку (2.6)."""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal

from app.infrastructure.services.prro.qr_url import (
    FISCAL_CHECK_BASE_URL,
    build_fiscal_check_url,
)


class TestBuildFiscalCheckUrl:
    def test_full_url(self):
        """Усі параметри → коректне посилання кабінету ДПС."""
        url = build_fiscal_check_url(
            fiscal_number="FISCAL-100",
            amount=Decimal("370.00"),
            prro_fn="4538765845",
            sent_at=datetime(2026, 8, 1, 11, 26, 0),
            mac="MAC-VALUE-123",
        )
        assert url is not None
        assert url.startswith(FISCAL_CHECK_BASE_URL)
        assert "mac=MAC-VALUE-123" in url
        assert "date=20260801" in url
        assert "time=1126" in url
        assert "id=FISCAL-100" in url
        assert "sm=370.00" in url
        assert "fn=4538765845" in url

    def test_amount_float(self):
        """Сума float форматується з двома знаками."""
        url = build_fiscal_check_url(
            fiscal_number="1", amount=123.5, prro_fn="1",
            sent_at=datetime(2026, 1, 2, 3, 4),
        )
        assert url is not None
        assert "sm=123.50" in url

    def test_mac_fallback_hash(self):
        """Без MAC — використовується хеш фіскального номера (SHA-1 hex)."""
        url = build_fiscal_check_url(
            fiscal_number="FISCAL-X", amount=Decimal("10"),
            prro_fn="4538765845", sent_at=datetime(2026, 1, 1),
        )
        assert url is not None
        mac_param = url.split("mac=")[1].split("&")[0]
        assert len(mac_param) == 40  # sha1 hex

    def test_missing_data_returns_none(self):
        """Недостатньо даних → None."""
        assert build_fiscal_check_url(
            fiscal_number="", amount=Decimal("1"), prro_fn="1",
            sent_at=datetime(2026, 1, 1),
        ) is None
        assert build_fiscal_check_url(
            fiscal_number="1", amount=Decimal("1"), prro_fn="",
            sent_at=datetime(2026, 1, 1),
        ) is None
        assert build_fiscal_check_url(
            fiscal_number="1", amount=Decimal("1"), prro_fn="1",
            sent_at=None,
        ) is None
