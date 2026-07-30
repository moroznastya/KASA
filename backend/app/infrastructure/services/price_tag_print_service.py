"""
Сервіс для рендеру цінників та етикеток у вигляді HTML-сітки.

Містить статичні методи:
  - render_price_tags_grid()    — цінники на A4 (CSS Grid)
  - render_labels_sequential()  — етикетки на термопринтер (одна за одною)

Використовує шаблони з {{variable}} для підстановки значень товару.

Підтримувані змінні шаблону:
  {{title}}, {{name}}                   — назва товару
  {{price}}                              — ціна
  {{barcode}}                            — текст штрих-коду (для цінників)
  {{barcode_image}}                      — SVG зображення (Code128 або QR)
  {{article}}, {{category}}, {{created_date}}
  {{barcode_type}}                       — "code128" (за умовч.) або "qr"
  {{barcode_height_mm}}                  — висота штрих-коду в мм (за умовч. 12)
  {{width}}, {{height}}                  — розмір етикетки/цінника

Підтримувані умовні блоки:
  {{#if show_barcode}}...{{/if}}
  {{#if show_price}}...{{/if}}
  {{#if show_article}}...{{/if}}
  {{#if show_created_date}}...{{/if}}
  {{#if show_category}}...{{/if}}
  — якщо поле є в списку fields, блок показується, інакше прибирається.
"""

from __future__ import annotations

import io
import math
import logging
import re
from datetime import datetime, timezone

logger = logging.getLogger(__name__)

# ─── Генерація SVG штрих-коду (Code128 / QR) ────────────────────────────────

try:
    import barcode
    from barcode.writer import SVGWriter

    HAS_BARCODE = True
except ImportError:
    HAS_BARCODE = False
    logger.warning("python-barcode не встановлено. {{barcode_image}} буде показувати текст.")

try:
    import qrcode
    from qrcode.image.svg import SvgPathImage

    HAS_QRCODE = True
except ImportError:
    HAS_QRCODE = False
    logger.warning("qrcode не встановлено. QR-код буде показано як текст.")

MAX_BARCODE_TEXT_LEN = 20


def _generate_barcode_svg(
    barcode_text: str,
    height_mm: float = 12,
    barcode_type: str = "code128",
) -> str:
    """
    Генерує SVG-рядок штрих-коду (Code128) або QR-коду.

    Args:
        barcode_text: значення штрих-коду (наприклад, "4820012345678")
        height_mm: висота штрих-коду в міліметрах
        barcode_type: тип кодування ("code128" або "qr")

    Returns:
        HTML-рядок з SVG-зображенням + підпис цифрами (для code128),
        або простий текст, якщо бібліотека не встановлена.
    """
    if not barcode_text or not barcode_text.strip():
        return ""

    barcode_type = barcode_type.lower().strip()

    # ── QR-код ──────────────────────────────────────────────────────────────
    if barcode_type == "qr":
        return _generate_qr_svg(barcode_text, height_mm)

    # ── Code128 (за замовчуванням) ─────────────────────────────────────────
    if not HAS_BARCODE:
        return (
            f'<span style="font-family: monospace; font-size: 10px; '
            f'letter-spacing: 1px;">{barcode_text}</span>'
        )

    try:
        code_class = barcode.get_barcode_class("code128")
        writer = SVGWriter()
        writer.set_options({
            "module_width": 0.25,
            "module_height": height_mm,
            "quiet_zone": 1.0,
            "font_size": 0,
            "text_distance": 0,
            "write_text": False,
            "background": "white",
        })

        svg_buffer = io.BytesIO()
        code = code_class(barcode_text, writer=writer)
        code.write(svg_buffer)
        svg_str = svg_buffer.getvalue().decode("utf-8")

        # Видаляємо текст всередині SVG (write_text=False не працює в деяких версіях)
        svg_str = re.sub(r'<text[^>]*>.*?</text>', '', svg_str, flags=re.DOTALL)
        svg_match = re.search(r'<svg[^>]*>.*?</svg>', svg_str, re.DOTALL)
        if not svg_match:
            return (
                f'<span style="font-family: monospace; font-size: 10px;">'
                f'{barcode_text}</span>'
            )

        svg_tag = svg_match.group(0)
        svg_tag = svg_tag.replace(
            "<svg",
            '<svg style="max-width: 100%; height: auto;"',
        )

        display_text = barcode_text[:MAX_BARCODE_TEXT_LEN]
        if len(barcode_text) > MAX_BARCODE_TEXT_LEN:
            display_text += "…"

        return (
            f'<div style="display: flex; flex-direction: column; '
            f'align-items: center;">'
            f'{svg_tag}'
            f'<span style="font-family: monospace; font-size: 7px; '
            f'color: #000; margin-top: 1px; letter-spacing: 0.5px;">'
            f'{display_text}</span>'
            f'</div>'
        )

    except Exception as e:
        logger.warning("Помилка генерації штрих-коду для '%s': %s", barcode_text, e)
        return (
            f'<span style="font-family: monospace; font-size: 10px;">'
            f'{barcode_text}</span>'
        )


def _generate_qr_svg(data: str, box_size_mm: float = 12) -> str:
    """
    Генерує SVG QR-коду.

    Args:
        data: дані для кодування
        box_size_mm: розмір модуля в міліметрах

    Returns:
        HTML-рядок з SVG-зображенням QR-коду + підпис цифрами внизу
    """
    if not HAS_QRCODE:
        # Fallback: показати текст як monospace
        return (
            f'<span style="font-family: monospace; font-size: 10px; '
            f'letter-spacing: 1px;">[QR: {data}]</span>'
        )

    try:
        qr = qrcode.QRCode(
            version=None,  # auto
            error_correction=qrcode.constants.ERROR_CORRECT_M,
            box_size=3,   # буде масштабовано через viewBox
            border=1,
        )
        qr.add_data(data)
        qr.make(fit=True)

        # Генеруємо SVG
        svg_buffer = io.BytesIO()
        qr.make_image(image_factory=SvgPathImage).save(svg_buffer)
        svg_str = svg_buffer.getvalue().decode("utf-8")

        # Додаємо стилі для розміру
        svg_str = svg_str.replace(
            "<svg",
            f'<svg style="width: {box_size_mm * 2}mm; height: {box_size_mm * 2}mm;"',
        )

        # Обрізаємо текст для підпису (аналогічно code128)
        display_text = data[:MAX_BARCODE_TEXT_LEN]
        if len(data) > MAX_BARCODE_TEXT_LEN:
            display_text += "…"

        return (
            f'<div style="display: flex; flex-direction: column; '
            f'align-items: center;">'
            f'{svg_str}'
            f'<span style="font-family: monospace; font-size: 7px; '
            f'color: #000; margin-top: 1px; letter-spacing: 0.5px;">'
            f'{display_text}</span>'
            f'</div>'
        )

    except Exception as e:
        logger.warning("Помилка генерації QR-коду для '%s': %s", data, e)
        return (
            f'<span style="font-family: monospace; font-size: 10px;">'
            f'[QR: {data}]</span>'
        )


# ─── Відповідність полів товару до ключів шаблону ────────────────────────────
FIELD_TO_TEMPLATE_KEY = {
    "title": "title",
    "name": "title",
    "price": "price",
    "barcode": "barcode",
    "article": "article",
    "sku": "article",
    "category": "category",
    "created_date": "created_date",
    "date": "created_date",
}


class PriceTagPrintService:
    """Сервіс для генерації HTML-документів для друку цінників та етикеток."""

    # ─── Рендер шаблону з підтримкою Handlebars-блоків ─────────────────────

    @staticmethod
    def _render_single(
        template: str,
        product: dict,
        enabled_fields: set | None = None,
        extra_context: dict | None = None,
    ) -> str:
        """
        Замінює {{variable}} та обробляє {{#if ...}}...{{/if}} блоки.

        Args:
            template: HTML-шаблон зі змінними та умовними блоками
            product: словник з даними товару
            enabled_fields: множина дозволених полів.
                Якщо None — всі блоки показуються.
            extra_context: додатковий контекст (width, height, тощо)

        Підтримувані змінні:
          {{title}}, {{name}}              — назва товару
          {{price}}                         — ціна
          {{barcode}}                       — текст штрих-коду (для цінників)
          {{barcode_image}}                 — SVG зображення (Code128 або QR)
          {{article}}, {{category}}, {{created_date}}
          {{barcode_type}}                  — "code128" (за умовч.) або "qr"
          {{barcode_height_mm}}             — висота штрих-коду в мм (за умовч. 12)
          {{width}}, {{height}}             — розмір етикетки/цінника

        Умовні блоки:
          {{#if show_barcode}}...{{/if}}
          {{#if show_price}}...{{/if}}
          {{#if show_article}}...{{/if}}
          {{#if show_created_date}}...{{/if}}
          {{#if show_category}}...{{/if}}
        """
        result = template

        # ── Крок 1: Обробка Handlebars блоків ──────────────────────────────
        if_block_map = {
            "show_barcode": "barcode",
            "show_price": "price",
            "show_article": "article",
            "show_created_date": "created_date",
            "show_category": "category",
        }

        def _process_if_block(match: re.Match) -> str:
            full_block = match.group(0)
            condition = match.group(1)
            inner_content = match.group(2)

            field_name = if_block_map.get(condition)
            if field_name is None:
                return full_block

            if enabled_fields is None:
                show = True
            else:
                show = field_name in enabled_fields

            return inner_content if show else ""

        result = re.sub(
            r'\{\{#if\s+(show_\w+)\}\}(.*?)\{\{/if\}\}',
            _process_if_block,
            result,
            flags=re.DOTALL,
        )

        # ── Крок 2: Підготовка даних ──────────────────────────────────────
        extra = extra_context or {}
        barcode_val = product.get("barcode", "")

        # Тип штрих-коду: з шаблону → з extra_context → за замовчуванням
        barcode_type = "code128"
        barcode_height_mm = 12

        # Спочатку пробуємо з extra_context (передається з API/сервісу)
        if "barcode_type" in extra:
            barcode_type = str(extra["barcode_type"])
        if "barcode_height_mm" in extra:
            barcode_height_mm = float(extra["barcode_height_mm"])

        # ── Крок 3: Заміна змінних ────────────────────────────────────────
        replacements: dict[str, str] = {
            # Дані товару
            "title": product.get("title", ""),
            "name": product.get("title", product.get("name", "")),
            "price": product.get("price", ""),
            "barcode": barcode_val,
            "article": product.get("article", product.get("sku", "")),
            "category": product.get("category", ""),
            "created_date": product.get("created_date", ""),
            # Штрих-код (SVG з підписом)
            "barcode_image": _generate_barcode_svg(
                barcode_val,
                height_mm=barcode_height_mm,
                barcode_type=barcode_type,
            ),
            # Тип та розмір штрих-коду
            "barcode_type": barcode_type,
            "barcode_height_mm": str(barcode_height_mm),
            # Розміри етикетки/цінника (з extra_context)
            "width": str(extra.get("width", "")),
            "height": str(extra.get("height", "")),
        }

        for key, value in replacements.items():
            result = result.replace("{{" + key + "}}", str(value))
            result = result.replace("{{product." + key + "}}", str(value))

        return result

    @staticmethod
    def _expand_products(products: list[dict]) -> list[dict]:
        """Розмножує товари згідно поля copies."""
        expanded = []
        for product in products:
            copies = product.get("copies", 1)
            for _ in range(copies):
                expanded.append(product)
        return expanded

    @staticmethod
    def _build_label_html(
        rendered: str,
        width_mm: float,
        height_mm: float,
        gap_mm: float,
    ) -> str:
        """Генерує HTML однієї етикетки."""
        return (
            f'<div class="label-item" style="'
            f"width: {width_mm}mm; "
            f"height: {height_mm}mm; "
            f"margin-bottom: {gap_mm}mm; "
            f"box-sizing: border-box; "
            f"overflow: hidden; "
            f"padding: 2mm; "
            f"border: 0.1mm solid #ccc; "
            f'font-family: Arial, sans-serif;'
            f'">{rendered}</div>'
        )

    @staticmethod
    def _calc_grid(
        width_mm: float,
        height_mm: float,
        gap_mm: float,
        page_width_mm: float,
        page_height_mm: float,
        margin_mm: float,
    ) -> tuple[int, int, int]:
        """Обчислює, скільки цінників вміщується на сторінці."""
        usable_width = page_width_mm - 2 * margin_mm
        usable_height = page_height_mm - 2 * margin_mm
        if width_mm <= 0 or height_mm <= 0:
            return (1, 1, 1)
        cols = max(1, int((usable_width + gap_mm) / (width_mm + gap_mm)))
        rows = max(1, int((usable_height + gap_mm) / (height_mm + gap_mm)))
        return (cols, rows, cols * rows)

    # ─── ОСНОВНИЙ МЕТОД: цінники на A4 ─────────────────────────────────────

    @staticmethod
    def render_price_tags_grid(
        template_content: str,
        products: list[dict],
        settings: dict,
    ) -> str:
        """Рендерить HTML з цінниками, розкладеними в сітку на A4."""
        if not products or not template_content:
            return PriceTagPrintService._empty_html("A4")

        width_mm = float(settings.get("width_mm", 40))
        height_mm = float(settings.get("height_mm", 25))
        gap_mm = float(settings.get("gap_mm", 3))
        page_width_mm = float(settings.get("page_width_mm", 210))
        page_height_mm = float(settings.get("page_height_mm", 297))
        margin_mm = float(settings.get("margin_mm", 10))
        fields = settings.get("fields", None)
        enabled_fields = set(fields) if fields else None

        expanded = PriceTagPrintService._expand_products(products)

        # Додатковий контекст для шаблону
        extra_context = {
            "width": str(width_mm),
            "height": str(height_mm),
            "barcode_type": settings.get("barcode_type", "code128"),
            "barcode_height_mm": settings.get("barcode_height_mm", 12),
        }

        rendered_items = []
        for product in expanded:
            if not product.get("created_date"):
                product["created_date"] = datetime.now(timezone.utc).strftime("%d.%m.%Y")
            rendered = PriceTagPrintService._render_single(
                template_content,
                product,
                enabled_fields,
                extra_context=extra_context,
            )
            rendered_items.append(rendered)

        cols, rows, per_page = PriceTagPrintService._calc_grid(
            width_mm, height_mm, gap_mm, page_width_mm, page_height_mm, margin_mm
        )
        total_labels = len(rendered_items)
        total_pages = max(1, math.ceil(total_labels / per_page))

        pages_html = []
        for page_idx in range(total_pages):
            start = page_idx * per_page
            end = min(start + per_page, total_labels)
            page_items = rendered_items[start:end]
            grid_cells = [
                f'<div class="tag-cell">{item}</div>' for item in page_items
            ]
            page_html = (
                f'<div class="page">'
                f'<div class="grid-container" style="'
                f"display: grid; "
                f"grid-template-columns: repeat({cols}, {width_mm}mm); "
                f"grid-template-rows: repeat({rows}, {height_mm}mm); "
                f"gap: {gap_mm}mm; "
                f"width: {page_width_mm - 2 * margin_mm}mm; "
                f"margin: {margin_mm}mm;"
                f'">{"".join(grid_cells)}</div></div>'
            )
            if page_idx < total_pages - 1:
                page_html += '\n<div style="page-break-after: always;"></div>'
            pages_html.append(page_html)

        return f'''<!DOCTYPE html>
<html lang="uk">
<head>
<meta charset="UTF-8">
<title>Цінники A4</title>
<style>
@page {{ size: A4; margin: {margin_mm}mm; }}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: Arial, Helvetica, sans-serif; font-size: 8pt; line-height: 1.2; }}
.page {{ width: {page_width_mm}mm; min-height: {page_height_mm}mm; }}
.grid-container {{ display: grid; }}
.tag-cell {{
    width: {width_mm}mm; height: {height_mm}mm; overflow: hidden;
    border: 0.1mm solid #999; padding: 1.5mm;
    display: flex; flex-direction: column; justify-content: center;
    align-items: center; text-align: center;
}}
@media print {{ .page {{ page-break-after: always; }} }}
</style>
</head>
<body>
{"".join(pages_html)}
</body>
</html>'''

    # ─── ОСНОВНИЙ МЕТОД: етикетки на термопринтер ─────────────────────────

    @staticmethod
    def render_labels_sequential(
        template_content: str,
        products: list[dict],
        settings: dict,
    ) -> str:
        """Рендерить HTML з етикетками для термопринтера — одна за одною."""
        if not products or not template_content:
            return PriceTagPrintService._empty_html("label")

        width_mm = float(settings.get("width_mm", 58))
        height_mm = float(settings.get("height_mm", 40))
        gap_mm = float(settings.get("gap_mm", 2))
        fields = settings.get("fields", None)
        enabled_fields = set(fields) if fields else None

        expanded = PriceTagPrintService._expand_products(products)
        total_labels = len(expanded)

        # Додатковий контекст для шаблону
        extra_context = {
            "width": str(width_mm),
            "height": str(height_mm),
            "barcode_type": settings.get("barcode_type", "code128"),
            "barcode_height_mm": settings.get("barcode_height_mm", 12),
        }

        labels_html = []
        for i, product in enumerate(expanded):
            if not product.get("created_date"):
                product["created_date"] = datetime.now(timezone.utc).strftime("%d.%m.%Y")

            rendered = PriceTagPrintService._render_single(
                template_content,
                product,
                enabled_fields,
                extra_context=extra_context,
            )
            label = PriceTagPrintService._build_label_html(
                rendered, width_mm, height_mm, gap_mm
            )
            if i < total_labels - 1:
                label += '\n<div style="page-break-after: always;"></div>'
            labels_html.append(label)

        return f'''<!DOCTYPE html>
<html lang="uk">
<head>
<meta charset="UTF-8">
<title>Етикетки термопринтер</title>
<style>
@page {{ size: {width_mm}mm {height_mm}mm; margin: 0mm; }}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: Arial, Helvetica, sans-serif; font-size: 7pt; line-height: 1.15; }}
.label-item {{
    display: flex; flex-direction: column; justify-content: center;
    align-items: center; text-align: center;
}}
@media print {{ .label-item {{ page-break-after: always; }} }}
</style>
</head>
<body>
{"".join(labels_html)}
</body>
</html>'''

    @staticmethod
    def _empty_html(page_type: str = "A4") -> str:
        if page_type == "label":
            return (
                '<!DOCTYPE html><html lang="uk"><head><meta charset="UTF-8">'
                '<title>Немає етикеток</title></head>'
                '<body style="font-family: Arial; text-align: center; padding: 20mm;">'
                '<p>Немає товарів для друку</p></body></html>'
            )
        return (
            '<!DOCTYPE html><html lang="uk"><head><meta charset="UTF-8">'
            '<title>Немає цінників</title></head>'
            '<body style="font-family: Arial; text-align: center; padding: 20mm;">'
            '<p>Немає товарів для друку цінників</p></body></html>'
        )
