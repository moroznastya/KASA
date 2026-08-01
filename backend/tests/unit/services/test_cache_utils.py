"""Unit-тести для cache_utils (декоратор cached, інвалідація)."""

import asyncio
from typing import Any, Optional
from uuid import uuid4

import pytest

from app.api.v2.cache_utils import (
    _make_cache_key,
    cached,
    invalidate_cache,
    invalidate_category_cache,
    invalidate_invoice_cache,
    invalidate_ledger_cache,
    invalidate_product_cache,
    invalidate_receipt_cache,
)


class FakeCache:
    """In-memory реалізація ICacheService для тестів."""

    def __init__(self) -> None:
        self._store: dict[str, Any] = {}
        self.set_calls: list[tuple[str, Any, Optional[int]]] = []
        self.cleared_patterns: list[str] = []

    async def get(self, key: str) -> Optional[Any]:
        return self._store.get(key)

    async def set(self, key: str, value: Any, ttl: Optional[int] = None) -> bool:
        self._store[key] = value
        self.set_calls.append((key, value, ttl))
        return True

    async def delete(self, key: str) -> bool:
        return self._store.pop(key, None) is not None

    async def exists(self, key: str) -> bool:
        return key in self._store

    async def clear_pattern(self, pattern: str) -> int:
        self.cleared_patterns.append(pattern)
        import fnmatch

        keys = [k for k in self._store if fnmatch.fnmatch(k, pattern)]
        for k in keys:
            del self._store[k]
        return len(keys)

    async def close(self) -> None:
        pass


@pytest.mark.asyncio
async def test_cached_miss_then_set():
    cache = FakeCache()
    calls = {"n": 0}

    @cached(cache, "products:list", ttl=30)
    async def fetch(page: int = 1, size: int = 20):
        calls["n"] += 1
        return {"items": [], "total": 0, "page": page, "size": size}

    result = await fetch(page=1, size=20)
    assert result == {"items": [], "total": 0, "page": 1, "size": 20}
    assert calls["n"] == 1

    # Кеш записано з TTL=30
    assert len(cache.set_calls) == 1
    key, value, ttl = cache.set_calls[0]
    assert key.startswith("products:list:")
    assert ttl == 30
    assert value == {"items": [], "total": 0, "page": 1, "size": 20}


@pytest.mark.asyncio
async def test_cached_hit_skips_function():
    cache = FakeCache()
    calls = {"n": 0}

    @cached(cache, "products:list", ttl=30)
    async def fetch(page: int = 1, size: int = 20):
        calls["n"] += 1
        return {"items": [], "total": 0, "page": page, "size": size}

    r1 = await fetch(page=1, size=20)
    r2 = await fetch(page=1, size=20)
    assert r1 == r2
    assert calls["n"] == 1, "Другий виклик має взяти дані з кешу (HIT)"


@pytest.mark.asyncio
async def test_cached_different_args_different_keys():
    cache = FakeCache()
    calls = {"n": 0}

    @cached(cache, "products:list", ttl=30)
    async def fetch(page: int = 1, size: int = 20):
        calls["n"] += 1
        return {"items": [], "total": 0, "page": page, "size": size}

    await fetch(page=1, size=20)
    await fetch(page=2, size=20)
    assert calls["n"] == 2, "Різні параметри → різні ключі кешу"


@pytest.mark.asyncio
async def test_cached_uses_default_ttl_when_none():
    cache = FakeCache()

    @cached(cache, "categories:list", ttl=None)
    async def fetch():
        return {"items": []}

    await fetch()
    assert cache.set_calls[0][2] is None or cache.set_calls[0][2] > 0


@pytest.mark.asyncio
async def test_cached_does_not_cache_none():
    cache = FakeCache()
    calls = {"n": 0}

    @cached(cache, "product:detail", ttl=60)
    async def fetch():
        calls["n"] += 1
        return None

    r = await fetch()
    assert r is None
    assert len(cache.set_calls) == 0, "None не має кешуватися"


def test_make_cache_key_is_stable():
    k1 = _make_cache_key("products:list", 1, 20, search="хліб", category_id=None)
    k2 = _make_cache_key("products:list", 1, 20, search="хліб", category_id=None)
    assert k1 == k2
    assert k1.startswith("products:list:")

    k3 = _make_cache_key("products:list", 2, 20, search="хліб", category_id=None)
    assert k1 != k3, "Різні аргументи → різні ключі"


@pytest.mark.asyncio
async def test_invalidate_cache_clears_by_pattern():
    cache = FakeCache()
    await cache.set("products:list:abc", {"items": []})
    await cache.set("products:barcode:def", {"id": str(uuid4())})
    await cache.set("product:xyz", {"id": str(uuid4())})

    count = await invalidate_cache(cache, "products:*")
    assert count == 2
    assert await cache.exists("products:list:abc") is False
    assert await cache.exists("product:xyz") is True, "product:* не входить у products:*"


@pytest.mark.asyncio
async def test_invalidate_product_cache():
    cache = FakeCache()
    await cache.set("products:list:abc", {})
    await cache.set("products:barcode:def", {})
    await cache.set("product:xyz", {})
    await invalidate_product_cache(cache)
    assert "products:*" in cache.cleared_patterns
    assert "product:*" in cache.cleared_patterns
    assert await cache.exists("products:list:abc") is False
    assert await cache.exists("product:xyz") is False


@pytest.mark.asyncio
async def test_invalidate_category_cache():
    cache = FakeCache()
    await cache.set("categories:list:abc", {})
    await cache.set("categories:tree:abc", {})
    await cache.set("category:xyz", {})
    await invalidate_category_cache(cache)
    assert "categories:*" in cache.cleared_patterns
    assert "category:*" in cache.cleared_patterns
    assert await cache.exists("categories:tree:abc") is False


@pytest.mark.asyncio
async def test_invalidate_ledger_cache():
    cache = FakeCache()
    await cache.set("ledger:entries:abc", {})
    await cache.set("ledger:balance:xyz", {})
    await invalidate_ledger_cache(cache)
    assert "ledger:*" in cache.cleared_patterns
    assert await cache.exists("ledger:entries:abc") is False


@pytest.mark.asyncio
async def test_invalidate_receipt_cache():
    cache = FakeCache()
    await cache.set("receipts:list:abc", {})
    await cache.set("receipts:stats:abc", {})
    await cache.set("receipt:xyz", {})
    await invalidate_receipt_cache(cache)
    assert "receipts:*" in cache.cleared_patterns
    assert "receipt:*" in cache.cleared_patterns
    assert await cache.exists("receipts:stats:abc") is False


@pytest.mark.asyncio
async def test_invalidate_invoice_cache():
    cache = FakeCache()
    await cache.set("invoices:list:abc", {})
    await cache.set("invoices:detail:xyz", {})
    await cache.set("invoice:abc", {})
    await invalidate_invoice_cache(cache)
    assert "invoices:*" in cache.cleared_patterns
    assert "invoice:*" in cache.cleared_patterns
    assert await cache.exists("invoices:detail:xyz") is False
