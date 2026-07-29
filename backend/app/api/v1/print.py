"""
API роутер для рендеру цінників та етикеток.

Ендпоінти:
  - POST /api/v1/print/price-tags/render  — рендер цінників на A4
  - POST /api/v1/print/labels/render      — рендер етикеток на термопринтер
  - POST /api/v1/print/test               — тестовий друк

Логіка:
  1. Отримуємо шаблон з БД за template_id
  2. Беремо налаштування полів з system_settings (price_tag_fields / label_fields)
  3. Для кожного товару фільтруємо поля згідно налаштувань
  4. Викликаємо відповідний метод сервісу з налаштуваннями штрих-коду
  5. Повертаємо HTML + мета-інформацію
"""

from __future__ import annotations

import json
import math
import logging

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.models.print_template import PrintTemplate
from app.models.system_setting import SystemSetting
from app.models.user import User
from app.schemas.print import (
    PriceTagRenderRequest,
    PriceTagRenderResponse,
    LabelRenderRequest,
    LabelRenderResponse,
)
from app.services.auth_service import AuthService
from app.services.price_tag_print_service import PriceTagPrintService

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
            "price": p.price,
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
            "price": p.price,
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
    }

    # ─── 5. Рендеримо HTML ──────────────────────────────────────────────────
    html = PriceTagPrintService.render_labels_sequential(
        template.content,
        products_dicts,
        settings,
    )

    # ─── 6. Обчислюємо мета-інформацію ──────────────────────────────────────
    total_labels = sum(p.copies for p in data.products)

    return LabelRenderResponse(
        html=html,
        total_labels=total_labels,
    )


# ─── Тестовий друк ────────────────────────────────────────────────────────────

class TestPrintRequest(BaseModel):
    """Запит на тестовий друк."""
    printer_name: str = ""
    template_type: str = "receipt_58mm"


class TestPrintResponse(BaseModel):
    """Відповідь на тестовий друк."""
    status: str
    message: str
    preview_html: str | None = None


@router.post("/test", response_model=TestPrintResponse)
async def test_print(
    data: TestPrintRequest,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Тестовий друк — генерує тестовий чек з демо-даними та повертає HTML.

    Якщо передано printer_name — спроба відправити на друк (в майбутньому).
    Зараз повертає HTML для попереднього перегляду.

    Тіло запиту:
    ```json
    {
      "printer_name": "EPSON TM-T20",
      "template_type": "receipt_58mm"
    }
    ```
    """
    from app.services.print_template_service import PrintTemplateService

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

    return TestPrintResponse(
        status="success",
        message=f"Тестовий чек згенеровано (шаблон: {template.name}, принтер: {data.printer_name or 'системний'})",
        preview_html=html,
    )
