"""
PrintFontService — модуль керування шрифтом друкованих документів.

Застосовує вибраний користувачем шрифт (налаштування print_font_family,
module='printing') до ВСІХ друкованих HTML: чеків, етикеток, цінників.

Робота модуля:
  1. get_font_family(session) — читає налаштування print_font_family з БД
     через SettingsService (кешується в пам'яті); якщо ключа немає —
     повертає DEFAULT_FONT_FAMILY ('Arial, sans-serif').
  2. apply_font_to_html(html, font_family) — замінює ВСІ font-family
     (inline style + <style> блоки) у HTML на вибраний шрифт. Якщо
     font-family у документі відсутній — додає глобальне CSS-правило
     для body. Якщо шрифт порожній — HTML повертається без змін.
  3. Для Google Font «Bad Script» (НЕ системний) — вбудовує @font-face
     з base64-даними (cyrillic + latin woff2) на початок HTML, щоб шрифт
     відобразився у попередньому перегляді (iframe) та при друці.
"""

from __future__ import annotations

import base64
import logging
import re
from pathlib import Path

logger = logging.getLogger(__name__)

# Регулярний вираз для пошуку оголошень font-family:
#   font-family: <значення>
# Покриває inline style="font-family: X" та CSS-правила у <style>.
# Значення може бути звичайним (Arial, sans-serif), у лапках
# ("Times New Roman", 'Courier New') або змішаним — захоплюється ЦІЛЕ
# значення до ; " ' }. Лапкові блоки дозволені ВСЕРЕДИНІ, щоб НЕ обрізати
# font-family: "Times New Roman", serif на першій лапці.
# ВАЖЛИВО: простий клас [^;"'}]+ зіставляє лише ПРОБІЛ після ':' (бо \s*
# віддає його класу) і ЗАЛИШАЄ лапкове значення сміттям — ламає CSS.
_FONT_FAMILY_RE = re.compile(
    r"font-family\s*:\s*((?:[^;\"'}]+|\"[^\"]*\"|'[^']*')+)",
    flags=re.IGNORECASE,
)


class PrintFontService:
    """Сервіс керування шрифтом друкованих документів."""

    # Шрифт за замовчуванням, якщо налаштування print_font_family не задано
    DEFAULT_FONT_FAMILY = "Arial, sans-serif"

    @staticmethod
    def _bad_script_face_css() -> str:
        """
        Генерує CSS @font-face для Google Font «Bad Script» (base64).

        Читає два woff2-файли з app/assets/fonts/ та вбудовує їх у CSS
        через data URI. Кириличний і латинський піднабори розділені
        unicode-range, щоб браузер завантажував лише потрібні гліфи.

        Returns:
            Рядок CSS з двома @font-face (cyrillic + latin), або порожній
            рядок, якщо файли шрифту відсутні/не читаються.
        """
        # app/infrastructure/services/print_font_service.py
        # → parent.parent.parent = app/ → далі assets/fonts/
        fonts_dir = (
            Path(__file__).resolve().parent.parent.parent
            / "assets"
            / "fonts"
        )
        # (ім'я файлу, unicode-range піднабору)
        subsets = (
            (
                "bad-script-cyrillic.woff2",
                "U+0301, U+0400-045F, U+0490-0491, U+2116",
            ),
            (
                "bad-script-latin.woff2",
                "U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, "
                "U+02DA, U+02DC, U+2000-206F, U+2074, U+20AC, U+2122, "
                "U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD",
            ),
        )
        faces: list[str] = []
        for file_name, unicode_range in subsets:
            try:
                data = (fonts_dir / file_name).read_bytes()
            except OSError:
                # Файл шрифту відсутній або не читається — не падаємо,
                # просто не вбудовуємо шрифт у цей документ
                return ""
            b64 = base64.b64encode(data).decode("ascii")
            faces.append(
                "@font-face { font-family: 'Bad Script'; font-style: normal; "
                "font-weight: 400; font-display: swap; "
                f"src: url(data:font/woff2;base64,{b64}) format('woff2'); "
                f"unicode-range: {unicode_range}; }}"
            )
        return "\n".join(faces)

    @staticmethod
    async def get_font_family(session) -> str:
        """
        Отримує вибраний користувачем шрифт друку з налаштувань БД.

        Args:
            session: асинхронна SQLAlchemy-сесія.

        Returns:
            Назва шрифту (наприклад 'Arial, sans-serif' або 'Courier New').
            Якщо налаштування print_font_family не задано — повертає
            DEFAULT_FONT_FAMILY.
        """
        # Імпорт усередині методу, щоб уникнути потенційних циклічних
        # імпортів між шарами (infrastructure → application).
        from app.application.services.settings_service import SettingsService

        font = await SettingsService(session).get_string(
            "print_font_family",
            PrintFontService.DEFAULT_FONT_FAMILY,
        )
        # Фінальна валідація значення з БД: обрізаємо зайві пробіли,
        # а плейсхолдер 'custom' (користувач обрав «Інший», але не ввів
        # значення) та порожній рядок замінюємо на шрифт за замовчуванням.
        font = (font or "").strip()
        if not font or font.lower() == "custom":
            return PrintFontService.DEFAULT_FONT_FAMILY

        logger.info(
            "PRINT_FONT_GET | font_family=%s (default=%s)",
            font,
            PrintFontService.DEFAULT_FONT_FAMILY,
        )
        return font

    @staticmethod
    def apply_font_to_html(html: str, font_family: str | None) -> str:
        """
        Застосовує вибраний шрифт до всього HTML-документа друку.

        Кроки:
          1. Якщо html порожній або font_family порожній — повернути html
             без змін.
          2. Замінити ВСІ font-family у HTML (inline style + <style> блоки)
             на вибраний шрифт. Regex захоплює значення до ; " ' } —
             покриває inline style="font-family: X" і CSS-правила.
          3. Якщо після заміни 'font-family' ВЗАГАЛІ не зустрічається
             в html (lower) — вставити
             '<style>body { font-family: {font_family}; }</style>'
             перед '</head>', а якщо '</head>' немає — додати в кінець html.
          4. Повернути оновлений html.

        Args:
            html: HTML-документ, що йде на друк (чек, етикетка, цінник).
            font_family: назва шрифту (наприклад 'Courier New, monospace').

        Returns:
            Оновлений HTML-рядок.
        """
        if not html or not font_family:
            return html

        # Санітизація: дозволені тільки безпечні символи CSS font-family
        # (літери, цифри, пробіл, кома, дефіс, апостроф).
        # Лапки прибираємо — вони ламають inline style="font-family: ...".
        # < > ; { } ( ) = & / \ — блокують HTML/CSS-ін'єкції.
        safe_font = re.sub(r"[^A-Za-z0-9 ,'-]", "", font_family).strip()
        if not safe_font:
            safe_font = PrintFontService.DEFAULT_FONT_FAMILY
        font_family = safe_font

        # Google Font «Bad Script» — НЕ системний шрифт: його треба
        # вбудувати в HTML через @font-face з base64, інакше він не
        # відобразиться у попередньому перегляді (iframe) та при друці
        # (окреме вікно).
        font_face_css = ""
        if "bad script" in font_family.lower():
            font_face_css = PrintFontService._bad_script_face_css()

        # Замінюємо всі font-family на вибраний шрифт.
        # Використовуємо lambda-заміну, щоб значення font_family не
        # інтерпретувалось як спецсимволи regex-заміни ($, \\, тощо).
        updated = _FONT_FAMILY_RE.sub(
            lambda _m: f"font-family: {font_family}",
            html,
        )

        # Чи потрібне глобальне правило для body? Перевіряємо ДО вставки
        # @font-face — той містить font-family: 'Bad Script' і зіпсував би
        # перевірку наявності font-family у документі.
        need_body_style = "font-family" not in updated.lower()

        # Вбудовуємо @font-face на початок документа (або перед </head>),
        # щоб браузер завантажив шрифт ДО застосування font-family.
        # Вставка виконується ПІСЛЯ regex-заміни font-family — інакше
        # regex пошкодив би font-family: 'Bad Script' усередині @font-face,
        # замінивши його на список fallback.
        if font_face_css:
            face_style = f"<style>{font_face_css}</style>"
            head_end = updated.lower().find("</head>")
            updated = updated[:head_end] + face_style + updated[head_end:] if head_end != -1 else face_style + updated

        # Якщо font-family не зустрічається зовсім — додаємо глобальне
        # правило для body, щоб шрифт застосувався до всього документа.
        if need_body_style:
            style_tag = (
                f"<style>body {{ font-family: {font_family}; }}</style>"
            )
            head_end = updated.lower().find("</head>")
            updated = updated[:head_end] + style_tag + updated[head_end:] if head_end != -1 else updated + style_tag

        logger.info(
            "PRINT_FONT_APPLY | font_family=%s html_len=%d replaced=%d",
            font_family,
            len(updated),
            len(_FONT_FAMILY_RE.findall(html)),
        )
        return updated
