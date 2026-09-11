"""
Use Case: InvoicePrintUseCases (друк цінників/етикеток з накладної).

Виносить бізнес-логіку друку з API-шару (v1/invoices.py) в Application Layer.
Працює з per-request сесією БД (AsyncSession), оскільки друк потребує
доступу до PrintTemplate, SystemSetting та Invoice з позиціями.

Використання у роутері:
    POST /api/v2/invoices/{invoice_id}/print-items
"""

from __future__ import annotations

import logging
import math
from datetime import UTC, datetime
from decimal import Decimal
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.infrastructure.persistence.models.invoice import (
    Invoice,
    InvoiceItem,
    InvoiceStatus,
)
from app.infrastructure.persistence.models.print_template import PrintTemplate
from app.infrastructure.persistence.models.system_setting import SystemSetting
from app.infrastructure.services.price_tag_print_service import PriceTagPrintService

logger = logging.getLogger(__name__)


class InvoicePrintUseCases:
    """Use Case для друку цінників/етикеток з прибуткової накладної."""

    def __init__(self, session: AsyncSession):
        """
        Ініціалізація.

        Args:
            session: Асинхронна сесія БД (per-request).
        """
        self._session = session

    async def _get_fields_from_settings(
        self,
        key_name: str,
        default_fields: list[str],
    ) -> list[str]:
        """Отримує список полів для показу з системних налаштувань."""
        result = await self._session.execute(
            select(SystemSetting).where(
                SystemSetting.key == key_name,
                SystemSetting.is_active == True,  # noqa: E712
            )
        )
        setting = result.scalar_one_or_none()

        if setting and setting.value:
            try:
                import json

                parsed = json.loads(setting.value)
                if isinstance(parsed, list) and parsed:
                    return [str(f) for f in parsed]
            except (ValueError, TypeError):
                logger.warning(
                    "Налаштування '%s' не є JSON-списком, використано за замовчуванням",
                    key_name,
                )
        return default_fields

    async def render_invoice_print_items(
        self,
        invoice_id: UUID,
        print_type: str,
        only_changed: bool,
        template_id: UUID,
        width_mm: float,
        height_mm: float,
        gap_mm: float,
        margin_mm: float,
        barcode_type: str,
        barcode_height_mm: float,
        print_mode: str = "system",
    ) -> dict:
        """
        Рендерить цінники або етикетки для товарів з накладної.

        Args:
            invoice_id: ID накладної (тільки підтвердженої).
            print_type: "price_tag" (A4) або "label" (термопринтер).
            only_changed: друкувати лише товари зі зміненою ціною.
            template_id: ID шаблону друку.
            width_mm / height_mm / gap_mm / margin_mm: розміри.
            barcode_type: "code128" або "qr".
            barcode_height_mm: висота штрих-коду в мм.

        Returns:
            dict: {html, total_labels, total_pages, changed_count, total_count}.

        Raises:
            ValueError: з детальним описом помилки (404/400).
        """
        # ─── 1. Завантажуємо накладну з товарами ──────────────────────────
        result = await self._session.execute(
            select(Invoice)
            .options(
                selectinload(Invoice.items).selectinload(InvoiceItem.product),
                selectinload(Invoice.supplier),
            )
            .where(Invoice.id == invoice_id)
        )
        invoice = result.scalar_one_or_none()

        if not invoice:
            raise ValueError(f"Накладну з ID '{invoice_id}' не знайдено")

        # Друкувати можна тільки з підтверджених накладних
        if invoice.status != InvoiceStatus.CONFIRMED:
            raise ValueError(
                "Друк цінників/етикеток можливий тільки для підтверджених "
                f"накладних. Поточний статус: {invoice.status.value}"
            )

        if not invoice.items:
            raise ValueError("Накладна не містить товарів")

        # ─── 2. Отримуємо шаблон з БД ─────────────────────────────────────
        tmpl_result = await self._session.execute(
            select(PrintTemplate).where(PrintTemplate.id == template_id)
        )
        template = tmpl_result.scalar_one_or_none()

        if not template or not template.is_active:
            raise ValueError(
                f"Шаблон з ID '{template_id}' не знайдено або він неактивний"
            )

        # ─── 3. Налаштування полів ────────────────────────────────────────
        fields_key = "price_tag_fields" if print_type == "price_tag" else "label_fields"
        fields = await self._get_fields_from_settings(
            fields_key,
            ["title", "price", "barcode"],
        )

        # ─── 4. Список товарів для друку та зміни цін ─────────────────────
        products_dicts: list[dict] = []
        price_changes: list[dict] = []
        changed_count = 0
        now_str = datetime.now(UTC).strftime("%d.%m.%Y")

        for item in invoice.items:
            product = item.product
            if not product:
                logger.warning(
                    "Товар з ID '%s' не знайдено в накладній '%s'",
                    item.product_id,
                    invoice_id,
                )
                continue

            invoice_price = item.price or 0
            prev_price = item.previous_price or product.price or 0
            current_price = product.price or 0

            prev_price_dec = Decimal(str(prev_price)).quantize(Decimal("0.01"))
            invoice_price_dec = Decimal(str(invoice_price)).quantize(Decimal("0.01"))
            current_price_dec = Decimal(str(current_price)).quantize(Decimal("0.01"))
            difference = (prev_price_dec - invoice_price_dec).quantize(Decimal("0.01"))
            changed = difference != Decimal("0.00")

            price_changes.append({
                "product_id": str(product.id),
                "title": product.title,
                "barcode": product.barcode or "",
                "article": product.sku or "",
                "invoice_price": str(invoice_price_dec),
                "current_price": str(current_price_dec),
                "changed": changed,
                "difference": str(difference),
            })

            if changed:
                changed_count += 1

            products_dicts.append({
                "id": str(product.id),
                "title": product.title,
                "price": str(current_price_dec),
                "barcode": product.barcode or "",
                "article": product.sku or "",
                "category": "",
                "copies": 1,
                "created_date": now_str,
            })

        if not products_dicts:
            raise ValueError("Не знайдено товарів для друку")

        # ─── 5. Фільтр тільки змінених цін ────────────────────────────────
        if only_changed:
            filtered = [
                p for i, p in enumerate(products_dicts)
                if price_changes[i]["changed"]
            ]
            products_dicts = filtered
            if not products_dicts:
                return {
                    "html": "",
                    "total_labels": 0,
                    "total_pages": 0,
                    "changed_count": 0,
                    "total_count": len(invoice.items),
                }

        # ─── 6. Налаштування для сервісу друку ────────────────────────────
        total_items = len(invoice.items)
        settings = {
            "width_mm": width_mm,
            "height_mm": height_mm,
            "gap_mm": gap_mm,
            "fields": fields,
            "barcode_type": barcode_type,
            "barcode_height_mm": barcode_height_mm,
            "print_mode": print_mode,
        }

        if print_type == "price_tag":
            settings["margin_mm"] = margin_mm
            settings["page_width_mm"] = 210   # A4
            settings["page_height_mm"] = 297  # A4

        # ─── 7. Рендеримо HTML ────────────────────────────────────────────
        if print_type == "price_tag":
            html = PriceTagPrintService.render_price_tags_grid(
                template.content,
                products_dicts,
                settings,
            )
            _cols, _rows, per_page = PriceTagPrintService._calc_grid(
                width_mm,
                height_mm,
                gap_mm,
                210,
                297,
                margin_mm,
            )
            total_labels = len(products_dicts)
            total_pages = max(1, math.ceil(total_labels / per_page)) if per_page > 0 else 1
        else:
            html = PriceTagPrintService.render_labels_sequential(
                template.content,
                products_dicts,
                settings,
            )
            total_labels = len(products_dicts)
            total_pages = None

        logger.info(
            "Згенеровано друк для накладної '%s': %s, %d товарів, %d змін цін",
            invoice.number,
            print_type,
            total_labels,
            changed_count,
        )

        return {
            "html": html,
            "total_labels": total_labels,
            "total_pages": total_pages,
            "changed_count": changed_count,
            "total_count": total_items,
        }
