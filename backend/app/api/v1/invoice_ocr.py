"""
API ендпоінт для OCR-розпізнавання накладної з автоматичним зіставленням товарів з БД.

Ендпоінти:
  - POST /api/v1/invoice-ocr/analyze — аналіз накладної + зіставлення товарів
"""

import logging

from fastapi import APIRouter, Depends, File, HTTPException, UploadFile, status
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.domain.services.auth_service import AuthService
from app.infrastructure.services.invoice_ocr_service import InvoiceOCRService

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/invoice-ocr", tags=["OCR накладних"])

# Дозволені MIME-типи зображень
ALLOWED_IMAGE_TYPES = {
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/bmp",
    "image/tiff",
}


@router.post("/analyze")
async def analyze_invoice_with_matching(
    file: UploadFile = File(...),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Аналізує зображення накладної через Gemini API та автоматично
    зіставляє товари з наявною базою товарів.

    **Алгоритм:**
    1. Аналіз накладної через Gemini (отримуємо товари з накладної)
    2. Для кожного товару:
       - Якщо є штрих-код — пошук в БД за штрих-кодом
       - Якщо немає штрих-коду — пошук за назвою
    3. Для незнайдених товарів — відправка в Gemini списку всіх товарів з БД
       для точного зіставлення назв
    4. Повернення результату з matched_product_id

    **Формат відповіді (успіх):**
    ```json
    {
      "success": true,
      "data": {
        "document_number": "ПН-00123",
        "invoice_date": "2026-07-23",
        "is_fiscal": false,
        "supplier_name": "ТОВ Постачальник",
        "payment_method": "credit",
        "items": [
          {
            "product_name": "Молоко 2.6%",
            "quantity": 10,
            "price": 100.00,
            "cost_price": 80.00,
            "matched_product_id": "uuid-товару-з-бд",
            "matched_product_name": "Молоко 2.6% с/п ТМ Селянське",
            "matched_barcode": "4821234567890",
            "match_source": "name|barcode|gemini|not_found"
          }
        ]
      }
    }
    ```

    **Формат відповіді (помилка):**
    ```json
    {
      "success": false,
      "error": "Опис помилки"
    }
    ```
    """
    # 1. Перевірити, що файл — зображення
    if file.content_type not in ALLOWED_IMAGE_TYPES:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Непідтримуваний тип файлу '{file.content_type}'. "
                   f"Дозволені типи: {', '.join(sorted(ALLOWED_IMAGE_TYPES))}",
        )

    # 2. Прочитати файл
    try:
        image_data = await file.read()
    except Exception as e:
        logger.error(f"Помилка читання файлу: {e}")
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Помилка читання файлу: {e}",
        )

    if not image_data:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Файл порожній",
        )

    # 3. Викликати InvoiceOCRService
    try:
        service = InvoiceOCRService(session)
        result = await service.analyze_and_match(image_data)
    except RuntimeError as e:
        logger.error(f"Помилка OCR сервісу: {e}")
        return {
            "success": False,
            "error": str(e),
        }
    except Exception as e:
        logger.error(f"Неочікувана помилка: {e}", exc_info=True)
        return {
            "success": False,
            "error": f"Внутрішня помилка сервера: {e}",
        }

    return result
