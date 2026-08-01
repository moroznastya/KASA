"""
Infrastructure Layer: RedisCacheService — реалізація ICacheService.

Використовує `redis.asyncio` для async/await підтримки.
Серіалізація значень: JSON (для сумісності з Python типами).

Правила:
1. Прозорий кеш: якщо Redis недоступний, методи повертають None/False/0
2. Graceful shutdown: close() закриває з'єднання
3. TTL: кожен ключ має обмежений час життя
4. Логування: всі помилки підключення логуються (debug/warning)
"""

from __future__ import annotations

import dataclasses
import json
import logging
from datetime import date, datetime
from decimal import Decimal
from typing import Any, Optional
from uuid import UUID

from pydantic import BaseModel

from app.domain.services.cache_service import ICacheService

logger = logging.getLogger(__name__)

try:
    from redis.asyncio import Redis as AsyncRedis
    REDIS_AVAILABLE = True
except ImportError:
    AsyncRedis = None  # type: ignore
    REDIS_AVAILABLE = False
    logger.warning("⚠️ redis.asyncio не встановлено. Кеш буде відключено.")


class RedisCacheService(ICacheService):
    """
    Реалізація ICacheService через Redis.

    Використовує пул з'єднань redis.asyncio для ефективної роботи.
    Підтримує JSON-серіалізацію значень.

    Args:
        url: Redis URL (наприклад, redis://localhost:6379/0).
        default_ttl: TTL за замовчуванням (секунди).
        max_retries: Кількість спроб підключення (0 — без повторів).
        retry_delay: Затримка між спробами (секунди).
    """

    def __init__(
        self,
        url: str = "redis://localhost:6379/0",
        default_ttl: int = 300,
        max_retries: int = 1,
        retry_delay: float = 0.5,
    ) -> None:
        self._url = url
        self._default_ttl = default_ttl
        self._max_retries = max_retries
        self._retry_delay = retry_delay
        self._redis: Optional[Any] = None
        self._connected: bool = False

    # ─── Підключення ─────────────────────────────────────────────────────────

    async def _ensure_connection(self) -> bool:
        """
        Забезпечує підключення до Redis.

        Returns:
            True якщо з'єднання встановлено, False при помилці.
        """
        if self._connected and self._redis is not None:
            return True

        if not REDIS_AVAILABLE:
            logger.warning("⚠️ redis.asyncio недоступний. Кеш відключено.")
            return False

        last_error: Optional[Exception] = None
        for attempt in range(1, self._max_retries + 2):  # +1 бо перша спроба не retry
            try:
                self._redis = AsyncRedis.from_url(
                    self._url,
                    decode_responses=True,
                    socket_connect_timeout=2,
                    socket_timeout=2,
                )
                await self._redis.ping()
                self._connected = True
                logger.info("✅ Підключено до Redis: %s", self._url)
                return True
            except Exception as e:
                last_error = e
                self._connected = False
                self._redis = None
                if attempt <= self._max_retries:
                    import asyncio
                    logger.debug(
                        "🔁 Спроба %d/%d підключення до Redis невдала: %s",
                        attempt, self._max_retries + 1, e,
                    )
                    await asyncio.sleep(self._retry_delay)
                else:
                    logger.warning(
                        "⚠️ Redis недоступний (%s). Кеш відключено.",
                        last_error,
                    )

        return False

    # ─── ICacheService Implementation ────────────────────────────────────────

    async def get(self, key: str) -> Optional[Any]:
        """
        Отримати значення з кешу.

        Returns:
            Десеріалізоване значення або None.
        """
        if not await self._ensure_connection():
            return None
        try:
            value = await self._redis.get(key)  # type: ignore
            if value is not None:
                return json.loads(value)
            return None
        except Exception as e:
            logger.debug("Помилка читання кешу [%s]: %s", key, e)
            return None

    @staticmethod
    def _default_serializer(obj: Any) -> Any:
        """
        Серіалізація складних типів у JSON-сумісні значення.

        - Pydantic v2 BaseModel → dict (model_dump(mode="json"))
        - dataclass (напр. ProductDTO, ReceiptDTO) → dict зі значеннями полів
        - UUID / Decimal → str
        - datetime / date → ISO-рядок
        - вкладені значення обробляються json.dumps рекурсивно через default

        Returns:
            JSON-сумісне значення.
        """
        if isinstance(obj, BaseModel):
            return obj.model_dump(mode="json")
        if dataclasses.is_dataclass(obj) and not isinstance(obj, type):
            return {
                f.name: getattr(obj, f.name)
                for f in dataclasses.fields(obj)
            }
        if isinstance(obj, (UUID, Decimal)):
            return str(obj)
        if isinstance(obj, (datetime, date)):
            return obj.isoformat()
        return str(obj)

    async def set(
        self,
        key: str,
        value: Any,
        ttl: Optional[int] = None,
    ) -> bool:
        """
        Зберегти значення в кеш.

        Pydantic-моделі, UUID, Decimal, datetime автоматично конвертуються
        у JSON-сумісні значення (щоб Cache HIT повертав коректні dict).

        Returns:
            True при успіху, False при помилці.
        """
        if not await self._ensure_connection():
            return False
        try:
            ttl_sec = ttl or self._default_ttl
            serialized = json.dumps(value, default=self._default_serializer)
            await self._redis.setex(key, ttl_sec, serialized)  # type: ignore
            return True
        except Exception as e:
            logger.debug("Помилка запису кешу [%s]: %s", key, e)
            return False

    async def delete(self, key: str) -> bool:
        """
        Видалити значення з кешу.

        Returns:
            True якщо ключ існував, False якщо ні.
        """
        if not await self._ensure_connection():
            return False
        try:
            result = await self._redis.delete(key)  # type: ignore
            return result > 0
        except Exception as e:
            logger.debug("Помилка видалення кешу [%s]: %s", key, e)
            return False

    async def exists(self, key: str) -> bool:
        """
        Перевірити, чи існує ключ.

        Returns:
            True якщо ключ існує.
        """
        if not await self._ensure_connection():
            return False
        try:
            return bool(await self._redis.exists(key))  # type: ignore
        except Exception:
            return False

    async def clear_pattern(self, pattern: str) -> int:
        """
        Інвалідувати всі ключі за патерном.

        Використовує SCAN для безпечного перебору ключів.
        Не використовує KEYS (блокуюча операція).

        Returns:
            Кількість видалених ключів.
        """
        if not await self._ensure_connection():
            return 0
        total_deleted = 0
        try:
            cursor = 0
            while True:
                cursor, keys = await self._redis.scan(  # type: ignore
                    cursor=cursor, match=pattern, count=100,
                )
                if keys:
                    deleted = await self._redis.delete(*keys)  # type: ignore
                    total_deleted += deleted
                if cursor == 0:
                    break
            if total_deleted > 0:
                logger.debug("🧹 Очищено кеш за патерном: %s (%d ключів)", pattern, total_deleted)
            return total_deleted
        except Exception as e:
            logger.debug("Помилка очищення кешу [%s]: %s", pattern, e)
            return 0

    async def close(self) -> None:
        """
        Закрити з'єднання з Redis (graceful shutdown).

        Викликається в lifespan при зупинці застосунку.
        """
        if self._redis is not None:
            try:
                await self._redis.close()
            except Exception as e:
                logger.debug("Помилка закриття Redis: %s", e)
            finally:
                self._redis = None
                self._connected = False
                logger.info("🔌 З'єднання з Redis закрито")
