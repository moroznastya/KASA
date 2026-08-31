"""Event Handler: Інвалідація кешу при доменних подіях."""

from __future__ import annotations

import logging
from typing import Any

from app.domain.events import (
    BaseDomainEvent,
    InvoiceApproved,
    InvoiceCreated,
    InvoiceDeleted,
    InvoiceUpdated,
    LedgerEntryCreated,
    ProductCreated,
    ProductDeleted,
    ProductUpdated,
    ReceiptCreated,
    ReceiptRefunded,
    StockChanged,
)

logger = logging.getLogger(__name__)


class CacheInvalidationHandler:
    """Інвалідація кешу при зміні даних."""

    def __init__(self, cache_service: Any = None):
        self._cache = cache_service

    async def handle(self, event: BaseDomainEvent) -> None:
        """Інвалідувати відповідний кеш."""

        if isinstance(event, (ProductCreated, ProductUpdated, ProductDeleted, StockChanged)):
            await self._invalidate_product_cache(getattr(event, "product_id", None))
            logger.debug(f"🧹 Кеш продуктів інвалідовано ({type(event).__name__})")
        elif isinstance(event, (InvoiceCreated, InvoiceUpdated, InvoiceDeleted, InvoiceApproved)):
            await self._invalidate_invoice_cache()
            logger.debug(f"🧹 Кеш накладних інвалідовано ({type(event).__name__})")
        elif isinstance(event, (ReceiptCreated, ReceiptRefunded)):
            await self._invalidate_receipt_cache()
            await self._invalidate_product_cache(None)  # залишки змінюються
            logger.debug(f"🧹 Кеш чеків інвалідовано ({type(event).__name__})")
        elif isinstance(event, LedgerEntryCreated):
            await self._invalidate_ledger_cache()
            logger.debug("🧹 Кеш ledger інвалідовано (LedgerEntryCreated)")

    async def _invalidate_product_cache(self, product_id: Any) -> None:
        """Інвалідувати кеш продуктів (списки, деталі, штрих-коди)."""
        if not self._cache:
            return
        if product_id is not None:
            await self._cache.clear_pattern(f"product:{product_id}:*")
        await self._cache.clear_pattern("products:*")
        await self._cache.clear_pattern("product:*")

    async def _invalidate_invoice_cache(self) -> None:
        """Інвалідувати кеш накладних."""
        if self._cache:
            await self._cache.clear_pattern("invoices:*")
            await self._cache.clear_pattern("invoice:*")

    async def _invalidate_receipt_cache(self) -> None:
        """Інвалідувати кеш чеків."""
        if self._cache:
            await self._cache.clear_pattern("receipts:*")
            await self._cache.clear_pattern("receipt:*")

    async def _invalidate_ledger_cache(self) -> None:
        """Інвалідувати кеш ledger (баланси, історія)."""
        if self._cache:
            await self._cache.clear_pattern("ledger:*")
