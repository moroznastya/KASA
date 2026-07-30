"""
Cache Utils для API v2 — декоратори та хелпери для кешування.

Використовує ICacheService для прозорого кешування.
Якщо кеш недоступний, запити проходять без кешування (fail-open).
"""

from __future__ import annotations

import hashlib
import json
import logging
from functools import wraps
from typing import Any, Callable, Optional, TypeVar

from app.config import settings
from app.domain.services.cache_service import ICacheService

logger = logging.getLogger(__name__)

# Тип для закешованої функції
F = TypeVar("F", bound=Callable[..., Any])


def _make_cache_key(prefix: str, *args, **kwargs) -> str:
    """
    Створює унікальний ключ кешу на основі параметрів запиту.

    Формат: {prefix}:{md5_of_args}

    Args:
        prefix: Префікс ключа (наприклад, "products:list").
        args: Позиційні аргументи.
        kwargs: Іменовані аргументи.

    Returns:
        Ключ кешу у форматі "prefix:hash".
    """
    raw = f"{args}:{json.dumps(kwargs, sort_keys=True, default=str)}"
    hash_digest = hashlib.md5(raw.encode()).hexdigest()
    return f"{prefix}:{hash_digest}"


def cached(
    cache_service: ICacheService,
    prefix: str,
    ttl: Optional[int] = None,
) -> Callable[[F], F]:
    """
    Декоратор для кешування результатів асинхронних функцій.

    Args:
        cache_service: Сервіс кешування (ICacheService).
        prefix: Префікс для ключа кешу (наприклад, "products:list").
        ttl: Час життя кешу в секундах. Якщо None — використовується CACHE_TTL_DEFAULT.

    Usage:
        @cached(cache_service, "products:list", ttl=60)
        async def list_products(page=1, size=20):
            ...

    Важливо:
        - Кешується ТІЛЬКИ успішний результат (не помилки)
        - Якщо кеш недоступний, функція працює без кешування
        - Ключ формується на основі всіх аргументів функції
    """
    effective_ttl = ttl or settings.CACHE_TTL_DEFAULT

    def decorator(func: F) -> F:
        @wraps(func)
        async def wrapper(*args, **kwargs) -> Any:
            cache_key = _make_cache_key(prefix, *args, **kwargs)

            # Спроба отримати з кешу
            cached_value = await cache_service.get(cache_key)
            if cached_value is not None:
                logger.debug(f"⚡ Cache HIT: {cache_key}")
                return cached_value

            logger.debug(f"💤 Cache MISS: {cache_key}")

            # Виконуємо оригінальну функцію
            result = await func(*args, **kwargs)

            # Зберігаємо в кеш (тільки успішний результат)
            if result is not None:
                await cache_service.set(cache_key, result, ttl=effective_ttl)
                logger.debug(f"💾 Cache SET: {cache_key} (TTL={effective_ttl}s)")

            return result

        return wrapper  # type: ignore

    return decorator


async def invalidate_cache(
    cache_service: ICacheService,
    pattern: str,
) -> int:
    """
    Інвалідувати кеш за патерном.

    Args:
        cache_service: Сервіс кешування.
        pattern: Патерн ключів для видалення (наприклад, "products:list:*").

    Returns:
        Кількість видалених ключів.
    """
    count = await cache_service.clear_pattern(pattern)
    if count > 0:
        logger.debug(f"🧹 Cache invalidated: {pattern} ({count} keys)")
    return count


async def invalidate_product_cache(cache_service: ICacheService) -> None:
    """Інвалідувати всі кеші, пов'язані з продуктами."""
    await invalidate_cache(cache_service, "products:list:*")
    await invalidate_cache(cache_service, "product:*")


async def invalidate_category_cache(cache_service: ICacheService) -> None:
    """Інвалідувати всі кеші, пов'язані з категоріями."""
    await invalidate_cache(cache_service, "categories:list:*")
    await invalidate_cache(cache_service, "category:*")
