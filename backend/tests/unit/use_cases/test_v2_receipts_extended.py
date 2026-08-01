"""Unit tests: нові методи ReceiptUseCases (stats, search, recent-sales, returnable, items)."""

from __future__ import annotations

from datetime import datetime, timezone
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.use_cases.receipt_use_cases import ReceiptUseCases


def _build_use_cases() -> tuple[ReceiptUseCases, AsyncMock]:
    """ReceiptUseCases з моками."""
    receipt_repo = AsyncMock()
    product_repo = AsyncMock()
    uow = MagicMock()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        return False

    uow.__aenter__ = __aenter__
    uow.__aexit__ = __aexit__
    uow.commit = AsyncMock()

    use_cases = ReceiptUseCases(
        receipt_repo=receipt_repo,
        product_repo=product_repo,
        unit_of_work=uow,
        event_bus=AsyncMock(),
    )
    return use_cases, receipt_repo


class TestTodayStats:
    async def test_stats_delegates(self):
        """Статистика за день делегується в репозиторій."""
        use_cases, repo = _build_use_cases()
        repo.get_today_stats.return_value = {
            "total_sales": 1000.0,
            "total_returns": 50.0,
            "total_profit": 300.0,
            "total_vat": 166.67,
            "receipts_count": 10,
            "items_sold": 25,
            "date": "2026-08-01",
        }

        stats = await use_cases.get_today_stats()

        assert stats["total_sales"] == 1000.0
        assert stats["receipts_count"] == 10
        repo.get_today_stats.assert_awaited_once()


class TestSearchReceipts:
    async def test_search_delegates(self):
        """Пошук чеків повертає спрощені записи + total."""
        use_cases, repo = _build_use_cases()

        receipt = MagicMock()
        receipt.id = uuid4()
        receipt.receipt_number = "RCPT-001"
        receipt.receipt_type = MagicMock(value="sale")
        receipt.total_amount = 100.0
        receipt.created_at = datetime.now(timezone.utc)
        from types import SimpleNamespace
        receipt.cashier = SimpleNamespace(name="Касир")
        receipt.items = [MagicMock(), MagicMock()]

        repo.search_with_details.return_value = ([receipt], 1)

        items, total = await use_cases.search_receipts(q="RCPT", receipt_type="sale")

        assert total == 1
        assert items[0]["receipt_number"] == "RCPT-001"
        assert items[0]["cashier_name"] == "Касир"
        assert items[0]["items_count"] == 2

    async def test_search_invalid_type_raises(self):
        """Невірний тип чеку → ValueError."""
        use_cases, _ = _build_use_cases()

        with pytest.raises(ValueError, match="Невірний тип"):
            await use_cases.search_receipts(q="x", receipt_type="unknown")


class TestRecentSalesByProduct:
    async def test_recent_sales_found(self):
        """Останні продажі товару."""
        use_cases, repo = _build_use_cases()
        repo.find_recent_sales_by_product.return_value = [
            {
                "product": {"id": uuid4(), "title": "Товар"},
                "total_sold": Decimal("5"),
                "total_returned": Decimal("1"),
                "returnable": Decimal("4"),
                "recent_sales": [],
            }
        ]

        items = await use_cases.get_recent_sales_by_product("Товар", limit=5)

        assert len(items) == 1
        assert items[0]["returnable"] == Decimal("4")
        repo.find_recent_sales_by_product.assert_awaited_once_with("Товар", 5)

    async def test_recent_sales_not_found_raises(self):
        """Товарів не знайдено → ValueError (роутер віддає 404)."""
        use_cases, repo = _build_use_cases()
        repo.find_recent_sales_by_product.return_value = []

        with pytest.raises(ValueError, match="не знайдено"):
            await use_cases.get_recent_sales_by_product("zzz", limit=5)


class TestReturnableQuantity:
    async def test_returnable_success(self):
        """Повертальна кількість товару."""
        use_cases, repo = _build_use_cases()
        product_id = uuid4()
        repo.find_by_id.return_value = MagicMock()  # product існує
        repo.get_sold_returned_totals.return_value = (Decimal("10"), Decimal("2"))
        repo.get_returnable_quantity.return_value = Decimal("8")

        result = await use_cases.get_returnable_quantity(product_id)

        assert result["total_sold"] == 10.0
        assert result["total_returned"] == 2.0
        assert result["returnable"] == 8.0

    async def test_returnable_product_not_found(self):
        """Товар не знайдено → ValueError."""
        from app.application.use_cases.receipt_use_cases import ReceiptUseCases
        from app.infrastructure.persistence.unit_of_work import SQLAlchemyUnitOfWork

        receipt_repo = AsyncMock()
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = None
        uow = MagicMock()
        use_cases = ReceiptUseCases(
            receipt_repo=receipt_repo,
            product_repo=product_repo,
            unit_of_work=uow,
            event_bus=AsyncMock(),
        )

        with pytest.raises(ValueError, match="не знайдено"):
            await use_cases.get_returnable_quantity(uuid4())


class TestReceiptItems:
    async def test_items_success(self):
        """Позиції чеку з назвами товарів."""
        use_cases, repo = _build_use_cases()
        receipt_id = uuid4()
        repo.find_by_id.return_value = MagicMock()  # чек існує

        item = MagicMock()
        item.id = uuid4()
        item.product_id = uuid4()
        item.product = MagicMock(title="Товар", barcode="123")
        item.quantity = 2.0
        item.price = 50.0
        item.total = 100.0
        item.purchase_price = 30.0
        item.created_at = None
        repo.find_items_with_products.return_value = [item]

        items = await use_cases.get_receipt_items(receipt_id)

        assert len(items) == 1
        assert items[0]["product_name"] == "Товар"
        assert items[0]["product_barcode"] == "123"
        assert items[0]["quantity"] == 2.0

    async def test_items_receipt_not_found(self):
        """Чек не знайдено → ValueError."""
        use_cases, repo = _build_use_cases()
        repo.find_by_id.return_value = None

        with pytest.raises(ValueError, match="не знайдено"):
            await use_cases.get_receipt_items(uuid4())
