"""
Infrastructure Layer: MemoryCacheService — in-memory TTL-кеш (fallback).

Використовується, коли Redis недоступний (redis.asyncio не встановлено):
- dict + monotonic time для TTL
- asyncio.Lock для async-safe доступу
- Прозорий: методи повертають None/False/0 при помилках
- Підтримує clear_pattern (fnmatch) для інвалідації
"""

from __future__ import annotations

import asyncio
import fnmatch
import logging
import time
from typing import Any, Optional

logger = logging.getLogger(__name__)


class MemoryCacheService:
    """
    In-memory реалізація ICacheService (сумісна з Protocol).

    Кожен ключ має TTL. Після закінчення TTL ключ вважається відсутнім
    і видаляється при наступному зверненні (lazy cleanup).

    Args:
        default_ttl: TTL за замовчуванням (секунди).
    """

    def __init__(self, default_ttl: int = 300) -> None:
        self._default_ttl = default_ttl
        self._store: dict[str, tuple[float, Any]] = {}
        self._lock = asyncio.Lock()

    def _is_expired(self, key: str, expires_at: float) -> bool:
        return time.monotonic() > expires_at

    async def get(self, key: str) -> Optional[Any]:
        """Отримати значення (None, якщо ключа немає або TTL минув)."""
        async with self._lock:
            item = self._store.get(key)
            if item is None:
                return None
            expires_at, value = item
            if self._is_expired(key, expires_at):
                del self._store[key]
                return None
            return value

    async def set(
        self,
        key: str,
        value: Any,
        ttl: Optional[int] = None,
    ) -> bool:
        """Зберегти значення з TTL."""
        ttl_sec = ttl or self._default_ttl
        async with self._lock:
            self._store[key] = (time.monotonic() + ttl_sec, value)
        return True

    async def delete(self, key: str) -> bool:
        """Видалити ключ (True, якщо ключ існував)."""
        async with self._lock:
            return self._store.pop(key, None) is not None

    async def exists(self, key: str) -> bool:
        """Перевірити наявність ключа (з урахуванням TTL)."""
        return await self.get(key) is not None

    async def clear_pattern(self, pattern: str) -> int:
        """Інвалідувати всі ключі, що відповідають fnmatch-патерну."""
        async with self._lock:
            keys = [k for k in self._store if fnmatch.fnmatch(k, pattern)]
            for k in keys:
                del self._store[k]
            if keys:
                logger.debug(f"🧹 MemoryCache invalidated: {pattern} ({len(keys)} keys)")
            return len(keys)

    async def close(self) -> None:
        """Очистити кеш (graceful shutdown)."""
        async with self._lock:
            self._store.clear()
