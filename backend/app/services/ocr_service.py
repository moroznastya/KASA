"""
OCR сервіс для розпізнавання накладних через Gemini API.

Підтримує:
- Ротацію API ключів при помилці 429 (Too Many Requests)
- Повторні спроби при тимчасових помилках (502, 503, timeout)
- Парсинг JSON з відповіді Gemini
- Логування помилок
"""

import json
import logging
import os
import re
from pathlib import Path

from google import genai
from google.genai import types as genai_types

logger = logging.getLogger(__name__)

# Шлях до файлу з ключами відносно кореня проекту
KEYS_FILE_PATH = Path(__file__).resolve().parent.parent.parent / "keys.txt"

# Промпт для Gemini (українською)
INVOICE_PROMPT = """Ти — асистент для розпізнавання прибуткових накладних. 
Проаналізуй зображення накладної та поверни ТІЛЬКИ JSON без додаткового тексту.
Формат JSON:
{
  "document_number": "рядок або null",
  "invoice_date": "рядок у форматі YYYY-MM-DD або null",
  "is_fiscal": true/false,
  "supplier_name": "рядок або null",
  "payment_method": "credit" | "bank_transfer" | "cash" | "other" | null,
  "items": [
    {
      "product_name": "рядок",
      "quantity": число,
      "price": число,
      "cost_price": число
    }
  ]
}

Правила:
- document_number: номер накладної (наприклад "ПН-00123")
- invoice_date: дата накладної
- is_fiscal: true якщо накладна фіскальна
- supplier_name: назва постачальника
- payment_method: спосіб оплати (credit - в борг, bank_transfer - перерахунок, cash - готівка, other - інше)
- items: масив товарів з назвою, кількістю, ціною та собівартістю
- Якщо якесь поле відсутнє на зображенні, використовуй null
- Якщо товарів немає, поверни порожній масив items"""


class OCRService:
    """Сервіс для розпізнавання накладних через Gemini API."""

    def __init__(self):
        self.api_keys: list[str] = []
        self.current_key_index: int = 0
        self.max_retries: int = 3
        self._load_api_keys()

    def _load_api_keys(self) -> None:
        """
        Завантажує ключі з keys.txt.
        Пропускає коментарі (#) та порожні рядки.
        """
        if not KEYS_FILE_PATH.exists():
            logger.warning(f"Файл ключів не знайдено: {KEYS_FILE_PATH}")
            self.api_keys = []
            return

        try:
            with open(KEYS_FILE_PATH, "r", encoding="utf-8") as f:
                keys = []
                for line in f:
                    stripped = line.strip()
                    # Пропускаємо коментарі та порожні рядки
                    if stripped and not stripped.startswith("#"):
                        keys.append(stripped)

            self.api_keys = keys
            logger.info(f"Завантажено {len(keys)} ключ(ів) Gemini API")
        except Exception as e:
            logger.error(f"Помилка завантаження ключів: {e}")
            self.api_keys = []

    def _get_current_key(self) -> str | None:
        """Повертає поточний ключ або None, якщо ключів немає."""
        if not self.api_keys:
            return None
        if self.current_key_index >= len(self.api_keys):
            return None
        return self.api_keys[self.current_key_index]

    def _rotate_key(self) -> None:
        """Перемикає на наступний ключ."""
        self.current_key_index += 1
        logger.info(
            f"Ротація ключа: перехід до індексу {self.current_key_index} "
            f"(всього ключів: {len(self.api_keys)})"
        )

    def _reset_key_index(self) -> None:
        """Скидає індекс ключа на початок."""
        self.current_key_index = 0

    def _parse_gemini_response(self, response_text: str) -> dict:
        """
        Парсить відповідь Gemini, витягує JSON.

        Шукає JSON у відповіді за допомогою регулярного виразу,
        оскільки Gemini може додавати текст до/після JSON.
        """
        if not response_text:
            raise ValueError("Порожня відповідь від Gemini")

        # Спроба знайти JSON у відповіді (між { та })
        json_match = re.search(r"\{.*\}", response_text, re.DOTALL)
        if not json_match:
            raise ValueError("Не вдалося знайти JSON у відповіді Gemini")

        json_str = json_match.group(0)

        try:
            data = json.loads(json_str)
        except json.JSONDecodeError as e:
            raise ValueError(f"Невалідний JSON у відповіді Gemini: {e}")

        # Валідація обов'язкових полів
        if not isinstance(data, dict):
            raise ValueError("Відповідь Gemini не є об'єктом JSON")

        # Переконуємось, що items — це список
        if "items" not in data or not isinstance(data.get("items"), list):
            data["items"] = []

        return data

    async def analyze_invoice_image(self, image_data: bytes) -> dict:
        """
        Аналізує зображення накладної через Gemini API.

        Args:
            image_data: Байти зображення (PNG, JPEG, WEBP тощо)

        Returns:
            dict: Розпізнані дані накладної у форматі:
                {
                    "document_number": str | None,
                    "invoice_date": str | None,
                    "is_fiscal": bool,
                    "supplier_name": str | None,
                    "payment_method": str | None,
                    "items": list[dict]
                }

        Raises:
            RuntimeError: Якщо всі ключі вичерпано або сталася критична помилка
        """
        # Скидаємо індекс ключа на початок при кожному новому запиті
        self._reset_key_index()

        last_error = None

        while True:
            api_key = self._get_current_key()
            if api_key is None:
                # Всі ключі вичерпано
                error_msg = (
                    "Всі ключі Gemini API вичерпано (всі повернули 429). "
                    "Додайте нові ключі в keys.txt"
                )
                logger.error(error_msg)
                raise RuntimeError(error_msg)

            for attempt in range(1, self.max_retries + 1):
                try:
                    logger.info(
                        f"Відправка запиту до Gemini API "
                        f"(ключ #{self.current_key_index + 1}, спроба {attempt}/{self.max_retries})"
                    )

                    client = genai.Client(api_key=api_key)

                    response = client.models.generate_content(
                        model="gemini-2.0-flash",
                        contents=[
                            INVOICE_PROMPT,
                            genai_types.Part.from_bytes(
                                data=image_data,
                                mime_type="image/jpeg",
                            ),
                        ],
                    )

                    # Перевіряємо, чи є текст у відповіді
                    if not response.candidates:
                        raise ValueError("Відповідь Gemini не містить кандидатів")

                    response_text = response.text
                    if not response_text:
                        raise ValueError("Порожній текст у відповіді Gemini")

                    # Парсимо JSON з відповіді
                    result = self._parse_gemini_response(response_text)
                    logger.info("Успішно отримано та розпарсено відповідь Gemini")
                    return result

                except Exception as e:
                    error_str = str(e).lower()
                    last_error = e

                    # Перевіряємо, чи це помилка 429 (Too Many Requests)
                    if "429" in error_str or "too many requests" in error_str or "rate limit" in error_str:
                        logger.warning(
                            f"Помилка 429 (Too Many Requests) для ключа #{self.current_key_index + 1}: {e}"
                        )
                        # Переходимо до наступного ключа
                        self._rotate_key()
                        break  # Виходимо з циклу спроб, переходимо до наступного ключа

                    # Перевіряємо, чи це тимчасова помилка (502, 503, timeout)
                    is_retryable = any(
                        code in error_str
                        for code in ["502", "503", "timeout", "deadline", "unavailable", "service unavailable"]
                    )

                    if is_retryable and attempt < self.max_retries:
                        logger.warning(
                            f"Тимчасова помилка (спроба {attempt}/{self.max_retries}): {e}. Повторюємо..."
                        )
                        continue  # Повторюємо спробу з тим самим ключем
                    elif is_retryable and attempt >= self.max_retries:
                        logger.error(
                            f"Всі {self.max_retries} спроб вичерпано для ключа #{self.current_key_index + 1}: {e}"
                        )
                        # Спробуємо наступний ключ
                        self._rotate_key()
                        break
                    else:
                        # Інша помилка — не retryable, пробуємо наступний ключ
                        logger.error(
                            f"Помилка Gemini API для ключа #{self.current_key_index + 1}: {e}"
                        )
                        self._rotate_key()
                        break

        # Якщо дійшли сюди — всі ключі та спроби вичерпано
        error_msg = f"Не вдалося отримати відповідь від Gemini API: {last_error}"
        logger.error(error_msg)
        raise RuntimeError(error_msg)
