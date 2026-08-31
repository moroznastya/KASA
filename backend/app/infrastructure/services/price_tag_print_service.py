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

БЕЗПЕКА:
  Усі дані товару (title, barcode, article, category, created_date) та
  значення налаштувань, що підставляються в HTML, проходять екранування
  через html.escape(quote=True) для запобігання Stored XSS-атак.
"""

from __future__ import annotations

import hashlib
import html
import io
import logging
import math
import re
from datetime import UTC, datetime

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


def _escape_html(value) -> str:
    """
    Екранує значення для безпечної вставки в HTML.

    Використовує html.escape з quote=True — екранує &, <, >, " та '.
    Захищає від Stored XSS через дані товару/налаштувань.

    Args:
        value: значення для екранування (будь-який тип — буде перетворено на str).

    Returns:
        Безпечний HTML-рядок.
    """
    return html.escape(str(value), quote=True)


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

    БЕЗПЕКА:
        Текст, що виводиться у <span> (fallback та підпис), екранується
        через _escape_html для запобігання XSS. Для кодування штрих-коду
        використовується оригінальний (неекранований) текст.
    """
    if not barcode_text or not barcode_text.strip():
        return ""

    barcode_type = barcode_type.lower().strip()

    # ── QR-код ──────────────────────────────────────────────────────────────
    if barcode_type == "qr":
        return _generate_qr_svg(barcode_text, height_mm)

    # ── Code128 (за замовчуванням) ─────────────────────────────────────────
    if not HAS_BARCODE:
        # Fallback: екрануємо текст перед вставкою в <span>
        safe_text = _escape_html(barcode_text)
        return (
            f'<span style="font-family: monospace; font-size: 10px; '
            f'letter-spacing: 1px;">{safe_text}</span>'
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
        # Для кодування використовуємо ОРИГІНАЛЬНИЙ текст (без екранування)
        code = code_class(barcode_text, writer=writer)
        code.write(svg_buffer)
        svg_str = svg_buffer.getvalue().decode("utf-8")

        # Видаляємо текст всередині SVG (write_text=False не працює в деяких версіях)
        svg_str = re.sub(r'<text[^>]*>.*?</text>', '', svg_str, flags=re.DOTALL)
        svg_match = re.search(r'<svg[^>]*>.*?</svg>', svg_str, re.DOTALL)
        if not svg_match:
            # Fallback: екрануємо текст перед вставкою в <span>
            safe_text = _escape_html(barcode_text)
            return (
                f'<span style="font-family: monospace; font-size: 10px;">'
                f'{safe_text}</span>'
            )

        svg_tag = svg_match.group(0)
        svg_tag = svg_tag.replace(
            "<svg",
            '<svg style="max-width: 100%; height: auto;"',
        )

        display_text = barcode_text[:MAX_BARCODE_TEXT_LEN]
        if len(barcode_text) > MAX_BARCODE_TEXT_LEN:
            display_text += "…"

        # Екрануємо підпис під штрих-кодом (захист від XSS)
        safe_display_text = _escape_html(display_text)

        logger.info(
            "PRICE_TAG_BARCODE128 | text=%s height_mm=%.1f module_width=%.2f caption_len=%d",
            str(barcode_text)[:20], height_mm, 0.25, len(display_text),
        )

        return (
            f'<div style="display: flex; flex-direction: column; '
            f'align-items: center;">'
            f'{svg_tag}'
            f'<span style="font-family: monospace; font-size: 9px; '
            f'font-weight: bold; color: #000; margin-top: 1px; '
            f'letter-spacing: 0.5px;">'
            f'{safe_display_text}</span>'
            f'</div>'
        )

    except Exception as e:
        logger.warning("Помилка генерації штрих-коду для '%s': %s", barcode_text, e)
        # Fallback: екрануємо текст перед вставкою в <span>
        safe_text = _escape_html(barcode_text)
        return (
            f'<span style="font-family: monospace; font-size: 10px;">'
            f'{safe_text}</span>'
        )


def _generate_qr_svg(data: str, box_size_mm: float = 12) -> str:
    """
    Генерує SVG QR-коду.

    Args:
        data: дані для кодування
        box_size_mm: розмір модуля в міліметрах

    Returns:
        HTML-рядок з SVG-зображенням QR-коду + підпис цифрами внизу.

    БЕЗПЕКА:
        Текст, що виводиться у <span> (fallback та підпис), екранується
        через _escape_html для запобігання XSS. Для кодування QR-коду
        використовується оригінальний (неекранований) текст.
    """
    if not HAS_QRCODE:
        # Fallback: показати текст як monospace (екранований)
        safe_data = _escape_html(data)
        return (
            f'<span style="font-family: monospace; font-size: 10px; '
            f'letter-spacing: 1px;">[QR: {safe_data}]</span>'
        )

    try:
        qr = qrcode.QRCode(
            version=None,  # auto
            error_correction=qrcode.constants.ERROR_CORRECT_M,
            box_size=3,   # буде масштабовано через viewBox
            border=1,
        )
        # Для кодування використовуємо ОРИГІНАЛЬНІ дані (без екранування)
        qr.add_data(data)
        qr.make(fit=True)

        # Генеруємо SVG
        svg_buffer = io.BytesIO()
        qr.make_image(image_factory=SvgPathImage).save(svg_buffer)
        svg_str = svg_buffer.getvalue().decode("utf-8")

        # Додаємо стилі для розміру.
        # QR — квадратний, сторона = box_size_mm (висота штрих-коду), щоб
        # QR+підпис вміщувались у контейнер шаблону (НЕ *2 — інакше QR
        # виходить 2× більшим і обрізається разом із підписом цифр).
        svg_str = svg_str.replace(
            "<svg",
            f'<svg style="width: {box_size_mm}mm; height: {box_size_mm}mm;"',
        )

        # Обрізаємо текст для підпису (аналогічно code128)
        display_text = data[:MAX_BARCODE_TEXT_LEN]
        if len(data) > MAX_BARCODE_TEXT_LEN:
            display_text += "…"

        # Екрануємо підпис під QR-кодом (захист від XSS)
        safe_display_text = _escape_html(display_text)

        logger.info(
            "PRICE_TAG_QR | text=%s text_len=%d box_size_mm=%.1f svg_style=%s caption=%s",
            str(data)[:20], len(data), box_size_mm,
            f'<svg style="width: {box_size_mm}mm; height: {box_size_mm}mm;"',
            'присутній',
        )

        return (
            f'<div style="display: flex; flex-direction: column; '
            f'align-items: center;">'
            f'{svg_str}'
            f'<span style="font-family: monospace; font-size: 9px; '
            f'font-weight: bold; color: #000; margin-top: 1px; '
            f'letter-spacing: 0.5px;">'
            f'{safe_display_text}</span>'
            f'</div>'
        )

    except Exception as e:
        logger.warning("Помилка генерації QR-коду для '%s': %s", data, e)
        # Fallback: екрануємо текст перед вставкою в <span>
        safe_data = _escape_html(data)
        return (
            f'<span style="font-family: monospace; font-size: 10px;">'
            f'[QR: {safe_data}]</span>'
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
    def _extract_body(html: str) -> tuple[str, str]:
        """
        Витягує атрибути та вміст <body> з повного HTML-документа.

        Шаблони цінників/етикеток зберігаються в БД як ПОВНІ HTML-документи
        (<html><head>...</head><body style="...">...</body></html>).
        Вкладати такий документ у .tag-cell / .label-item не можна: HTML5-парсер
        викидає вкладені <html>/<body> теги, через що body-атрибути (рамка,
        padding, font) втрачаються. Цей метод витягує атрибути та вміст body,
        щоб обгорнути їх у звичайний <div>.

        Args:
            html: рядок HTML (повний документ або фрагмент).

        Returns:
            (body_attrs, body_content):
              body_attrs   — рядок атрибутів body (наприклад 'style="..."'),
                             або '' якщо атрибутів немає / body не знайдено.
              body_content — вміст між <body> і </body>, або очищений HTML
                             без DOCTYPE/<html>/<head>, якщо body не знайдено.
        """
        body_attrs = ""
        body_content = ""
        found = False

        # 1) <body> з атрибутами та закриваючим тегом
        match = re.search(
            r'<body\s+([^>]*)>([\s\S]*?)</body>',
            html,
            flags=re.IGNORECASE,
        )
        if match:
            body_attrs, body_content = match.group(1).strip(), match.group(2)
            found = True

        # 2) <body> без атрибутів (але з закриваючим тегом)
        if not found:
            match = re.search(r'<body>([\s\S]*?)</body>', html, flags=re.IGNORECASE)
            if match:
                body_attrs, body_content = "", match.group(1)
                found = True

        # 3) <body> з атрибутами, без закриваючого тега (до кінця)
        if not found:
            match = re.search(r'<body\s+([^>]*)>([\s\S]*)$', html, flags=re.IGNORECASE)
            if match:
                body_attrs, body_content = match.group(1).strip(), match.group(2).rstrip()
                found = True

        # 4) <body> без атрибутів і без закриваючого тега
        if not found:
            match = re.search(r'<body>([\s\S]*)$', html, flags=re.IGNORECASE)
            if match:
                body_attrs, body_content = "", match.group(1).rstrip()
                found = True

        # 5) <body> не знайдено — прибираємо обгортку документа, щоб кастомні
        #    фрагменти без body працювали як є.
        if not found:
            cleaned = re.sub(r'<!DOCTYPE[^>]*>', '', html, flags=re.IGNORECASE)
            cleaned = re.sub(r'<html[^>]*>', '', cleaned, flags=re.IGNORECASE)
            cleaned = re.sub(r'</html>', '', cleaned, flags=re.IGNORECASE)
            cleaned = re.sub(r'<head>[\s\S]*?</head>', '', cleaned, flags=re.IGNORECASE)
            body_content = cleaned.strip()

        logger.info(
            "PRICE_TAG_EXTRACT_BODY | found=%s body_attrs_len=%d content_len=%d",
            'так' if found else 'ні', len(body_attrs), len(body_content),
        )
        return body_attrs, body_content

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

        БЕЗПЕКА:
            Всі дані товару (title, barcode, article, category, created_date)
            та значення з extra_context екрануються перед підстановкою в HTML.
            Виняток — {{barcode_image}}: це згенерований SVG-код, в якому
            текст вже екрановано всередині _generate_barcode_svg().
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

            show = True if enabled_fields is None else field_name in enabled_fields

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
        # ВАЖЛИВО: Всі значення, що підставляються в HTML, проходять
        # екранування через _escape_html (захист від Stored XSS).
        # Виняток — "barcode_image": це згенерований SVG, де текст вже
        # екрановано всередині _generate_barcode_svg() / _generate_qr_svg().
        replacements: dict[str, str] = {
            # Дані товару (екрануємо — можуть містити HTML/JS від користувача)
            "title": _escape_html(product.get("title", "")),
            "name": _escape_html(product.get("title", product.get("name", ""))),
            "price": _escape_html(product.get("price", "")),
            "barcode": _escape_html(barcode_val),
            "article": _escape_html(product.get("article", product.get("sku", ""))),
            "category": _escape_html(product.get("category", "")),
            "created_date": _escape_html(product.get("created_date", "")),
            # Штрих-код (SVG з підписом) — НЕ екрануємо, бо це згенерований
            # HTML-код; текст усередині вже екрановано.
            "barcode_image": _generate_barcode_svg(
                barcode_val,
                height_mm=barcode_height_mm,
                barcode_type=barcode_type,
            ),
            # Тип та розмір штрих-коду (екрануємо для надійності)
            "barcode_type": _escape_html(barcode_type),
            "barcode_height_mm": _escape_html(str(barcode_height_mm)),
            # Розміри етикетки/цінника (з extra_context, екрануємо для надійності)
            "width": _escape_html(str(extra.get("width", ""))),
            "height": _escape_html(str(extra.get("height", ""))),
        }

        for key, value in replacements.items():
            result = result.replace("{{" + key + "}}", str(value))
            result = result.replace("{{product." + key + "}}", str(value))

        # ── Крок 4: Нормалізація повного HTML-документа до фрагмента ────────
        # Шаблон з БД — повний HTML-документ (<html><body style="...">...</body></html>).
        # Вкладати <html>/<body> у .tag-cell / .label-item НЕ МОЖНА: HTML5-парсер
        # викидає вкладені теги → body-атрибути (рамка, padding, font) втрачаються
        # → рамка не друкується, контент обрізається. Тому витягуємо атрибути
        # та вміст body і обгортаємо у <div>, зберігаючи стилі (рамку, розміри).
        body_attrs, body_content = PriceTagPrintService._extract_body(result)
        logger.info(
            "PRICE_TAG_RENDER_SINGLE | template_len=%d has_body=%s barcode_type=%s barcode_h=%.1f price=%s name_len=%d",
            len(template), 'так' if body_attrs else 'ні', barcode_type,
            barcode_height_mm, str(product.get("price", ""))[:10],
            len(str(product.get("title", ""))),
        )
        if re.search(r'<body[\s>]', result, flags=re.IGNORECASE):
            style_match = re.search(r'style="([^"]*)"', body_attrs)
            if style_match:
                return (
                    f'<div style="{style_match.group(1)}">'
                    f'{body_content}</div>'
                )
            return f'<div>{body_content}</div>'

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
        print_mode: str = "escpos",
    ) -> str:
        """Генерує HTML однієї етикетки (термопринтер).

        Ширина етикетки залежить від режиму друку (print_mode):
          - 'system' → effective_width = width_mm (ПОВНА ширина етикетки,
            без обмеження 48мм) — для системного друку (CUPS), напр.
            Xprinter XP-420B (832 dots, друкована область 104мм).
          - 'escpos' → effective_width = min(width_mm, 48.0) — реальна
            друкована область 58мм термопринтера (384 dots @203dpi):
            html2canvas знімає canvas з пропорціями 48×40 = 384×320 dots,
            і Rust масштабує його РІВНОМІРНО без спотворення.
        Для етикеток ≤48мм обидва режими збігаються. (Тільки для
        термо-етикеток; A4-цінники використовують render_price_tags_grid
        зі своєю сіткою.)
        """
        effective_width = (
            width_mm if print_mode == "system" else min(width_mm, 48.0)
        )
        # padding: 2mm прибрано — шаблон сам керує відступами
        # (body padding/border); зовнішній padding створював «білу рамку»
        # між краєм канваса і рамкою етикетки; розмір не змінюється
        # (box-sizing: border-box). Рамка етикетки тепер доходить
        # ДО КРАЮ друкованої області.
        return (
            f'<div class="label-item" style="'
            f"width: {effective_width}mm; "
            f"height: {height_mm}mm; "
            f"margin-bottom: {gap_mm}mm; "
            f"box-sizing: border-box; "
            f"overflow: hidden; "
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
        fields = settings.get("fields")
        enabled_fields = set(fields) if fields else None

        # Адаптивне обмеження висоти штрих-коду: щоб цифри під кодом
        # не обрізались (цінник 25мм → max 7мм; етикетка 40мм → max 11.2мм).
        # Користувач може зменшити через UI, але не може задати завелике
        # значення, що ламає друк.
        settings = dict(settings)
        barcode_h = float(settings.get("barcode_height_mm", 7))
        settings["barcode_height_mm"] = min(barcode_h, max(3.0, height_mm * 0.28))

        logger.info(
            "PRICE_TAG_GRID | w=%.1f h=%.1f gap=%.1f margin=%.1f products=%d barcode_type=%s barcode_h=%.1f fields=%s",
            width_mm, height_mm, gap_mm, margin_mm, len(products),
            settings.get('barcode_type'), settings.get('barcode_height_mm', 7),
            list(settings.get('fields') or []),
        )

        expanded = PriceTagPrintService._expand_products(products)

        # Додатковий контекст для шаблону
        extra_context = {
            "width": str(width_mm),
            "height": str(height_mm),
            "barcode_type": settings.get("barcode_type", "code128"),
            "barcode_height_mm": settings.get("barcode_height_mm", 7),
        }

        rendered_items = []
        for product in expanded:
            if not product.get("created_date"):
                product["created_date"] = datetime.now(UTC).strftime("%d.%m.%Y")
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
                # Єдине поле сторінки — @page margin; grid-container margin = 0,
                # інакше контент починається на 2×margin від краю і сітка
                # виходить за праву межу друкованої області.
                f"width: {page_width_mm - 2 * margin_mm}mm; "
                f"margin: 0;"
                f'">{"".join(grid_cells)}</div></div>'
            )
            if page_idx < total_pages - 1:
                page_html += '\n<div style="page-break-after: always;"></div>'
            pages_html.append(page_html)

        result_html = f'''<!DOCTYPE html>
<html lang="uk">
<head>
<meta charset="UTF-8">
<title>Цінники A4</title>
<style>
@page {{ size: A4; margin: {margin_mm}mm; }}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: Arial, Helvetica, sans-serif; font-size: 8pt; line-height: 1.2; }}
/* .page використовує КОРИСНУ висоту сторінки (page_height − 2×margin):
   @page margin (10мм) забирає по 10мм зверху/знизу → 297 − 20 = 277мм.
   Якщо лишити 297мм, контент виходить за друковану область 277мм →
   порожня друга сторінка або обрізання при друку A4. */
.page {{ width: 100%; min-height: {page_height_mm - 2 * margin_mm}mm; }}
.grid-container {{ display: grid; }}
.tag-cell {{
    width: {width_mm}mm; height: {height_mm}mm; overflow: hidden;
    border: none; padding: 0;
    display: flex; flex-direction: column; justify-content: stretch;
    align-items: stretch; text-align: center;
}}
@media print {{ .page {{ page-break-after: always; }} }}
</style>
</head>
<body>
{"".join(pages_html)}
</body>
</html>'''

        logger.info(
            "PRICE_TAG_GRID_DONE | html_md5=%s html_len=%d total_labels=%d total_pages=%d grid=%dx%d",
            hashlib.md5(result_html.encode("utf-8")).hexdigest(),
            len(result_html), total_labels, total_pages, cols, rows,
        )
        return result_html

    # ─── ОСНОВНИЙ МЕТОД: етикетки на термопринтер ─────────────────────────

    @staticmethod
    def render_labels_sequential(
        template_content: str,
        products: list[dict],
        settings: dict,
        print_mode: str | None = None,
    ) -> str:
        """Рендерить HTML з етикетками для термопринтера — одна за одною.

        Режим друку (print_mode):
          - 'system' → effective_width = width_mm (ПОВНА ширина етикетки,
            без обмеження 48мм) — для системного друку (CUPS), напр.
            Xprinter XP-420B (832 dots, друкована область 104мм).
          - 'escpos' → effective_width = min(width_mm, 48.0) — для 58мм
            термопринтера (384 dots @203dpi).

        Якщо print_mode не передано — береться з settings['print_mode'],
        інакше 'escpos' (зворотна сумісність зі старою поведінкою).
        """
        if not products or not template_content:
            return PriceTagPrintService._empty_html("label")

        width_mm = float(settings.get("width_mm", 58))
        height_mm = float(settings.get("height_mm", 40))
        gap_mm = float(settings.get("gap_mm", 2))
        fields = settings.get("fields")
        enabled_fields = set(fields) if fields else None

        # Режим друку: явний параметр → settings['print_mode'] → 'escpos'
        # (стара поведінка min(width_mm, 48.0) для 58мм термо з 384 dots).
        if print_mode is None:
            print_mode = str(settings.get("print_mode", "escpos"))

        # Ефективна ширина етикетки:
        #   - 'system' (CUPS, напр. XP-420B з друкованою областю 104мм) →
        #     ПОВНА ширина width_mm; контент заповнює всю етикетку;
        #     @page = width_mm × height_mm.
        #   - 'escpos' (58мм термо, 384 dots) → min(width_mm, 48.0):
        #     html2canvas знімає canvas 48×40мм (пропорції 1.2), Rust
        #     масштабує до 384×320 dots (1.2) РІВНОМІРНО, без спотворення.
        # Якщо width_mm <= 48 (напр. 40×25) — обидва режими збігаються.
        # Зміна стосується ТІЛЬКИ термо-етикеток
        # (render_labels_sequential/_build_label_html), НЕ A4-цінників.
        effective_width = (
            width_mm if print_mode == "system" else min(width_mm, 48.0)
        )

        logger.info(
            "PRICE_TAG_SEQUENTIAL | w=%.1f h=%.1f gap=%.1f eff_w=%.1f print_mode=%s products=%d barcode_type=%s barcode_h=%.1f fields=%s",
            width_mm, height_mm, gap_mm, effective_width, print_mode, len(products),
            settings.get('barcode_type'), settings.get('barcode_height_mm', 12),
            list(settings.get('fields') or []),
        )

        expanded = PriceTagPrintService._expand_products(products)
        total_labels = len(expanded)

        # Додатковий контекст для шаблону
        # {{width}} = ефективна ширина (48мм для 58мм етикетки), щоб
        # внутрішні розрахунки шаблону збігались із реальним CSS-контейнером.
        extra_context = {
            "width": str(effective_width),
            "height": str(height_mm),
            "barcode_type": settings.get("barcode_type", "code128"),
            "barcode_height_mm": settings.get("barcode_height_mm", 12),
        }

        labels_html = []
        for i, product in enumerate(expanded):
            # Етикетка ЗАВЖДИ друкує ПОТОЧНУ (сьогоднішню) дату локального
            # часу системи — БЕЗ timezone.utc (інакше о 00:00-02:00 UTC+3
            # показує вчорашній день). Безумовне перезаписування:
            # дата на етикетці = дата друку, а не дата створення товару.
            # (A4-цінники render_price_tags_grid НЕ чіпаємо.)
            product["created_date"] = datetime.now().strftime("%d.%m.%Y")

            rendered = PriceTagPrintService._render_single(
                template_content,
                product,
                enabled_fields,
                extra_context=extra_context,
            )
            label = PriceTagPrintService._build_label_html(
                rendered, effective_width, height_mm, gap_mm, print_mode=print_mode
            )
            if i < total_labels - 1:
                label += '\n<div style="page-break-after: always;"></div>'
            labels_html.append(label)

        result_html = f'''<!DOCTYPE html>
<html lang="uk">
<head>
<meta charset="UTF-8">
<title>Етикетки термопринтер</title>
<style>
/* @page = ефективний розмір етикетки:
   - 'system' (CUPS): повна ширина width_mm × height_mm (напр. 104×40мм
     для Xprinter XP-420B) — контент заповнює всю етикетку.
   - 'escpos' (58мм термо): 48×40мм — html2canvas знімає СТОРІНКУ
     цілком, тому і сторінка, і .label-item мають бути 48×40мм — інакше
     Rust масштабує canvas 58×40 (1.45) у 384×320 (1.2) нерівномірно
     → спотворення. */
@page {{ size: {effective_width}mm {height_mm}mm; margin: 0mm; }}
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

        logger.info(
            "PRICE_TAG_SEQ_DONE | html_md5=%s html_len=%d total_labels=%d",
            hashlib.md5(result_html.encode("utf-8")).hexdigest(),
            len(result_html), total_labels,
        )
        return result_html

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
