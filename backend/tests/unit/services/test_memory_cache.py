"""Unit-тести MemoryCacheService (in-memory TTL fallback)."""

import asyncio

import pytest

from app.infrastructure.cache.memory_cache import MemoryCacheService


@pytest.mark.asyncio
async def test_set_get():
    cache = MemoryCacheService(default_ttl=60)
    await cache.set("products:list:abc", {"items": []}, ttl=30)
    assert await cache.get("products:list:abc") == {"items": []}


@pytest.mark.asyncio
async def test_get_missing_returns_none():
    cache = MemoryCacheService()
    assert await cache.get("nope") is None


@pytest.mark.asyncio
async def test_ttl_expiry():
    cache = MemoryCacheService(default_ttl=60)
    await cache.set("key", "value", ttl=0.05)
    await asyncio.sleep(0.1)
    assert await cache.get("key") is None


@pytest.mark.asyncio
async def test_delete():
    cache = MemoryCacheService()
    await cache.set("key", 42)
    assert await cache.delete("key") is True
    assert await cache.delete("key") is False
    assert await cache.get("key") is None


@pytest.mark.asyncio
async def test_exists():
    cache = MemoryCacheService()
    await cache.set("key", 1)
    assert await cache.exists("key") is True
    await cache.delete("key")
    assert await cache.exists("key") is False


@pytest.mark.asyncio
async def test_clear_pattern():
    cache = MemoryCacheService()
    await cache.set("products:list:1", {})
    await cache.set("products:list:2", {})
    await cache.set("product:3", {})
    await cache.set("categories:list:1", {})

    count = await cache.clear_pattern("products:*")
    assert count == 2, "products:* має збігатися лише з products:* (не product:*)"
    assert await cache.exists("products:list:1") is False
    assert await cache.exists("product:3") is True, "product:* не входить у products:*"

    # Другий патерн для одиничних ключів
    count2 = await cache.clear_pattern("product:*")
    assert count2 == 1
    assert await cache.exists("product:3") is False
    assert await cache.exists("categories:list:1") is True


@pytest.mark.asyncio
async def test_default_ttl_used():
    cache = MemoryCacheService(default_ttl=100)
    await cache.set("key", "v")  # без ttl → default
    item = cache._store["key"]
    assert item[0] > 0


@pytest.mark.asyncio
async def test_concurrent_set_get():
    """Асинхронний конкурентний доступ не має втрачати дані."""
    cache = MemoryCacheService()

    async def worker(i: int):
        for j in range(50):
            await cache.set(f"k:{i}:{j}", j)
            assert await cache.get(f"k:{i}:{j}") == j

    await asyncio.gather(*(worker(i) for i in range(5)))
    assert await cache.exists("k:4:49") is True


@pytest.mark.asyncio
async def test_close_clears_store():
    cache = MemoryCacheService()
    await cache.set("key", 1)
    await cache.close()
    assert await cache.get("key") is None
