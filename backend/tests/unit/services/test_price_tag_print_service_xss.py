"""
Юніт-тести захисту від Stored XSS у сервісі друку цінників/етикеток.

Перевіряють, що всі дані товару та значення налаштувань екрануються
перед підстановкою в HTML (html.escape з quote=True).
"""
from __future__ import annotations

from typing import ClassVar

from app.infrastructure.services.price_tag_print_service import (
    PriceTagPrintService,
    _escape_html,
    _generate_barcode_svg,
    _generate_qr_svg,
)

# ─── Тестові payload для XSS ─────────────────────────────────────────────────

XSS_PAYLOAD = '<img src=x onerror=alert(document.cookie)>'
XSS_QUOTES = '" onmouseover="alert(1)"'
XSS_SCRIPT = '<script>alert(1)</script>'


# ─── _escape_html ────────────────────────────────────────────────────────────

class TestEscapeHtml:
    """Тести допоміжної функції _escape_html."""

    def test_escapes_tags(self):
        """HTML-теги екрануються."""
        assert _escape_html(XSS_PAYLOAD) == (
            "&lt;img src=x onerror=alert(document.cookie)&gt;"
        )

    def test_escapes_quotes(self):
        """Лапки екрануються (quote=True)."""
        assert '"' not in _escape_html(XSS_QUOTES)
        assert "&quot;" in _escape_html(XSS_QUOTES)

    def test_escapes_script(self):
        """Тег <script> екранується."""
        assert _escape_html(XSS_SCRIPT) == "&lt;script&gt;alert(1)&lt;/script&gt;"

    def test_plain_text_unchanged(self):
        """Звичайний текст не змінюється."""
        assert _escape_html("Хліб білий 25.00") == "Хліб білий 25.00"
        assert _escape_html("4820012345678") == "4820012345678"

    def test_non_string_value(self):
        """Числові значення коректно перетворюються на рядок."""
        assert _escape_html(123) == "123"
        assert _escape_html(None) == "None"


# ─── _render_single ──────────────────────────────────────────────────────────

class TestRenderSingleXss:
    """Тести екранування в _render_single."""

    TEMPLATE = (
        "<div>{{title}}</div>"
        "<div>{{name}}</div>"
        "<div>{{price}}</div>"
        "<div>{{barcode}}</div>"
        "<div>{{article}}</div>"
        "<div>{{category}}</div>"
        "<div>{{created_date}}</div>"
    )

    def test_product_fields_are_escaped(self):
        """Всі поля товару екрануються перед підстановкою."""
        product = {
            "title": XSS_PAYLOAD,
            "price": XSS_PAYLOAD,
            "barcode": XSS_PAYLOAD,
            "article": XSS_PAYLOAD,
            "category": XSS_PAYLOAD,
            "created_date": XSS_PAYLOAD,
        }
        result = PriceTagPrintService._render_single(self.TEMPLATE, product)

        assert "<img src=x" not in result
        assert result.count("&lt;img src=x") >= 6

    def test_name_alias_uses_title_and_escaped(self):
        """Аліас {{name}} бере title та екранується."""
        product = {"title": XSS_PAYLOAD}
        result = PriceTagPrintService._render_single(
            "<div>{{name}}</div>", product
        )
        assert "<img src=x" not in result
        assert "&lt;img src=x" in result

    def test_extra_context_values_are_escaped(self):
        """Значення extra_context (barcode_type, width, height) екрануються."""
        product = {"title": "Тест"}
        extra = {
            "barcode_type": XSS_SCRIPT,
            "width": XSS_SCRIPT,
            "height": XSS_SCRIPT,
        }
        result = PriceTagPrintService._render_single(
            "<div>{{barcode_type}}</div><div>{{width}}</div><div>{{height}}</div>",
            product,
            extra_context=extra,
        )
        assert "<script>" not in result
        assert "&lt;script&gt;" in result


# ─── _generate_barcode_svg / _generate_qr_svg ────────────────────────────────

class TestBarcodeSvgXss:
    """Тести екранування тексту в SVG fallback-гілках."""

    def test_barcode_fallback_escaped(self):
        """Текст штрих-коду в fallback-гілках екранується."""
        result = _generate_barcode_svg(XSS_PAYLOAD, barcode_type="code128")
        assert "<img src=x" not in result
        assert "&lt;img src=x" in result

    def test_qr_fallback_escaped(self):
        """Дані QR-коду в fallback-гілках екрануються."""
        result = _generate_qr_svg(XSS_PAYLOAD)
        assert "<img src=x" not in result
        assert "&lt;img src=x" in result

    def test_barcode_display_text_escaped(self):
        """Підпис під штрих-кодом екрановано."""
        result = _generate_barcode_svg("4820012345678", barcode_type="code128")
        assert "4820012345678" in result

    def test_empty_barcode_returns_empty(self):
        """Порожній штрих-код повертає порожній рядок."""
        assert _generate_barcode_svg("", barcode_type="code128") == ""


# ─── Повний рендер ───────────────────────────────────────────────────────────

class TestFullRenderXss:
    """Тести повного рендеру сітки цінників та етикеток."""

    PRODUCTS: ClassVar[list[dict]] = [{
        "id": "00000000-0000-0000-0000-000000000001",
        "title": XSS_PAYLOAD,
        "price": "25.00",
        "barcode": XSS_PAYLOAD,
        "article": XSS_PAYLOAD,
        "category": XSS_PAYLOAD,
        "copies": 1,
    }]

    def test_price_tags_grid_no_raw_payload(self):
        """Фінальний HTML цінників не містить RAW payload."""
        html = PriceTagPrintService.render_price_tags_grid(
            "<div>{{title}}</div><div>{{barcode}}</div>",
            self.PRODUCTS,
            {
                "width_mm": 40, "height_mm": 25, "gap_mm": 3, "margin_mm": 10,
                "fields": ["title", "barcode"], "barcode_type": "code128",
            },
        )
        assert "<img src=x" not in html
        assert "&lt;img src=x" in html

    def test_labels_sequential_no_raw_payload(self):
        """Фінальний HTML етикеток не містить RAW payload."""
        html = PriceTagPrintService.render_labels_sequential(
            "<div>{{title}}</div><div>{{barcode}}</div>",
            self.PRODUCTS,
            {
                "width_mm": 58, "height_mm": 40, "gap_mm": 2,
                "fields": ["title", "barcode"], "barcode_type": "code128",
            },
        )
        assert "<img src=x" not in html
        assert "&lt;img src=x" in html
