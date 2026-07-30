"""
Сервіс для OCR-розпізнавання накладної з автоматичним зіставленням товарів з БД.

Алгоритм роботи:
1. Аналіз зображення накладної через Gemini API (отримуємо товари з накладної)
2. Для кожного товару з накладної:
   a. Якщо Gemini повернув штрих-код (barcode) — шукаємо в БД за штрих-кодом (стовідсотковий збіг)
   b. Якщо немає штрих-коду або не знайдено — шукаємо за назвою
3. Якщо товар не знайдено в БД — додаємо до накладної з позначкою "not_found"
   (товар позначається як відсутній в базі, але додається до накладної)
4. Повертаємо фінальний список товарів з product_id (якщо знайдено) та markup_percent
"""

import asyncio
import json
import logging
from typing import Optional
from uuid import UUID

from sqlalchemy import select, or_
from sqlalchemy.ext.asyncio import AsyncSession

from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.barcode import Barcode
from app.infrastructure.services.ocr_service import OCRService

logger = logging.getLogger(__name__)


class InvoiceOCRService:
    """
    Сервіс для OCR-розпізнавання накладної з автоматичним зіставленням товарів з БД.
    """

    def __init__(self, session: AsyncSession):
        self.session = session
        self.ocr_service = OCRService()

    async def analyze_and_match(
        self,
        image_data: bytes,
    ) -> dict:
        """
        Аналізує накладну та зіставляє товари з БД.

        Args:
            image_data: Байти зображення накладної.

        Returns:
            dict: Результат у форматі:
                {
                    "success": True,
                    "data": {
                        "document_number": ...,
                        "invoice_date": ...,
                        "is_fiscal": ...,
                        "supplier_name": ...,
                        "payment_method": ...,
                        "items": [
                            {
                                "product_name": "назва з накладної",
                                "quantity": 10,
                                "cost_price": 80.00,
                                "matched_product_id": "UUID або null",
                                "matched_product_name": "назва з БД або null",
                                "matched_barcode": "штрих-код або null",
                                "markup_percent": 20.0,
                                "match_source": "barcode|name|not_found"
                            }
                        ]
                    }
                }
        """
        # Крок 1: Аналізуємо накладну через Gemini
        logger.info("Крок 1: Аналіз накладної через Gemini...")
        ocr_result = await self.ocr_service.analyze_invoice_image(image_data)

        if not ocr_result.get("items"):
            logger.warning("OCR не знайшов товарів у накладній")
            return {
                "success": True,
                "data": {
                    **ocr_result,
                    "items": [],
                },
            }

        items_from_invoice = ocr_result["items"]
        logger.info(f"Знайдено {len(items_from_invoice)} товарів у накладній")

        # Крок 2: Зіставляємо товари з БД
        logger.info("Крок 2: Зіставлення товарів з БД...")
        matched_items = await self._match_items_with_db(items_from_invoice)

        # Підраховуємо статистику
        matched_count = sum(1 for item in matched_items if item.get("matched_product_id"))
        not_found_count = len(matched_items) - matched_count
        logger.info(
            f"Зіставлено: {matched_count} з {len(matched_items)} товарів "
            f"(не знайдено в БД: {not_found_count} — додано до накладної без прив'язки)"
        )

        return {
            "success": True,
            "data": {
                "document_number": ocr_result.get("document_number"),
                "invoice_date": ocr_result.get("invoice_date"),
                "is_fiscal": ocr_result.get("is_fiscal", False),
                "supplier_name": ocr_result.get("supplier_name"),
                "payment_method": ocr_result.get("payment_method"),
                "items": matched_items,
            },
        }

    async def _match_items_with_db(self, items: list[dict]) -> list[dict]:
        """
        Зіставляє товари з накладної з товарами в БД.

        Алгоритм:
        1. Стовідсотковий збіг за штрих-кодом (якщо Gemini повернув barcode)
        2. Якщо немає штрих-коду або не знайдено — шукаємо за назвою
        3. Якщо не знайдено — додаємо до накладної з позначкою "not_found"

        Args:
            items: Список товарів з накладної.

        Returns:
            list[dict]: Товари з доданими полями matched_product_id, matched_product_name,
                       matched_barcode, markup_percent, match_source.
        """
        matched_items = []

        for item in items:
            product_name = item.get("product_name", "")
            quantity = item.get("quantity", 1)
            # Нас цікавить тільки вартість з ПДВ (cost_price)
            cost_price = float(item.get("cost_price", 0))
            # Штрих-код, який Gemini розпізнав на накладній
            barcode_from_gemini = item.get("barcode")

            logger.info(f"  Обробка товару: '{product_name}' (qty={quantity}, cost_price={cost_price})")

            # ─── Крок 2a: Стовідсотковий збіг за штрих-кодом (пріоритет) ───
            barcode_to_search = None

            # 1. Якщо Gemini повернув barcode — використовуємо його в першу чергу
            if barcode_from_gemini and str(barcode_from_gemini).strip():
                barcode_to_search = str(barcode_from_gemini).strip()
                logger.info(f"    Gemini повернув штрих-код: {barcode_to_search}")

            # 2. Якщо немає — перевіряємо, чи є штрих-код в назві товару (формат: "Назва (ШК: 12345)")
            if not barcode_to_search:
                import re
                barcode_in_name = re.search(r'[Шш][КкКк]\s*[:：]\s*(\d{8,14})', product_name)
                if barcode_in_name:
                    barcode_to_search = barcode_in_name.group(1)
                    logger.info(f"    Знайдено штрих-код в назві: {barcode_to_search}")

            # 3. Якщо немає — перевіряємо, чи сама назва є штрих-кодом (тільки цифри, 8-14 символів)
            if not barcode_to_search:
                import re
                if re.match(r'^\d{8,14}$', product_name.strip()):
                    barcode_to_search = product_name.strip()
                    logger.info(f"    Назва є штрих-кодом: {barcode_to_search}")

            # Якщо знайшли штрих-код — шукаємо товар в БД
            if barcode_to_search:
                product = await self._find_product_by_barcode(barcode_to_search)
                if product:
                    matched_items.append(self._make_result_item(
                        product_name=product_name,
                        quantity=quantity,
                        cost_price=cost_price,
                        product=product,
                        match_source="barcode",
                    ))
                    logger.info(f"    ✅ Стовідсотковий збіг за штрих-кодом: '{product.title}' (націнка: {product.markup}%)")
                    continue
                else:
                    logger.info(f"    ❌ Товар за штрих-кодом {barcode_to_search} не знайдено в БД")

            # ─── Крок 2b: Спроба знайти за назвою ──────────────────────────
            product = await self._find_product_by_name(product_name)
            if product:
                matched_items.append(self._make_result_item(
                    product_name=product_name,
                    quantity=quantity,
                    cost_price=cost_price,
                    product=product,
                    match_source="name",
                ))
                logger.info(f"    ✅ Знайдено за назвою: '{product.title}' (націнка: {product.markup}%)")
                continue

            # ─── Крок 2c: Товар не знайдено в БД — додаємо до накладної з позначкою
            logger.info(f"    ⚠️ Товар не знайдено в БД, додаю до накладної: '{product_name}'")
            matched_items.append({
                "product_name": product_name,
                "quantity": quantity,
                "cost_price": cost_price,
                "matched_product_id": None,
                "matched_product_name": None,
                "matched_barcode": None,
                "markup_percent": 0.0,
                "match_source": "not_found",
            })

        return matched_items

    def _make_result_item(
        self,
        product_name: str,
        quantity: int | float,
        cost_price: float,
        product: Product,
        match_source: str,
    ) -> dict:
        """
        Формує результат для знайденого товару, додаючи markup_percent з карточки товару.
        """
        return {
            "product_name": product_name,
            "quantity": quantity,
            "cost_price": cost_price,
            "matched_product_id": str(product.id),
            "matched_product_name": product.title,
            "matched_barcode": product.barcode,
            "markup_percent": float(product.markup) if product.markup is not None else 0.0,
            "match_source": match_source,
        }

    async def _find_product_by_barcode(self, barcode: str) -> Optional[Product]:
        """
        Шукає товар в БД за штрих-кодом.

        Спочатку шукає в таблиці Barcode (якщо є окрема таблиця штрих-кодів),
        потім в таблиці Product (поле barcode).

        Args:
            barcode: Штрих-код для пошуку.

        Returns:
            Optional[Product]: Знайдений товар або None.
        """
        # Спроба 1: Пошук в таблиці Barcode
        try:
            result = await self.session.execute(
                select(Barcode).where(Barcode.barcode == barcode)
            )
            barcode_record = result.scalar_one_or_none()
            if barcode_record and barcode_record.product_id:
                result = await self.session.execute(
                    select(Product).where(Product.id == barcode_record.product_id)
                )
                product = result.scalar_one_or_none()
                if product:
                    logger.info(f"      Знайдено через Barcode таблицю: {product.title}")
                    return product
        except Exception as e:
            logger.warning(f"      Помилка пошуку в Barcode таблиці: {e}")

        # Спроба 2: Пошук в таблиці Product (поле barcode)
        try:
            result = await self.session.execute(
                select(Product).where(Product.barcode == barcode)
            )
            product = result.scalar_one_or_none()
            if product:
                logger.info(f"      Знайдено через Product.barcode: {product.title}")
                return product
        except Exception as e:
            logger.warning(f"      Помилка пошуку в Product.barcode: {e}")

        # Спроба 3: Пошук з ігноруванням пробілів та дефісів
        clean_barcode = barcode.replace(" ", "").replace("-", "").replace("_", "")
        if clean_barcode != barcode:
            logger.info(f"      Спроба пошуку з очищеним штрих-кодом: {clean_barcode}")
            return await self._find_product_by_barcode(clean_barcode)

        return None

    async def _find_product_by_name(self, name: str) -> Optional[Product]:
        """
        Шукає товар в БД за назвою.

        Алгоритм:
        1. Точний збіг (case-insensitive)
        2. Частковий збіг (назва містить пошуковий запит)
        3. Пошук за окремими словами

        Args:
            name: Назва товару для пошуку.

        Returns:
            Optional[Product]: Знайдений товар або None.
        """
        if not name or not name.strip():
            return None

        clean_name = name.strip()

        # Спроба 1: Точний збіг (case-insensitive)
        try:
            result = await self.session.execute(
                select(Product).where(Product.title.ilike(clean_name))
            )
            product = result.scalar_one_or_none()
            if product:
                logger.info(f"      Точний збіг за назвою: {product.title}")
                return product
        except Exception as e:
            logger.warning(f"      Помилка точного пошуку: {e}")

        # Спроба 2: Частковий збіг (назва товару містить пошуковий запит)
        try:
            result = await self.session.execute(
                select(Product).where(Product.title.ilike(f"%{clean_name}%"))
            )
            product = result.scalar_one_or_none()
            if product:
                logger.info(f"      Частковий збіг за назвою: {product.title}")
                return product
        except Exception as e:
            logger.warning(f"      Помилка часткового пошуку: {e}")

        # Спроба 3: Пошук за окремими словами (якщо назва складається з кількох слів)
        words = clean_name.split()
        if len(words) > 1:
            try:
                conditions = [Product.title.ilike(f"%{word}%") for word in words]
                result = await self.session.execute(
                    select(Product).where(or_(*conditions))
                )
                products = result.scalars().all()
                if products:
                    best_match = max(
                        products,
                        key=lambda p: sum(1 for w in words if w.lower() in p.title.lower())
                    )
                    logger.info(f"      Збіг за словами: {best_match.title}")
                    return best_match
            except Exception as e:
                logger.warning(f"      Помилка пошуку за словами: {e}")

        return None
