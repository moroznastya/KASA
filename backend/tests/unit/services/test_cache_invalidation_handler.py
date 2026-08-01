"""Unit-тести CacheInvalidationHandler (інвалідація кешу при доменних подіях)."""

from typing import Any, Optional
from decimal import Decimal
from uuid import uuid4

import pytest

from app.application.event_handlers.cache_handler import CacheInvalidationHandler
from app.domain.events.invoice_events import (
    InvoiceApproved,
    InvoiceCreated,
    InvoiceDeleted,
    InvoiceUpdated,
)
from app.domain.events.ledger_events import LedgerEntryCreated
from app.domain.events.product_events import (
    ProductCreated,
    ProductDeleted,
    ProductUpdated,
    StockChanged,
)
from app.domain.events.receipt_events import ReceiptCreated, ReceiptRefunded


class FakeCache:
    def __init__(self) -> None:
        self.cleared: list[str] = []

    async def get(self, key: str) -> Optional[Any]:
        return None

    async def set(self, key: str, value: Any, ttl: Optional[int] = None) -> bool:
        return True

    async def delete(self, key: str) -> bool:
        return True

    async def exists(self, key: str) -> bool:
        return False

    async def clear_pattern(self, pattern: str) -> int:
        self.cleared.append(pattern)
        return 0

    async def close(self) -> None:
        pass


@pytest.mark.asyncio
async def test_product_created_invalidates_product_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    await handler.handle(ProductCreated(product_id=uuid4(), name="Тест", barcode="4820000000000"))
    assert "products:*" in cache.cleared
    assert "product:*" in cache.cleared


@pytest.mark.asyncio
async def test_product_updated_invalidates_product_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    await handler.handle(ProductUpdated(product_id=uuid4(), changes={"price": (10.0, 12.0)}))
    assert "products:*" in cache.cleared


@pytest.mark.asyncio
async def test_product_deleted_invalidates_product_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    await handler.handle(ProductDeleted(product_id=uuid4()))
    assert "products:*" in cache.cleared


@pytest.mark.asyncio
async def test_stock_changed_invalidates_product_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    await handler.handle(StockChanged(product_id=uuid4(), old_quantity=10.0, new_quantity=9.0, reason="sale"))
    assert "products:*" in cache.cleared


@pytest.mark.asyncio
async def test_invoice_events_invalidate_invoice_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    inv_id = uuid4()
    for event in (
        InvoiceCreated(invoice_id=inv_id, supplier_id=uuid4(), total_amount=Decimal("0")),
        InvoiceUpdated(invoice_id=inv_id, changes={"status": ("draft", "confirmed")}),
        InvoiceDeleted(invoice_id=inv_id),
        InvoiceApproved(invoice_id=inv_id, items_count=2),
    ):
        await handler.handle(event)
    assert cache.cleared.count("invoices:*") == 4
    assert cache.cleared.count("invoice:*") == 4


@pytest.mark.asyncio
async def test_receipt_created_invalidates_receipt_and_product_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    await handler.handle(ReceiptCreated(receipt_id=uuid4(), cashier_id=uuid4(), total_amount=Decimal("0"), payment_method="cash"))
    assert "receipts:*" in cache.cleared
    assert "products:*" in cache.cleared, "Продаж змінює залишки → products теж"


@pytest.mark.asyncio
async def test_receipt_refunded_invalidates_receipt_and_product_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    await handler.handle(ReceiptRefunded(receipt_id=uuid4(), original_receipt_id=uuid4(), refund_amount=Decimal("0")))
    assert "receipts:*" in cache.cleared
    assert "products:*" in cache.cleared


@pytest.mark.asyncio
async def test_ledger_entry_created_invalidates_ledger_cache():
    cache = FakeCache()
    handler = CacheInvalidationHandler(cache_service=cache)
    await handler.handle(LedgerEntryCreated(entry_id=uuid4(), supplier_id=uuid4(), amount=Decimal("0"), entry_type="debit", reference_type="invoice", reference_id=uuid4()))
    assert "ledger:*" in cache.cleared


@pytest.mark.asyncio
async def test_handler_without_cache_is_noop():
    handler = CacheInvalidationHandler(cache_service=None)
    await handler.handle(ProductCreated(product_id=uuid4(), name="Тест", barcode="4820000000000"))  # не має кинути помилку
