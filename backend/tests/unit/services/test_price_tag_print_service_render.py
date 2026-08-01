"""
Юніт-тести коректності рендеру HTML цінників/етикеток.

Перевіряють, що повний HTML-документ (шаблон з БД, де <body style="...">
містить рамку border, width/height 100%) НЕ вкладається як документ
у комірки друку, а нормалізується у внутрішній <div>:

  - .tag-cell / .label-item не містять вкладених <html>/<body>;
  - стилі body (рамка border, width/height 100%) переносяться
    у внутрішній div → рамка друкується, контент не обрізається;
  - кастомні фрагменти без <body> працюють як є.

Додатково перевіряються розміри друку:
  - A4 (.page) використовує КОРИСНУ висоту сторінки: 297 − 2×margin = 277мм
    (інакше контент переповнює друковану область → порожня 2-га сторінка);
  - термо-етикетки (label-item) рендеряться з ефективною шириною
    min(width, 48мм) — реальною друкованою областю 58мм принтера
    (384 dots @203dpi) → html2canvas-канвас має пропорції 384×320 dots
    і Rust масштабує рівномірно, без спотворення.
"""
from __future__ import annotations

import pytest

from app.infrastructure.services.price_tag_print_service import (
    HAS_QRCODE,
    PriceTagPrintService,
    _generate_qr_svg,
)

# ─── Повний HTML-документ як шаблон з БД ────────────────────────────────────

FULL_TEMPLATE = (
    '<!DOCTYPE html><html lang="uk"><head><meta charset="UTF-8"></head>'
    '<body style="width: 100%; height: 100%; border: 1px solid #000; '
    'box-sizing: border-box; display: flex; flex-direction: column; '
    'justify-content: center; align-items: center; text-align: center;">'
    '<div class="name">{{title}}</div>'
    '<div class="price">{{price}} грн</div>'
    '<div class="bc">{{barcode_image}}</div>'
    '</body></html>'
)

PRODUCT = {
    "id": "00000000-0000-0000-0000-000000000001",
    "title": "Хліб білий",
    "price": "25.00",
    "barcode": "4820012345678",
    "copies": 1,
}

GRID_SETTINGS = {
    "width_mm": 40, "height_mm": 25, "gap_mm": 3, "margin_mm": 10,
    "fields": ["title", "price", "barcode"],
    "barcode_type": "code128",
}

LABEL_SETTINGS = {
    "width_mm": 58, "height_mm": 40, "gap_mm": 2,
    "fields": ["title", "price", "barcode"],
    "barcode_type": "code128",
}


# ─── _extract_body ───────────────────────────────────────────────────────────

class TestExtractBody:
    """Тести статичного методу _extract_body."""

    def test_full_document_with_style(self):
        """Повний документ: повертає атрибути та вміст body."""
        attrs, content = PriceTagPrintService._extract_body(
            '<html><head></head>'
            '<body style="border: 1px solid #000">Hi</body></html>'
        )
        assert attrs == 'style="border: 1px solid #000"'
        assert content.strip() == "Hi"

    def test_body_without_attrs(self):
        """<body> без атрибутів: повертає ('', вміст)."""
        attrs, content = PriceTagPrintService._extract_body(
            "<html><body>Hello</body></html>"
        )
        assert attrs == ""
        assert content == "Hello"

    def test_unclosed_body(self):
        """<body> без закриваючого тега: повертає атрибути та вміст до кінця."""
        attrs, content = PriceTagPrintService._extract_body(
            '<body style="border:1px solid #000">Hello'
        )
        assert attrs == 'style="border:1px solid #000"'
        assert content == "Hello"

    def test_no_body_cleans_document_wrapper(self):
        """Без <body>: прибирає DOCTYPE/html/head."""
        attrs, content = PriceTagPrintService._extract_body(
            '<!DOCTYPE html><html><head><style>x{}</style></head>'
            '<div>Frag</div></html>'
        )
        assert attrs == ""
        assert "<!DOCTYPE" not in content
        assert "<html" not in content
        assert "<head>" not in content
        assert "Frag" in content

    def test_fragment_without_body_unchanged(self):
        """Фрагмент без body: повертається як є."""
        attrs, content = PriceTagPrintService._extract_body(
            "<div>{{title}}</div>"
        )
        assert attrs == ""
        assert content == "<div>{{title}}</div>"


# ─── Рендер A4-сітки з повним HTML-шаблоном ─────────────────────────────────

class TestGridRender:
    """Тести рендеру цінників на A4 (render_price_tags_grid)."""

    def test_no_nested_html_inside_tag_cell(self):
        """
        Усередині .tag-cell немає вкладеного <html>/<body>.

        Комірка містить <div style="..."> зі стилями body (рамка, 100%).
        """
        html = PriceTagPrintService.render_price_tags_grid(
            FULL_TEMPLATE, [PRODUCT], GRID_SETTINGS
        )
        # Єдиний <html> — зовнішній документ; єдиний <body> — його обгортка
        assert html.count("<html") == 1
        assert html.count("<body") == 1

        # Вміст комірки — внутрішній div зі стилями body, БЕЗ html/body
        cell = html.split('<div class="tag-cell">', 1)[1]
        assert cell.startswith('<div style="')
        assert "<html" not in cell
        assert "<body" not in cell

    def test_border_style_preserved_in_cell(self):
        """Рамка (border 1px solid #000) зі style body перенесена у комірку."""
        html = PriceTagPrintService.render_price_tags_grid(
            FULL_TEMPLATE, [PRODUCT], GRID_SETTINGS
        )
        assert "border: 1px solid #000" in html
        assert "width: 100%" in html
        assert "height: 100%" in html

    def test_product_data_rendered_inside_cell(self):
        """Дані товару підставлені всередину комірки."""
        html = PriceTagPrintService.render_price_tags_grid(
            FULL_TEMPLATE, [PRODUCT], GRID_SETTINGS
        )
        cell = html.split('<div class="tag-cell">', 1)[1]
        assert "Хліб білий" in cell
        assert "25.00 грн" in cell

    def test_no_gray_border_and_padding_in_tag_cell(self):
        """.tag-cell не має сірої рамки та внутрішніх відступів."""
        html = PriceTagPrintService.render_price_tags_grid(
            FULL_TEMPLATE, [PRODUCT], GRID_SETTINGS
        )
        assert "0.1mm solid #999" not in html
        assert "padding: 1.5mm" not in html

    def test_page_min_height_uses_usable_height(self):
        """
        .page використовує КОРИСНУ висоту сторінки: 297 − 2×margin.

        @page margin = 10мм → друкована область = 277мм. Якщо min-height
        залишити 297мм, контент переповнює друковану область → порожня
        друга сторінка або обрізання при друку A4.
        """
        html = PriceTagPrintService.render_price_tags_grid(
            FULL_TEMPLATE, [PRODUCT], GRID_SETTINGS
        )
        page_css = html.split(".page {", 1)[1].split("}", 1)[0]
        assert "min-height: 277.0mm" in page_css
        assert "min-height: 297" not in page_css


# ─── Рендер термо-етикеток з повним HTML-шаблоном ───────────────────────────

class TestLabelRender:
    """Тести рендеру етикеток термопринтера (render_labels_sequential)."""

    def test_no_nested_html_inside_label_item(self):
        """Усередині .label-item немає вкладеного <html>/<body>."""
        html = PriceTagPrintService.render_labels_sequential(
            FULL_TEMPLATE, [PRODUCT], LABEL_SETTINGS
        )
        # Єдиний <html> — зовнішній документ; єдиний <body> — його обгортка
        assert html.count("<html") == 1
        assert html.count("<body") == 1

        # Вміст label-item — внутрішній div зі стилями body, БЕЗ html/body
        marker = 'font-family: Arial, sans-serif;">'
        inner = html.split(marker, 1)[1]
        assert inner.startswith('<div style="')
        assert "<html" not in inner
        assert "<body" not in inner

    def test_border_style_preserved_in_label(self):
        """Рамка (border 1px solid #000) перенесена у .label-item."""
        html = PriceTagPrintService.render_labels_sequential(
            FULL_TEMPLATE, [PRODUCT], LABEL_SETTINGS
        )
        assert "border: 1px solid #000" in html
        assert "Хліб білий" in html

    def test_label_item_has_no_gray_border(self):
        """.label-item більше не має сірої рамки border: 0.1mm solid #ccc."""
        html = PriceTagPrintService.render_labels_sequential(
            FULL_TEMPLATE, [PRODUCT], LABEL_SETTINGS
        )
        assert "0.1mm solid #ccc" not in html

    def test_label_width_capped_at_48mm(self):
        """
        Термо-етикетка 58×40 рендериться з ЕФЕКТИВНОЮ шириною 48мм.

        Друкована область 58мм термопринтера = 48мм (384 dots @203dpi).
        Рендер 48×40мм дає canvas з пропорціями 1.2 (= 384×320 dots),
        Rust масштабує рівномірно → без спотворення штрих-коду/вертикалі.
        """
        html = PriceTagPrintService.render_labels_sequential(
            FULL_TEMPLATE, [PRODUCT], LABEL_SETTINGS
        )
        label_item = html.split('class="label-item" style="', 1)[1].split('"', 1)[0]
        assert "width: 48.0mm;" in label_item
        assert "width: 58.0mm;" not in label_item

        # @page теж 48×40мм: html2canvas знімає СТОРІНКУ цілком
        page_rule = html.split("@page {", 1)[1].split("}", 1)[0]
        assert "size: 48.0mm 40.0mm;" in page_rule

    def test_narrow_label_width_unchanged(self):
        """Етикетка ≤48мм (напр. 40×25) рендериться без змін."""
        html = PriceTagPrintService.render_labels_sequential(
            FULL_TEMPLATE, [PRODUCT],
            {"width_mm": 40, "height_mm": 25, "gap_mm": 2,
             "fields": ["title", "price", "barcode"], "barcode_type": "code128"},
        )
        label_item = html.split('class="label-item" style="', 1)[1].split('"', 1)[0]
        assert "width: 40.0mm;" in label_item
        page_rule = html.split("@page {", 1)[1].split("}", 1)[0]
        assert "size: 40.0mm 25.0mm;" in page_rule

    def test_label_item_has_no_outer_padding(self):
        """
        .label-item НЕ має padding: 2mm — зовнішній padding створював
        «білу рамку 2мм» між краєм канваса і рамкою етикетки.

        Шаблон сам керує відступами контенту (body padding/border);
        обгортка лишається на місці (width/height/box-sizing) і розмір
        не змінюється завдяки box-sizing: border-box.
        """
        html = PriceTagPrintService.render_labels_sequential(
            FULL_TEMPLATE, [PRODUCT], LABEL_SETTINGS
        )
        label_item_style = html.split('class="label-item" style="', 1)[1].split('"', 1)[0]

        # Зовнішній padding прибрано — рамка шаблону доходить до краю
        assert "padding: 2mm" not in label_item_style
        # Обгортка на місці: розміри та box-sizing збережені
        assert "width: 48.0mm;" in label_item_style
        assert "height: 40.0mm;" in label_item_style
        assert "box-sizing: border-box;" in label_item_style

    def test_build_label_html_caps_width(self):
        """_build_label_html обмежує ширину 48мм навіть при прямому виклику."""
        label = PriceTagPrintService._build_label_html("<div>x</div>", 58.0, 40.0, 2.0)
        assert "width: 48.0mm;" in label


# ─── Масштаб QR-коду ───────────────────────────────────────────────────────

@pytest.mark.skipif(not HAS_QRCODE, reason="qrcode не встановлено")
class TestQrScale:
    """
    Тести масштабу QR-коду: сторона QR = box_size_mm (висота штрих-коду).

    Раніше було box_size_mm * 2 → QR ставав 2× більшим (14×14мм замість
    7×7мм) і разом із підписом цифр обрізався контейнером шаблону.
    """

    def test_qr_svg_size_equals_box_size(self):
        """QR 7×7мм (НЕ 14×14мм) для цінника 25мм (height_mm=7)."""
        svg = _generate_qr_svg("4820000000010", 7)
        assert 'width: 7mm; height: 7mm' in svg
        assert "14mm" not in svg
        assert "14 mm" not in svg

    def test_qr_svg_default_size_is_12mm(self):
        """Дефолтний box_size_mm=12 → QR 12×12мм (для етикеток 40мм)."""
        svg = _generate_qr_svg("4820000000010")
        assert 'width: 12mm; height: 12mm' in svg

    def test_qr_svg_is_square(self):
        """QR квадратний: width == height."""
        svg = _generate_qr_svg("4820000000010", 7)
        w = svg.split("width: ", 1)[1].split("mm", 1)[0]
        h = svg.split("height: ", 1)[1].split("mm", 1)[0]
        assert w == h == "7"


# ─── Кастомні фрагменти без <body> ──────────────────────────────────────────

class TestFragmentRender:
    """Кастомні шаблони-фрагменти без <body> не ламаються."""

    def test_fragment_grid_still_works(self):
        """Фрагмент без body: сітка рендериться без обгортки у div."""
        html = PriceTagPrintService.render_price_tags_grid(
            "<div>{{title}}</div>",
            [PRODUCT],
            {"width_mm": 40, "height_mm": 25, "gap_mm": 3, "margin_mm": 10,
             "fields": ["title", "price", "barcode"]},
        )
        cell = html.split('<div class="tag-cell">', 1)[1]
        assert cell.startswith("<div>Хліб білий</div>")
        assert html.count("<html") == 1

    def test_fragment_label_still_works(self):
        """Фрагмент без body: етикетка рендериться без обгортки у div."""
        html = PriceTagPrintService.render_labels_sequential(
            "<div>{{title}}</div>",
            [PRODUCT],
            {"width_mm": 58, "height_mm": 40, "gap_mm": 2,
             "fields": ["title", "price", "barcode"]},
        )
        assert "Хліб білий" in html
        assert html.count("<html") == 1
