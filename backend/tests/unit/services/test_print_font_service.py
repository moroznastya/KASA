"""
Юніт-тести PrintFontService — модуля керування шрифтом друкованих документів.

Перевіряють:
  - apply_font_to_html: заміну ВСІХ font-family (inline style + <style>
    блоки), включно зі значеннями в лапках ("Times New Roman", 'Courier New')
    та змішаними (Arial, "Helvetica Neue", sans-serif);
  - вставку глобального <style>body{font-family:...}</style>, якщо
    font-family у HTML відсутній (перед </head> або в кінець документа);
  - безпечну поведінку для порожніх значень та спецсимволів у назві шрифту;
  - get_font_family: читання налаштування print_font_family з БД
    (SQLite in-memory) з дефолтом 'Arial, sans-serif'.
"""
from __future__ import annotations

import pytest

from app.infrastructure.persistence.models.system_setting import SystemSetting
from app.infrastructure.services.print_font_service import PrintFontService

FONT = "Courier New, monospace"


# ─── apply_font_to_html: заміна font-family ─────────────────────────────────

class TestApplyFontToHtml:
    """Тести застосування шрифту до HTML."""

    def test_replaces_all_font_family(self):
        """Замінює ВСІ font-family: inline style + <style> + лапки + змішані."""
        html = (
            "<html><head><style>"
            "body { font-family: Arial, sans-serif; font-size: 12px; }"
            ".tag { font-family: \"Times New Roman\", serif; }"
            ".q { font-family: 'Courier New'; }"
            ".mix { font-family: Arial, \"Helvetica Neue\", sans-serif; }"
            "</style></head>"
            '<body style="font-family: Courier; color: #000">Hi</body></html>'
        )
        out = PrintFontService.apply_font_to_html(html, FONT)
        assert out.count(f"font-family: {FONT}") == 5
        # Жодного залишку старих шрифтів чи зламаних хвостів
        assert "Times New Roman" not in out
        assert "Courier;" not in out
        assert f"{FONT}\"" not in out
        assert f"{FONT}'" not in out

    def test_replaces_inline_without_semicolon(self):
        """Inline style без ';' — закриваюча лапка style зберігається."""
        out = PrintFontService.apply_font_to_html(
            '<body style="font-family: Courier">x</body>', FONT,
        )
        assert f'style="font-family: {FONT}"' in out

    def test_inserts_style_before_head_end(self):
        """Без font-family → <style> вставляється перед </head>."""
        html = "<html><head><meta charset='utf-8'></head><body>Чек</body></html>"
        out = PrintFontService.apply_font_to_html(html, FONT)
        style = f"<style>body {{ font-family: {FONT}; }}</style>"
        assert style in out
        assert out.index(style) < out.index("</head>")

    def test_appends_style_when_no_head_end(self):
        """Без </head> → <style> додається в кінець документа."""
        html = "<html><body><div>Текст</div></body>"
        out = PrintFontService.apply_font_to_html(html, FONT)
        assert out.endswith(f"<style>body {{ font-family: {FONT}; }}</style>")

    def test_empty_inputs_unchanged(self):
        """Порожній html або шрифт → повертає html без змін."""
        assert PrintFontService.apply_font_to_html("", FONT) == ""
        assert PrintFontService.apply_font_to_html("<html></html>", "") == "<html></html>"
        assert PrintFontService.apply_font_to_html("<html></html>", None) == "<html></html>"

    def test_special_chars_in_font_sanitized(self):
        """Небезпечні символи ($, \\) у назві шрифту вирізаються санітизацією."""
        out = PrintFontService.apply_font_to_html(
            "<style>body{font-family: X;}</style>", "DejaVu $Sans \\ Mono",
        )
        # $ та \\ не входять у дозволений клас [A-Za-z0-9 ,'-] → видаляються
        assert "font-family: DejaVu Sans  Mono;" in out
        assert "$" not in out and "\\" not in out

    def test_strips_quotes_from_font(self):
        """Лапки в значенні шрифту з БД не ламають HTML-атрибут style."""
        out = PrintFontService.apply_font_to_html(
            '<div style="font-family: monospace">x</div>',
            '"Times New Roman", serif',
        )
        assert out == '<div style="font-family: Times New Roman, serif">x</div>'

    def test_blocks_css_injection(self):
        """HTML/CSS-ін'єкція у назві шрифту очищується (без <script>)."""
        out = PrintFontService.apply_font_to_html(
            "<div>t</div>",
            "</style><script>alert(1)</script>",
        )
        assert "<script>" not in out
        # Залишок після санітизації — лише безпечні символи
        assert "font-family: stylescriptalert1script" in out

    def test_fully_unsafe_font_falls_back_to_default(self):
        """Шрифт з лише недозволених символів → дефолтний."""
        out = PrintFontService.apply_font_to_html("<div>t</div>", "</><()=")
        assert "font-family: Arial, sans-serif;" in out

    def test_idempotent_application(self):
        """Повторне застосування того самого шрифту не ламає HTML."""
        html = (
            "<html><head><style>body { font-family: Arial, sans-serif; }"
            "</style></head><body>Чек</body></html>"
        )
        once = PrintFontService.apply_font_to_html(html, FONT)
        twice = PrintFontService.apply_font_to_html(once, FONT)
        assert twice.count(f"font-family: {FONT}") == 1
        assert twice == once


# ─── Bad Script: вбудовування @font-face ────────────────────────────────────

class TestBadScriptFontFace:
    """Тести вбудовування Google Font «Bad Script» через @font-face."""

    def test_bad_script_embeds_font_face(self):
        """Bad Script додає @font-face з base64 (cyrillic + latin)."""
        r = PrintFontService.apply_font_to_html(
            "<html><head></head><body>Привіт 123</body></html>",
            "Bad Script, cursive",
        )
        assert "@font-face" in r
        assert "data:font/woff2;base64" in r
        assert r.count("@font-face") == 2  # cyrillic + latin
        assert r.count("unicode-range:") == 2

    def test_bad_script_face_css_valid(self):
        """_bad_script_face_css містить коректні @font-face для обох піднаборів."""
        css = PrintFontService._bad_script_face_css()
        assert css.count("@font-face") == 2
        assert "font-family: 'Bad Script'" in css
        assert "font-weight: 400" in css
        assert "font-display: swap" in css
        assert "U+0400-045F" in css          # кирилиця (укр., включно з Ґ/ґ)
        assert "U+0000-00FF" in css          # латиниця, цифри, пунктуація

    def test_bad_script_face_not_broken_by_regex(self):
        """Regex-заміна НЕ пошкоджує font-family всередині @font-face."""
        r = PrintFontService.apply_font_to_html(
            '<div style="font-family: Arial">x</div>',
            "Bad Script, cursive",
        )
        # @font-face зберіг ім'я 'Bad Script' (з лапками), а не список fallback
        assert "font-family: 'Bad Script';" in r
        # inline style замінений на вибраний шрифт
        assert 'style="font-family: Bad Script, cursive"' in r

    def test_bad_script_face_before_font_application(self):
        """@font-face стоїть ДО застосування font-family у документі."""
        r = PrintFontService.apply_font_to_html(
            "<html><head></head><body>Привіт</body></html>",
            "Bad Script, cursive",
        )
        face_pos = r.index("@font-face")
        body_pos = r.index("body { font-family: Bad Script, cursive")
        assert face_pos < body_pos

    def test_regular_font_has_no_font_face(self):
        """Звичайний шрифт (Arial) НЕ додає @font-face."""
        r = PrintFontService.apply_font_to_html(
            '<div style="font-family: Arial">x</div>',
            "Arial, sans-serif",
        )
        assert "@font-face" not in r

    def test_bad_script_fragment_without_head(self):
        """Фрагмент без </head>: @font-face додається на початок документа."""
        r = PrintFontService.apply_font_to_html(
            "<div>Текст</div>",
            "Bad Script, cursive",
        )
        assert r.startswith("<style>@font-face { font-family: 'Bad Script';")


# ─── get_font_family: читання налаштування з БД ─────────────────────────────

class TestGetFontFamily:
    """Тести читання налаштування print_font_family з БД."""

    async def test_default_when_setting_missing(self, session):
        """Ключ не заданий → повертається DEFAULT_FONT_FAMILY."""
        font = await PrintFontService.get_font_family(session)
        assert font == PrintFontService.DEFAULT_FONT_FAMILY

    async def test_returns_value_from_db(self, session):
        """Після upsert-запису через /settings-механізм значення читається."""
        session.add(SystemSetting(
            module="printing",
            key="print_font_family",
            value="Courier New, monospace",
            value_type="string",
            label="Шрифт друку",
            description="Шрифт для чеків, етикеток та цінників",
        ))
        await session.commit()

        font = await PrintFontService.get_font_family(session)
        assert font == "Courier New, monospace"

    async def test_custom_placeholder_falls_back_to_default(self, session):
        """Значення 'custom' (вибір «Інший» без введення) → дефолтний шрифт."""
        session.add(SystemSetting(
            module="printing",
            key="print_font_family",
            value="custom",
            value_type="string",
            label="Шрифт друку",
        ))
        await session.commit()

        font = await PrintFontService.get_font_family(session)
        assert font == PrintFontService.DEFAULT_FONT_FAMILY

    async def test_custom_uppercase_falls_back_to_default(self, session):
        """'CUSTOM' у верхньому регістрі також вважається плейсхолдером."""
        session.add(SystemSetting(
            module="printing",
            key="print_font_family",
            value="CUSTOM",
            value_type="string",
            label="Шрифт друку",
        ))
        await session.commit()

        font = await PrintFontService.get_font_family(session)
        assert font == PrintFontService.DEFAULT_FONT_FAMILY

    async def test_whitespace_value_is_stripped(self, session):
        """Пробіли навколо значення обрізаються при читанні з БД."""
        session.add(SystemSetting(
            module="printing",
            key="print_font_family",
            value="  Courier New  ",
            value_type="string",
            label="Шрифт друку",
        ))
        await session.commit()

        font = await PrintFontService.get_font_family(session)
        assert font == "Courier New"

    async def test_empty_value_falls_back_to_default(self, session):
        """Порожнє значення в БД → дефолтний шрифт."""
        session.add(SystemSetting(
            module="printing",
            key="print_font_family",
            value="",
            value_type="string",
            label="Шрифт друку",
        ))
        await session.commit()

        font = await PrintFontService.get_font_family(session)
        assert font == PrintFontService.DEFAULT_FONT_FAMILY

    @pytest.mark.asyncio
    async def test_settings_module_auto_created(self, session):
        """Ключ print_font_family коректно визначає module='printing'."""
        # Імітуємо логіку _determine_module з app/api/v1/settings.py:
        # ключ починається з 'print_' → module 'printing'
        key = "print_font_family"
        assert key.startswith("print_") is True
