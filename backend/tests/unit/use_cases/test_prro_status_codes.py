"""Unit tests: спільна таблиця кодів статусів ДПС (status_codes).

1:1 Rust `torgashka-prro/src/prro/status_codes.rs`.
Ключова вимога: status=-13 НІКОЛИ не доходить до користувача без
імені (ERROR_NOT_REGISTERED_RRO) і людського опису («ПРРО не зареєстровано»).
"""

from __future__ import annotations

from app.application.use_cases.prro.status_codes import (
    DPS_STATUS_CODES,
    status_description_uk,
    status_error_text,
    status_name,
)


class TestStatusName:
    def test_known_codes(self):
        assert status_name(1) == "OK"
        assert status_name(-3) == "ERROR_SAVE"
        assert status_name(-12) == "ERROR_BAD_HASH_PREV"
        assert status_name(-13) == "ERROR_NOT_REGISTERED_RRO"
        assert status_name(-15) == "ERROR_NOT_OPEN_SHIFT"
        assert status_name(-16) == "ERROR_OFFLINE_ID"

    def test_unknown_code(self):
        assert status_name(999) == "STATUS_999"
        assert status_name(-999) == "STATUS_999"


class TestDescription:
    def test_known_codes(self):
        assert status_description_uk(-13) == "ПРРО не зареєстровано"
        assert status_description_uk(-15) == "Зміну не відкрито"
        assert status_description_uk(-12) == "Невірний хеш попереднього чека"
        assert status_description_uk(1) == "Успішно"

    def test_unknown_code(self):
        assert status_description_uk(999) is None


class TestStatusErrorText:
    """Головний сценарій задачі: status=-13 → код + ім'я + опис."""

    def test_negative_thirteen_includes_name_and_description(self):
        text = status_error_text(-13)
        assert text == "status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)"
        assert "ERROR_NOT_REGISTERED_RRO" in text
        assert "ПРРО не зареєстровано" in text

    def test_unknown_status(self):
        text = status_error_text(999)
        assert text.startswith("status=999 (STATUS_999:")
        assert "невідомий статус" in text

    def test_table_covers_full_dps_range(self):
        # docs/scr/_site_text.txt: -1..-16 + OK=1
        for code in range(-16, 0):
            assert code in DPS_STATUS_CODES, f"код ДПС {code} відсутній"
        assert 1 in DPS_STATUS_CODES
