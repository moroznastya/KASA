"""
API ендпоінт для OCR розпізнавання накладних через Gemini API.

Ендпоінти:
  - POST /api/v1/ocr/invoice — аналіз зображення накладної
"""

import logging

from fastapi import APIRouter, Depends, File, HTTPException, UploadFile, status

from app.domain.services.auth_service import AuthService
from app.infrastructure.services.ocr_service import OCRService

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/ocr", tags=["OCR"])
ocr_service = OCRService()

# Дозволені MIME-типи зображень
ALLOWED_IMAGE_TYPES = {
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/bmp",
    "image/tiff",
}


@router.post("/invoice")
async def analyze_invoice(
    file: UploadFile = File(...),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Аналізує зображення накладної через Gemini API.

    Приймає файл зображення, передає його в Gemini API,
    отримує структуровані дані та повертає JSON для заповнення форми.

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
        "items": [...]
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

    # 3. Викликати OCR сервіс
    try:
        result = await ocr_service.analyze_invoice_image(image_data)
    except RuntimeError as e:
        logger.error(f"Помилка OCR сервісу: {e}")
        return {
            "success": False,
            "error": str(e),
        }
    except Exception as e:
        logger.error(f"Неочікувана помилка OCR: {e}", exc_info=True)
        return {
            "success": False,
            "error": f"Внутрішня помилка сервера: {e}",
        }

    # 4. Повернути результат
    return {
        "success": True,
        "data": result,
    }
