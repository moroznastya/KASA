"""Unit tests: Receipt Use Cases."""

from __future__ import annotations

from uuid import uuid4
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock

import pytest

from app.domain.use_cases.receipt_use_cases import (
    CreateReceiptUseCase,
    ReturnReceiptUseCase,
    ReceiptItemCreate,
    ReturnItemCreate,
)
from app.domain.entities.receipt import Receipt, ReceiptItem, PaymentMethod
from app.domain.entities.product import Product
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate


class TestCreateReceiptUseCase:
    """Тести для CreateReceiptUseCase."""

    @pytest.mark.asyncio
    async def test_create_receipt_success(self):
        """Успішне створення чеку продажу."""
        receipt_repo = AsyncMock()
        product_repo = AsyncMock()
        stock_service = MagicMock()
        document_service = MagicMock()
        uow = AsyncMock()

        product_id = uuid4()
        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("50")),
            unit="шт",
        )
        product_repo.find_by_id.return_value = product
        stock_service.check_sufficient.return_value = True

        expected_receipt = Receipt(id=uuid4())
        receipt_repo.save.return_value = expected_receipt

        use_case = CreateReceiptUseCase(
            receipt_repo=receipt_repo,
            product_repo=product_repo,
            stock_service=stock_service,
            document_service=document_service,
            uow=uow,
        )

        items = [
            ReceiptItemCreate(
                product_id=product_id,
                quantity=Decimal("2"),
                price=Decimal("100.00"),
            )
        ]

        result = await use_case.execute(items=items, payment_method="cash")

        assert result.id is not None
        receipt_repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_create_receipt_empty_items_raises(self):
        """Помилка при пустому списку позицій."""
        use_case = CreateReceiptUseCase(
            receipt_repo=AsyncMock(),
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="хоча б одну позицію"):
            await use_case.execute(items=[], payment_method="cash")

    @pytest.mark.asyncio
    async def test_create_receipt_invalid_payment_method_raises(self):
        """Помилка при невідомому способі оплати."""
        use_case = CreateReceiptUseCase(
            receipt_repo=AsyncMock(),
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="Невідомий спосіб оплати"):
            await use_case.execute(
                items=[ReceiptItemCreate(product_id=uuid4(), quantity=Decimal("1"), price=Decimal("100"))],
                payment_method="bitcoin",
            )

    @pytest.mark.asyncio
    async def test_create_receipt_product_not_found_raises(self):
        """Помилка коли товар не знайдено."""
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = None

        use_case = CreateReceiptUseCase(
            receipt_repo=AsyncMock(),
            product_repo=product_repo,
            stock_service=MagicMock(),
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(
                items=[ReceiptItemCreate(product_id=uuid4(), quantity=Decimal("1"), price=Decimal("100"))],
                payment_method="cash",
            )

    @pytest.mark.asyncio
    async def test_create_receipt_insufficient_stock_raises(self):
        """Помилка при недостатній кількості товару."""
        product_id = uuid4()
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("1")),
            unit="шт",
        )
        stock_service = MagicMock()
        stock_service.check_sufficient.return_value = False

        use_case = CreateReceiptUseCase(
            receipt_repo=AsyncMock(),
            product_repo=product_repo,
            stock_service=stock_service,
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="Недостатньо"):
            await use_case.execute(
                items=[ReceiptItemCreate(product_id=product_id, quantity=Decimal("10"), price=Decimal("100"))],
                payment_method="cash",
            )

    @pytest.mark.asyncio
    async def test_create_receipt_negative_quantity_raises(self):
        """Помилка при від'ємній кількості."""
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = Product(
            id=uuid4(), name="Товар", unit="шт",
            price=Money(Decimal("100")), stock=Quantity(Decimal("10")),
        )

        use_case = CreateReceiptUseCase(
            receipt_repo=AsyncMock(),
            product_repo=product_repo,
            stock_service=MagicMock(),
            document_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="повинна бути додатною"):
            await use_case.execute(
                items=[ReceiptItemCreate(product_id=uuid4(), quantity=Decimal("-1"), price=Decimal("100"))],
                payment_method="cash",
            )


class TestReturnReceiptUseCase:
    """Тести для ReturnReceiptUseCase."""

    @pytest.mark.asyncio
    async def test_return_receipt_success(self):
        """Успішне повернення товару."""
        receipt_repo = AsyncMock()
        product_repo = AsyncMock()
        stock_service = MagicMock()
        uow = AsyncMock()

        original_id = uuid4()
        product_id = uuid4()
        original_receipt = Receipt(
            id=original_id,
            payment_method=PaymentMethod.CASH,
        )
        receipt_repo.find_by_id.return_value = original_receipt

        product = Product(
            id=product_id,
            name="Товар",
            price=Money(Decimal("100")),
            stock=Quantity(Decimal("5")),
            unit="шт",
        )
        product_repo.find_by_id.return_value = product

        expected_return_receipt = Receipt(id=uuid4())
        receipt_repo.save.return_value = expected_return_receipt

        use_case = ReturnReceiptUseCase(
            receipt_repo=receipt_repo,
            product_repo=product_repo,
            stock_service=stock_service,
            uow=uow,
        )

        items = [
            ReturnItemCreate(
                product_id=product_id,
                quantity=Decimal("2"),
                price=Decimal("100.00"),
            )
        ]

        result = await use_case.execute(
            original_receipt_id=original_id,
            items=items,
        )

        assert result.id is not None
        receipt_repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_return_receipt_original_not_found_raises(self):
        """Помилка при неіснуючому оригінальному чеку."""
        receipt_repo = AsyncMock()
        receipt_repo.find_by_id.return_value = None

        use_case = ReturnReceiptUseCase(
            receipt_repo=receipt_repo,
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(
                original_receipt_id=uuid4(),
                items=[ReturnItemCreate(product_id=uuid4(), quantity=Decimal("1"), price=Decimal("100"))],
            )

    @pytest.mark.asyncio
    async def test_return_receipt_empty_items_raises(self):
        """Помилка при пустому списку повернення."""
        receipt_repo = AsyncMock()
        receipt_repo.find_by_id.return_value = Receipt(id=uuid4())

        use_case = ReturnReceiptUseCase(
            receipt_repo=receipt_repo,
            product_repo=AsyncMock(),
            stock_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="хоча б одну позицію"):
            await use_case.execute(
                original_receipt_id=uuid4(),
                items=[],
            )

    @pytest.mark.asyncio
    async def test_return_receipt_product_not_found_raises(self):
        """Помилка коли товар для повернення не знайдено."""
        receipt_repo = AsyncMock()
        receipt_repo.find_by_id.return_value = Receipt(id=uuid4())

        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = None

        use_case = ReturnReceiptUseCase(
            receipt_repo=receipt_repo,
            product_repo=product_repo,
            stock_service=MagicMock(),
            uow=AsyncMock(),
        )

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(
                original_receipt_id=uuid4(),
                items=[ReturnItemCreate(product_id=uuid4(), quantity=Decimal("1"), price=Decimal("100"))],
            )
