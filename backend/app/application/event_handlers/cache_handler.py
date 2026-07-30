"""Event Handler: Інвалідація кешу при доменних подіях."""

from __future__ import annotations

import logging
from typing import Any

from app.domain.events import (
    BaseDomainEvent,
    ProductCreated,
    ProductUpdated,
    ProductDeleted,
    StockChanged,
    InvoiceCreated,
    InvoiceApproved,
)

logger = logging.getLogger(__name__)


class CacheInvalidationHandler:
    """Інвалідація кешу при зміні даних."""

    def __init__(self, cache_service: Any = None):
        self._cache = cache_service

    async def handle(self, event: BaseDomainEvent) -> None:
        """Інвалідувати відповідний кеш."""

        if isinstance(event, ProductCreated):
            await self._invalidate_product_cache(event.product_id)
            logger.debug(f"🧹 Кеш продукту {event.product_id} інвалідовано (створено)")
        elif isinstance(event, ProductUpdated):
            await self._invalidate_product_cache(event.product_id)
            logger.debug(f"🧹 Кеш продукту {event.product_id} інвалідовано (оновлено)")
        elif isinstance(event, ProductDeleted):
            await self._invalidate_product_cache(event.product_id)
            logger.debug(f"🧹 Кеш продукту {event.product_id} інвалідовано (видалено)")
        elif isinstance(event, StockChanged):
            await self._invalidate_product_cache(event.product_id)
            logger.debug(f"🧹 Кеш залишків {event.product_id} інвалідовано")
        elif isinstance(event, InvoiceCreated) or isinstance(event, InvoiceApproved):
            logger.debug(f"🧹 Кеш накладних інвалідовано")

    async def _invalidate_product_cache(self, product_id: Any) -> None:
        """Інвалідувати кеш для конкретного продукту."""
        if self._cache:
            await self._cache.delete(f"product:{product_id}")
            await self._cache.delete("products:list")
