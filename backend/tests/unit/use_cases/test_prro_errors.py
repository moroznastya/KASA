"""Unit tests: помилки ПРРО — код + точний текст у фінальному повідомленні.

Вимога UX: жодна помилка ПРРО не доходить до користувача як загальне
повідомлення без коду/точного тексту. Формат: «[КОД] Точний текст».
"""

from __future__ import annotations

from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    PrroFiscalizeError,
    server_error_text,
    status_name,
)
from app.application.use_cases.prro.prro_settings_use_case import (
    PrroSettingsError,
)
from app.application.use_cases.prro.shift_use_case import PrroShiftError


class TestErrorStrFormat:
    """__str__ кожного типу помилки = «[КОД] Точний текст»."""

    def test_fiscalize_error_with_code(self):
        e = PrroFiscalizeError("Невірний хеш попереднього чеку", "ERROR_BAD_HASH_PREV")
        assert str(e) == "[ERROR_BAD_HASH_PREV] Невірний хеш попереднього чеку"

    def test_fiscalize_error_default_code(self):
        e = PrroFiscalizeError("Щось пішло не так")
        assert str(e).startswith("[PRRO_FISCALIZE_ERROR] ")

    def test_fiscalize_error_grpc_code(self):
        e = PrroFiscalizeError("gRPC send_chk не вдався: timeout", "GRPC_ERROR")
        assert str(e).startswith("[GRPC_ERROR] gRPC send_chk не вдався:")

    def test_fiscalize_error_numeric_dps_code(self):
        # код ДПС може бути числовим (-12)
        e = PrroFiscalizeError("ERROR_BAD_HASH_PREV", "-12")
        assert str(e) == "[-12] ERROR_BAD_HASH_PREV"

    def test_shift_error_includes_code(self):
        e = PrroShiftError("Зміну вже закрито")
        assert str(e) == "[PRRO_SHIFT_ERROR] Зміну вже закрито"

    def test_settings_error_includes_code(self):
        e = PrroSettingsError("налаштуйте ПРРО")
        assert str(e) == "[PRRO_SETTINGS_ERROR] налаштуйте ПРРО"

    def test_error_keeps_message_attribute(self):
        e = PrroFiscalizeError("текст", "CODE")
        assert e.message == "текст"
        assert e.code == "CODE"


class TestStatusName:
    """Мапа статусів ДПС → назва (parity з Rust status_name)."""

    def test_known_statuses(self):
        assert status_name(1) == "OK"
        assert status_name(-3) == "ERROR_SAVE"
        assert status_name(-4) == "ERROR_UNKNOWN"
        assert status_name(-12) == "ERROR_BAD_HASH_PREV"
        assert status_name(-16) == "ERROR_OFFLINE_ID"

    def test_unknown_status(self):
        assert status_name(999) == "STATUS_999"


class TestServerErrorText:
    """Фінальний текст: код завжди присутній."""

    def test_server_error_with_message(self):
        assert server_error_text(-4, "Unknown error") == "[ERROR_UNKNOWN] Unknown error"

    def test_server_error_save(self):
        assert server_error_text(-3, "Server rejected") == "[ERROR_SAVE] Server rejected"

    def test_server_error_empty_message_uses_human_text(self):
        t = server_error_text(-12, "")
        assert t.startswith("[ERROR_BAD_HASH_PREV] ")
        assert "ERROR_BAD_HASH_PREV" in t

    def test_server_error_empty_message_unknown_status(self):
        t = server_error_text(999, "")
        assert t == "[STATUS_999] Невідомий статус фіскального сервера."

    def test_server_error_whitespace_message(self):
        t = server_error_text(-15, "   ")
        assert t.startswith("[ERROR_NOT_OPEN_SHIFT] ")
