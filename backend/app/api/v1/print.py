"""
API роутер для рендеру цінників та етикеток.

Ендпоінти:
  - POST /api/v1/print/price-tags/render  — рендер цінників на A4
  - POST /api/v1/print/labels/render      — рендер етикеток на термопринтер
  - POST /api/v1/print/test               — тестовий друк (чек / цінник / етикетка)

Логіка:
  1. Отримуємо шаблон з БД за template_id
  2. Беремо налаштування полів з system_settings (price_tag_fields / label_fields)
  3. Для кожного товару фільтруємо поля згідно налаштувань
  4. Викликаємо відповідний метод сервісу з налаштуваннями штрих-коду
  5. Повертаємо HTML + мета-інформацію
"""

from __future__ import annotations

import asyncio
import json
import math
import logging
import subprocess
from typing import Literal
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.infrastructure.persistence.models.print_template import PrintTemplate
from app.infrastructure.persistence.models.system_setting import SystemSetting
from app.infrastructure.persistence.models.user import User
from app.schemas.print import (
    PriceTagRenderRequest,
    PriceTagRenderResponse,
    LabelRenderRequest,
    LabelRenderResponse,
)
from app.domain.services.auth_service import AuthService
from app.infrastructure.services.price_tag_print_service import PriceTagPrintService
from app.infrastructure.services.print_template_service import PrintTemplateService
from app.infrastructure.services.print_font_service import PrintFontService

logger = logging.getLogger(__name__)

router = APIRouter(
    prefix="/print",
    tags=["Print (Price Tags & Labels)"],
)


# ─── Допоміжна функція: отримати налаштування полів ─────────────────────────

async def _get_fields_from_settings(
    session: AsyncSession,
    key_name: str,
    default_fields: list[str],
) -> list[str]:
    """
    Отримує список полів для показу з системних налаштувань.

    Args:
        session: асинхронна сесія БД
        key_name: ключ налаштування (price_tag_fields або label_fields)
        default_fields: поля за замовчуванням

    Returns:
        Список рядків з назвами полів
    """
    result = await session.execute(
        select(SystemSetting).where(
            SystemSetting.key == key_name,
            SystemSetting.is_active == True,
        )
    )
    setting = result.scalar_one_or_none()

    if setting and setting.value:
        try:
            fields = json.loads(setting.value)
            if isinstance(fields, list) and len(fields) > 0:
                return fields
        except (json.JSONDecodeError, TypeError):
            logger.warning(f"Не вдалося розпарсити налаштування {key_name}: {setting.value}")

    return default_fields


# ─── Допоміжна функція: нормалізація ціни ────────────────────────────────────

def _format_price(price) -> str:
    """
    Нормалізує ціну товару до рядка з двома десятковими знаками.

    Використовується при формуванні products_dicts для друку цінників/етикеток,
    щоб у шаблоні "{{price}} грн" виводилось "25.00 грн", а не "25.0 грн".

    Args:
        price: ціна товару (float, str або None).

    Returns:
        Рядок виду "25.00", "45.50". Якщо price None або нечисловий — "0.00".
    """
    if price is None or price == "":
        return "0.00"
    try:
        return f"{float(price):.2f}"
    except (ValueError, TypeError):
        return "0.00"


# ─── Допоміжна функція: демо-товари для тестового друку ──────────────────────

def _build_demo_products() -> list[dict]:
    """
    Формує демо-товари для тестового друку цінників/етикеток.

    Returns:
        Список словників з даними товарів (id, title, price, barcode, ...).
    """
    return [
        {
            "id": "00000000-0000-0000-0000-000000000001",
            "title": "Тестовий товар №1",
            "price": _format_price("25.00"),
            "barcode": "4820012345678",
            "article": "ТЕСТ-001",
            "category": "Тестова категорія",
            "copies": 1,
        },
        {
            "id": "00000000-0000-0000-0000-000000000002",
            "title": "Тестовий товар №2",
            "price": _format_price("45.50"),
            "barcode": "4820012345679",
            "article": "ТЕСТ-002",
            "category": "Тестова категорія",
            "copies": 1,
        },
    ]


# ─── ЕНДПОІНТ: Рендер цінників на A4 ─────────────────────────────────────────

@router.post("/price-tags/render", response_model=PriceTagRenderResponse)
async def render_price_tags(
    data: PriceTagRenderRequest,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Рендерить цінники на A4 у вигляді HTML-сітки.

    Тіло запиту:
    ```json
    {
      "template_id": "uuid",
      "products": [
        {
          "id": "uuid",
          "title": "Хліб білий",
          "price": "25.00",
          "barcode": "4820012345678",
          "article": "ХЛ-001",
          "category": "Хлібобулочні",
          "copies": 2
        }
      ],
      "width_mm": 40,
      "height_mm": 25,
      "gap_mm": 3,
      "margin_mm": 10,
      "barcode_type": "code128",
      "barcode_height_mm": 12
    }
    ```

    Повертає готовий до друку HTML + кількість сторінок та цінників.
    """
    # ─── 1. Отримуємо шаблон з БД ──────────────────────────────────────────
    result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == data.template_id)
    )
    template = result.scalar_one_or_none()

    if not template or not template.is_active:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{data.template_id}' не знайдено або він неактивний",
        )

    # ─── 2. Отримуємо налаштування полів ────────────────────────────────────
    fields = await _get_fields_from_settings(
        session,
        "price_tag_fields",
        ["title", "price", "barcode"],
    )

    # ─── 3. Перетворюємо товари ─────────────────────────────────────────────
    products_dicts = []
    for p in data.products:
        products_dicts.append({
            "id": str(p.id),
            "title": p.title,
            "price": _format_price(p.price),
            "barcode": p.barcode or "",
            "article": p.article or "",
            "category": p.category or "",
            "copies": p.copies,
        })

    # ─── 4. Формуємо налаштування для сервісу ───────────────────────────────
    settings = {
        "width_mm": data.width_mm,
        "height_mm": data.height_mm,
        "gap_mm": data.gap_mm,
        "margin_mm": data.margin_mm,
        "page_width_mm": 210,   # A4
        "page_height_mm": 297,  # A4
        "fields": fields,
        "barcode_type": data.barcode_type,
        "barcode_height_mm": data.barcode_height_mm,
    }

    # ─── 5. Рендеримо HTML ──────────────────────────────────────────────────
    html = PriceTagPrintService.render_price_tags_grid(
        template.content,
        products_dicts,
        settings,
    )

    # ─── 5а. Застосовуємо вибраний шрифт (налаштування print_font_family) ───
    # Шрифт читається з БД один раз на запит і застосовується до всього HTML,
    # що йде на друк (inline style + <style> блоки).
    font = await PrintFontService.get_font_family(session)
    html = PrintFontService.apply_font_to_html(html, font)

    # ─── 6. Обчислюємо мета-інформацію ──────────────────────────────────────
    total_labels = sum(p.copies for p in data.products)

    cols, rows, per_page = PriceTagPrintService._calc_grid(
        data.width_mm,
        data.height_mm,
        data.gap_mm,
        210,  # A4 ширина
        297,  # A4 висота
        data.margin_mm,
    )
    total_pages = max(1, math.ceil(total_labels / per_page)) if per_page > 0 else 1

    return PriceTagRenderResponse(
        html=html,
        total_pages=total_pages,
        total_labels=total_labels,
    )


# ─── ЕНДПОІНТ: Рендер етикеток на термопринтер ─────────────────────────────

@router.post("/labels/render", response_model=LabelRenderResponse)
async def render_labels(
    data: LabelRenderRequest,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Рендерить етикетки для термопринтера — одна за одною.

    Тіло запиту:
    ```json
    {
      "template_id": "uuid",
      "products": [
        {
          "id": "uuid",
          "title": "Хліб білий",
          "price": "25.00",
          "barcode": "4820012345678",
          "article": "ХЛ-001",
          "category": "Хлібобулочні",
          "copies": 2
        }
      ],
      "width_mm": 58,
      "height_mm": 40,
      "gap_mm": 2,
      "barcode_type": "code128",
      "barcode_height_mm": 12
    }
    ```

    Повертає готовий до друку HTML + кількість етикеток.
    """
    # ─── 1. Отримуємо шаблон з БД ──────────────────────────────────────────
    result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == data.template_id)
    )
    template = result.scalar_one_or_none()

    if not template or not template.is_active:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{data.template_id}' не знайдено або він неактивний",
        )

    # ─── 2. Отримуємо налаштування полів ────────────────────────────────────
    fields = await _get_fields_from_settings(
        session,
        "label_fields",
        ["title", "price", "barcode"],
    )

    # ─── 3. Перетворюємо товари ─────────────────────────────────────────────
    products_dicts = []
    for p in data.products:
        products_dicts.append({
            "id": str(p.id),
            "title": p.title,
            "price": _format_price(p.price),
            "barcode": p.barcode or "",
            "article": p.article or "",
            "category": p.category or "",
            "copies": p.copies,
        })

    # ─── 4. Формуємо налаштування для сервісу ───────────────────────────────
    settings = {
        "width_mm": data.width_mm,
        "height_mm": data.height_mm,
        "gap_mm": data.gap_mm,
        "fields": fields,
        "barcode_type": data.barcode_type,
        "barcode_height_mm": data.barcode_height_mm,
        "print_mode": data.print_mode,
    }

    # ─── 5. Рендеримо HTML ──────────────────────────────────────────────────
    html = PriceTagPrintService.render_labels_sequential(
        template.content,
        products_dicts,
        settings,
    )

    # ─── 5а. Застосовуємо вибраний шрифт (налаштування print_font_family) ───
    font = await PrintFontService.get_font_family(session)
    html = PrintFontService.apply_font_to_html(html, font)

    # ─── 6. Обчислюємо мета-інформацію ──────────────────────────────────────
    total_labels = sum(p.copies for p in data.products)

    return LabelRenderResponse(
        html=html,
        total_labels=total_labels,
    )


# ─── Тестовий друк ────────────────────────────────────────────────────────────

class TestPrintRequest(BaseModel):
    """
    Запит на тестовий друк.

    Підтримує три типи друку:
    - receipt    — тестовий чек (зворотна сумісність)
    - price_tag  — тестовий цінник на A4
    - label      — тестова етикетка на термопринтер
    """
    print_type: Literal["receipt", "price_tag", "label"] = Field(
        "receipt",
        description="Тип друку: receipt (чек), price_tag (цінник A4), label (етикетка)",
    )
    printer_name: str = Field(
        "", description="Назва принтера (для повідомлення; друк поки що не виконується)"
    )
    template_type: str = Field(
        "receipt_58mm",
        description="Тип шаблону для чека: receipt_58mm, receipt_80mm, return_receipt_58mm, fiscal, custom",
    )
    # ── Параметри для price_tag / label ─────────────────────────────────────
    template_id: UUID | None = Field(
        None,
        description="ID шаблону цінника/етикетки. Якщо не передано — береться з налаштувань або за замовчуванням",
    )
    width_mm: float | None = Field(
        None, ge=10, le=200,
        description="Ширина цінника/етикетки в мм (за замовчуванням: 40 для цінника, 58 для етикетки)",
    )
    height_mm: float | None = Field(
        None, ge=10, le=200,
        description="Висота цінника/етикетки в мм (за замовчуванням: 25 для цінника, 40 для етикетки)",
    )
    gap_mm: float | None = Field(
        None, ge=0, le=20,
        description="Проміжок між цінниками/етикетками в мм (за замовчуванням: 3)",
    )
    margin_mm: float | None = Field(
        None, ge=0, le=50,
        description="Поля сторінки в мм, тільки для price_tag (за замовчуванням: 10)",
    )
    barcode_type: Literal["code128", "qr"] = Field(
        "code128",
        description="Тип кодування штрих-коду: code128 (лінійний) або qr",
    )
    barcode_height_mm: float = Field(
        12, ge=4, le=40,
        description="Висота штрих-коду в мм (для code128) або розмір (для QR)",
    )


class TestPrintResponse(BaseModel):
    """Відповідь на тестовий друк."""
    status: str
    message: str
    preview_html: str | None = None
    template_name: str | None = Field(
        None, description="Назва використаного шаблону"
    )


# ─── Допоміжна функція: тестовий друк цінника/етикетки ───────────────────────

async def _test_print_price_tag_or_label(
    data: TestPrintRequest,
    session: AsyncSession,
) -> TestPrintResponse:
    """
    Генерує тестовий цінник (A4) або етикетку (термопринтер).

    Args:
        data: запит на тестовий друк.
        session: асинхронна сесія БД.

    Returns:
        TestPrintResponse з preview_html та template_name.

    Raises:
        HTTPException 404: Якщо шаблон не знайдено.
    """
    is_price_tag = data.print_type == "price_tag"
    template_type = "price_tag" if is_price_tag else "label"

    # ─── 1. Отримуємо шаблон ────────────────────────────────────────────────
    template = None

    # 1a. Якщо template_id передано — використовуємо його
    if data.template_id:
        result = await session.execute(
            select(PrintTemplate).where(PrintTemplate.id == data.template_id)
        )
        template = result.scalar_one_or_none()

        if not template or not template.is_active:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Шаблон з ID '{data.template_id}' не знайдено або він неактивний",
            )

    # 1b. Інакше — пробуємо взяти з налаштувань (price_tag_template_id / label_template_id)
    if template is None:
        setting_key = "price_tag_template_id" if is_price_tag else "label_template_id"
        result = await session.execute(
            select(SystemSetting).where(SystemSetting.key == setting_key)
        )
        setting = result.scalar_one_or_none()
        if setting and setting.value:
            try:
                template_id = UUID(setting.value.strip())
                result = await session.execute(
                    select(PrintTemplate).where(PrintTemplate.id == template_id)
                )
                candidate = result.scalar_one_or_none()
                if candidate and candidate.is_active:
                    template = candidate
            except ValueError:
                logger.warning(f"Невірний UUID у налаштуванні {setting_key}: {setting.value}")

    # 1c. Інакше — шаблон за замовчуванням для типу
    if template is None:
        service = PrintTemplateService(session)
        template = await service.get_default_for_type(template_type)

        if not template:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=(
                    f"Не знайдено шаблону для типу '{template_type}'. "
                    f"Створіть шаблон у розділі «Шаблони друку» або передайте template_id."
                ),
            )

    # ─── 2. Отримуємо налаштування полів ────────────────────────────────────
    fields_key = "price_tag_fields" if is_price_tag else "label_fields"
    fields = await _get_fields_from_settings(
        session,
        fields_key,
        ["title", "price", "barcode"],
    )

    # ─── 3. Демо-товари ──────────────────────────────────────────────────────
    products = _build_demo_products()

    # ─── 4. Формуємо налаштування для сервісу ────────────────────────────────
    # Розміри за замовчуванням залежать від типу друку
    if is_price_tag:
        width_mm = data.width_mm if data.width_mm is not None else 40
        height_mm = data.height_mm if data.height_mm is not None else 25
        gap_mm = data.gap_mm if data.gap_mm is not None else 3
        margin_mm = data.margin_mm if data.margin_mm is not None else 10
    else:
        width_mm = data.width_mm if data.width_mm is not None else 58
        height_mm = data.height_mm if data.height_mm is not None else 40
        gap_mm = data.gap_mm if data.gap_mm is not None else 3
        margin_mm = data.margin_mm if data.margin_mm is not None else 0

    settings = {
        "width_mm": width_mm,
        "height_mm": height_mm,
        "gap_mm": gap_mm,
        "margin_mm": margin_mm,
        "page_width_mm": 210,   # A4 (тільки для price_tag)
        "page_height_mm": 297,  # A4 (тільки для price_tag)
        "fields": fields,
        "barcode_type": data.barcode_type,
        "barcode_height_mm": data.barcode_height_mm,
        # Тестовий друк — на системному принтері (CUPS): повна ширина.
        # render_price_tags_grid (A4) ігнорує print_mode.
        "print_mode": "system",
    }

    # ─── 5. Рендеримо HTML ───────────────────────────────────────────────────
    if is_price_tag:
        html = PriceTagPrintService.render_price_tags_grid(
            template.content,
            products,
            settings,
        )
        label_word = "цінник"
    else:
        html = PriceTagPrintService.render_labels_sequential(
            template.content,
            products,
            settings,
        )
        label_word = "етикетка"

    # ─── 6. Застосовуємо вибраний шрифт (налаштування print_font_family) ───
    # Покриває обидві гілки тестового друку: цінники (render_price_tags_grid)
    # та етикетки (render_labels_sequential).
    font = await PrintFontService.get_font_family(session)
    html = PrintFontService.apply_font_to_html(html, font)

    return TestPrintResponse(
        status="success",
        message=(
            f"Тестовий {label_word} згенеровано "
            f"(шаблон: {template.name}, принтер: {data.printer_name or 'системний'})"
        ),
        preview_html=html,
        template_name=template.name,
    )


# ─── ЕНДПОІНТ: Список принтерів (CUPS) ──────────────────────────────────────────


@router.get("/printers")
async def list_printers():
    """
    Список доступних принтерів (CUPS).

    Виконує системну команду `lpstat -e` у фоновому потоці
    (run_in_executor), щоб не блокувати event loop. Якщо lpstat
    недоступний або сталася помилка — повертає порожній список
    зі статусом 200 (бекенд не падає).

    Returns:
        {"printers": ["PrinterName1", "PrinterName2", ...]}
    """
    try:
        loop = asyncio.get_running_loop()
        printers = await loop.run_in_executor(None, _list_printers_sync)
        return {"printers": printers}
    except Exception:  # noqa: BLE001
        logger.exception("Помилка отримання списку принтерів (lpstat)")
        return {"printers": []}


def _list_printers_sync() -> list[str]:
    """
    Синхронне отримання списку принтерів через `lpstat -e` (CUPS).

    Returns:
        Список назв доступних принтерів; порожній — якщо CUPS
        недоступний або команда завершилась помилкою.
    """
    try:
        result = subprocess.run(
            ["lpstat", "-e"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        if result.returncode != 0:
            return []
        return [
            line.strip()
            for line in result.stdout.splitlines()
            if line.strip()
        ]
    except Exception:  # noqa: BLE001
        return []


# ─── ЕНДПОІНТ: Тестовий друк ─────────────────────────────────────────────────

@router.post("/test", response_model=TestPrintResponse)
async def test_print(
    data: TestPrintRequest,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Тестовий друк — генерує тестовий документ та повертає HTML для перегляду.

    Підтримує три типи друку (print_type):
    - "receipt"    (за замовчуванням) — тестовий чек з демо-даними;
      використовує template_type (receipt_58mm, receipt_80mm, ...) та printer_name.
    - "price_tag"  — тестовий цінник на A4; використовує PriceTagPrintService
      з параметрами template_id/width_mm/height_mm/gap_mm/margin_mm/barcode_type.
    - "label"      — тестова етикетка на термопринтер; ті самі параметри,
      але без margin_mm (поля сторінки).

    Приклад для чека (зворотна сумісність):
    ```json
    {
      "printer_name": "EPSON TM-T20",
      "template_type": "receipt_58mm"
    }
    ```

    Приклад для цінника:
    ```json
    {
      "print_type": "price_tag",
      "width_mm": 40,
      "height_mm": 25,
      "barcode_type": "code128"
    }
    ```

    Якщо template_id не передано — шаблон береться з налаштувань
    (price_tag_template_id / label_template_id) або за замовчуванням для типу.
    """
    # ─── Тест цінника / етикетки ─────────────────────────────────────────────
    if data.print_type in ("price_tag", "label"):
        return await _test_print_price_tag_or_label(data, session)

    # ─── Тестовий чек (зворотна сумісність) ──────────────────────────────────
    # 1. Отримуємо дефолтний шаблон для вказаного типу
    service = PrintTemplateService(session)
    template = await service.get_default_for_type(data.template_type)

    if not template:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Не знайдено шаблону для типу '{data.template_type}'",
        )

    # 2. Демо-дані для тестового чеку
    test_data = {
        "shop_name": "Мій магазин (ТЕСТ)",
        "shop_address": "вул. Тестова, 1",
        "tax_id": "12345678",
        "receipt_number": "TEST-001",
        "date": "29.07.2026",
        "time": "12:00",
        "cashier": "Тестовий Касир",
        "items": (
            '<div style="margin-bottom:4px;">'
            '<div style="display:flex;justify-content:space-between;">'
            '<span>Тестовий товар №1</span><span>25.00</span>'
            '</div>'
            '<div style="display:flex;justify-content:space-between;font-size:10px;color:#666;">'
            '<span>1 × 25.00</span><span style="font-weight:bold;">25.00</span>'
            '</div>'
            '</div>'
            '<div style="margin-bottom:4px;">'
            '<div style="display:flex;justify-content:space-between;">'
            '<span>Тестовий товар №2</span><span>45.50</span>'
            '</div>'
            '<div style="display:flex;justify-content:space-between;font-size:10px;color:#666;">'
            '<span>2 × 22.75</span><span style="font-weight:bold;">45.50</span>'
            '</div>'
            '</div>'
        ),
        "total": "70.50",
        "payment_method": "Готівка",
        "paid": "100.00",
        "change": "29.50",
        "footer": "Дякуємо за покупку!",
    }

    # 3. Рендеримо HTML
    html = PrintTemplateService.render_template(template.content, test_data)

    # Застосовуємо вибраний шрифт (налаштування print_font_family) до чеку
    font = await PrintFontService.get_font_family(session)
    html = PrintFontService.apply_font_to_html(html, font)

    return TestPrintResponse(
        status="success",
        message=f"Тестовий чек згенеровано (шаблон: {template.name}, принтер: {data.printer_name or 'системний'})",
        preview_html=html,
        template_name=template.name,
    )
