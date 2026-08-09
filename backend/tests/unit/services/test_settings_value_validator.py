"""
Юніт-тести валідації значень налаштувань (SettingsValueValidator).

Перевіряють:
  - Числові діапазони за ключами
  - Булеві значення та нормалізацію
  - Whitelist для barcode_type
  - Зворотну сумісність з seed_settings.py (price_tag_gap=3, label_gap=3,
    price_tag_margin=10, barcode_type=code128)
  - Що інші ключі зберігаються без обмежень
"""
from __future__ import annotations

import pytest

from app.application.services.settings_value_validator import (
    validate_and_normalize_setting_value as v,
)


# ─── Зворотна сумісність із seed_settings.py ─────────────────────────────────

class TestSeedCompatibility:
    """Seed значення з seed_settings.py та міграції f89706f0cc14."""

    def test_seed_values_pass(self):
        """Всі seed значення проходять валідацію без змін."""
        assert v("price_tag_gap", "3") == "3"
        assert v("label_gap", "3") == "3"
        assert v("price_tag_margin", "10") == "10"
        assert v("barcode_type", "code128") == "code128"

    def test_seed_numeric_defaults_pass(self):
        """Числові seed-дефолти проходять валідацію."""
        assert v("price_tag_width", "40") == "40"
        assert v("price_tag_height", "25") == "25"
        assert v("label_width", "60") == "60"
        assert v("label_height", "40") == "40"
        assert v("print_copies", "1") == "1"

    def test_seed_bool_defaults_pass(self):
        """Булеві seed-дефолти проходять валідацію."""
        assert v("auto_cut_paper", "true") == "true"
        assert v("show_logo", "true") == "true"


# ─── Числові діапазони ───────────────────────────────────────────────────────

class TestIntRanges:
    """Тести цілочисельних діапазонів."""

    @pytest.mark.parametrize("key", [
        "price_tag_width", "price_tag_height", "label_width", "label_height",
    ])
    @pytest.mark.parametrize("value", ["10", "40", "100", "200"])
    def test_range_10_200_accepts(self, key, value):
        """Діапазон 10..200 приймає граничні та проміжні значення."""
        assert v(key, value) == value

    @pytest.mark.parametrize("key", [
        "price_tag_width", "price_tag_height", "label_width", "label_height",
    ])
    @pytest.mark.parametrize("value", ["9", "201", "-500", "abc", "40.5"])
    def test_range_10_200_rejects(self, key, value):
        """Діапазон 10..200 відхиляє некоректні значення."""
        with pytest.raises(ValueError):
            v(key, value)

    @pytest.mark.parametrize("key", ["price_tag_gap", "label_gap"])
    @pytest.mark.parametrize("value", ["0", "3", "20"])
    def test_range_0_20_accepts(self, key, value):
        """Діапазон 0..20 приймає граничні значення."""
        assert v(key, value) == value

    @pytest.mark.parametrize("key", ["price_tag_gap", "label_gap"])
    @pytest.mark.parametrize("value", ["-1", "21", "abc"])
    def test_range_0_20_rejects(self, key, value):
        """Діапазон 0..20 відхиляє некоректні значення."""
        with pytest.raises(ValueError):
            v(key, value)

    def test_margin_range_0_50(self):
        """Діапазон 0..50 для price_tag_margin."""
        assert v("price_tag_margin", "0") == "0"
        assert v("price_tag_margin", "50") == "50"
        with pytest.raises(ValueError):
            v("price_tag_margin", "51")
        with pytest.raises(ValueError):
            v("price_tag_margin", "-1")

    @pytest.mark.parametrize("value", ["1", "3", "100"])
    def test_print_copies_accepts(self, value):
        """Діапазон 1..100 для print_copies приймає значення."""
        assert v("print_copies", value) == value

    @pytest.mark.parametrize("value", ["0", "101", "999999", "-5", "abc"])
    def test_print_copies_rejects(self, value):
        """Діапазон 1..100 для print_copies відхиляє некоректні значення."""
        with pytest.raises(ValueError):
            v("print_copies", value)

    def test_normalizes_int_to_str(self):
        """Значення нормалізується: int → str для зберігання."""
        assert v("print_copies", "007") == "7"
        assert v("price_tag_width", " 40 ") == "40"


# ─── Булеві значення ─────────────────────────────────────────────────────────

class TestBoolValues:
    """Тести булевих налаштувань."""

    @pytest.mark.parametrize("key", ["auto_cut_paper", "show_logo"])
    @pytest.mark.parametrize("raw,expected", [
        ("true", "true"),
        ("TRUE", "true"),
        ("True", "true"),
        ("1", "true"),
        ("false", "false"),
        ("FALSE", "false"),
        ("0", "false"),
    ])
    def test_accepts_and_normalizes(self, key, raw, expected):
        """Булеві значення приймаються та нормалізуються."""
        assert v(key, raw) == expected

    @pytest.mark.parametrize("key", ["auto_cut_paper", "show_logo"])
    @pytest.mark.parametrize("value", ["yes", "on", "2", "так", "Truee"])
    def test_rejects_invalid(self, key, value):
        """Невідомі булеві значення відхиляються."""
        with pytest.raises(ValueError):
            v(key, value)


# ─── Whitelist barcode_type ──────────────────────────────────────────────────

class TestBarcodeTypeWhitelist:
    """Тести whitelist для barcode_type."""

    @pytest.mark.parametrize("value", ["code128", "qr"])
    def test_accepts_allowed(self, value):
        """Допустимі типи приймаються."""
        assert v("barcode_type", value) == value

    @pytest.mark.parametrize("value", [
        "ean13", "ean8", "upc_a", "<script>alert(1)</script>", "CODE128", "",
    ])
    def test_rejects_not_allowed(self, value):
        """Недопустимі типи відхиляються."""
        with pytest.raises(ValueError):
            v("barcode_type", value)


# ─── Інші ключі ──────────────────────────────────────────────────────────────

class TestOtherKeys:
    """Інші ключі зберігаються без обмежень."""

    def test_arbitrary_values_pass(self):
        """Довільні ключі не валідуються."""
        assert v("company_name", "<b>Torgashka</b>") == "<b>Torgashka</b>"
        assert v("price_tag_fields", '["name","price","barcode"]') == (
            '["name","price","barcode"]'
        )
        assert v("printer_name", "EPSON TM-T20") == "EPSON TM-T20"
        assert v("unknown_key", "anything -500") == "anything -500"

    def test_none_value_passes(self):
        """None проходить без валідації (зберігається як NULL)."""
        assert v("price_tag_width", None) is None
