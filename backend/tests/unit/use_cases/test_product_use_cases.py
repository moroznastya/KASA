"""Unit tests: Product Use Cases."""

from __future__ import annotations

from uuid import uuid4
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock, PropertyMock

import pytest

from app.domain.use_cases.product_use_cases import (
    CreateProductUseCase,
    UpdateProductUseCase,
    DeleteProductUseCase,
    SearchProductsUseCase,
    PaginatedResult,
)
from app.domain.entities.product import Product
from app.domain.entities.category import Category
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.barcode import Barcode
from app.domain.services.pricing_service import PricingService


# Валідний CODE128 штрих-код (допускає 1-50 ASCII)
VALID_BARCODE = "TEST-BARCODE-001"


def _make_mock_product_repo(**overrides) -> AsyncMock:
    """Створює AsyncMock для product_repo з розумними значеннями за замовчуванням."""
    repo = AsyncMock()
    repo.exists_by_barcode.return_value = False
    repo.exists_by_sku.return_value = False
    for k, v in overrides.items():
        setattr(repo, k, v)
    return repo


class TestCreateProductUseCase:
    """Тести для CreateProductUseCase."""

    @pytest.mark.asyncio
    async def test_create_product_success(self):
        """Успішне створення товару."""
        product_repo = _make_mock_product_repo()
        category_repo = AsyncMock()
        pricing_service = MagicMock(spec=PricingService)
        uow = AsyncMock()

        category_id = uuid4()
        category_repo.find_by_id.return_value = Category(id=category_id, name="Тест")

        expected_product = Product(
            id=uuid4(),
            name="Тестовий товар",
            price=Money(Decimal("100.00")),
            cost_price=Money(Decimal("50.00")),
            category_id=category_id,
        )
        product_repo.save.return_value = expected_product

        use_case = CreateProductUseCase(
            product_repo=product_repo,
            category_repo=category_repo,
            pricing_service=pricing_service,
            uow=uow,
        )
        result = await use_case.execute(
            title="Тестовий товар",
            barcode=VALID_BARCODE,
            price=Decimal("100.00"),
            cost_price=Decimal("50.00"),
            category_id=category_id,
            quantity=Decimal("10"),
        )

        assert result.name == "Тестовий товар"
        product_repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_create_product_empty_title_raises(self):
        """Помилка при пустій назві."""
        use_case = CreateProductUseCase(
            product_repo=AsyncMock(),
            category_repo=AsyncMock(),
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="Назва товару не може бути пустою"):
            await use_case.execute(
                title="", barcode=VALID_BARCODE,
                price=Decimal("100"), cost_price=Decimal("50"),
            )

    @pytest.mark.asyncio
    async def test_create_product_empty_barcode_raises(self):
        """Помилка при пустому штрих-коді."""
        use_case = CreateProductUseCase(
            product_repo=AsyncMock(),
            category_repo=AsyncMock(),
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="Штрих-код товару не може бути пустим"):
            await use_case.execute(
                title="Тест", barcode="",
                price=Decimal("100"), cost_price=Decimal("50"),
            )

    @pytest.mark.asyncio
    async def test_create_product_duplicate_barcode_raises(self):
        """Помилка при дублюванні штрих-коду."""
        product_repo = AsyncMock()
        product_repo.exists_by_barcode.return_value = True
        use_case = CreateProductUseCase(
            product_repo=product_repo,
            category_repo=AsyncMock(),
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="вже існує"):
            await use_case.execute(
                title="Тест", barcode=VALID_BARCODE,
                price=Decimal("100"), cost_price=Decimal("50"),
            )

    @pytest.mark.asyncio
    async def test_create_product_price_less_than_cost_raises(self):
        """Помилка коли ціна менша за собівартість."""
        use_case = CreateProductUseCase(
            product_repo=_make_mock_product_repo(),
            category_repo=AsyncMock(),
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="не може бути меншою за собівартість"):
            await use_case.execute(
                title="Тест", barcode=VALID_BARCODE,
                price=Decimal("50"), cost_price=Decimal("100"),
            )

    @pytest.mark.asyncio
    async def test_create_product_negative_quantity_raises(self):
        """Помилка при від'ємній кількості."""
        use_case = CreateProductUseCase(
            product_repo=_make_mock_product_repo(),
            category_repo=AsyncMock(),
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="не може бути від'ємною"):
            await use_case.execute(
                title="Тест", barcode=VALID_BARCODE,
                price=Decimal("100"), cost_price=Decimal("50"),
                quantity=Decimal("-5"),
            )

    @pytest.mark.asyncio
    async def test_create_product_category_not_found_raises(self):
        """Помилка при неіснуючій категорії."""
        category_repo = AsyncMock()
        category_repo.find_by_id.return_value = None
        use_case = CreateProductUseCase(
            product_repo=_make_mock_product_repo(),
            category_repo=category_repo,
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(
                title="Тест", barcode=VALID_BARCODE,
                price=Decimal("100"), cost_price=Decimal("50"),
                category_id=uuid4(),
            )


class TestUpdateProductUseCase:
    """Тести для UpdateProductUseCase."""

    @pytest.mark.asyncio
    async def test_update_product_success(self):
        """Успішне оновлення товару."""
        product_id = uuid4()
        product_repo = _make_mock_product_repo()
        category_repo = AsyncMock()
        existing = Product(
            id=product_id, name="Стара назва",
            price=Money(Decimal("100")),
            cost_price=Money(Decimal("50")),
        )
        product_repo.find_by_id.return_value = existing
        product_repo.update.return_value = Product(
            id=product_id, name="Нова назва",
            price=Money(Decimal("150")),
            cost_price=Money(Decimal("50")),
        )

        use_case = UpdateProductUseCase(
            product_repo=product_repo,
            category_repo=category_repo,
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        result = await use_case.execute(
            product_id=product_id,
            title="Нова назва",
            price=Decimal("150"),
        )

        assert result.name == "Нова назва"
        product_repo.update.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_update_product_not_found_raises(self):
        """Помилка при оновленні неіснуючого товару."""
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = None
        use_case = UpdateProductUseCase(
            product_repo=product_repo,
            category_repo=AsyncMock(),
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(product_id=uuid4(), title="Нова назва")

    @pytest.mark.asyncio
    async def test_update_product_duplicate_barcode_raises(self):
        """Помилка при дублюванні штрих-коду."""
        product_id = uuid4()
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = Product(
            id=product_id, name="Тест",
            price=Money(Decimal("100")),
        )
        product_repo.exists_by_barcode.return_value = True
        category_repo = AsyncMock()

        use_case = UpdateProductUseCase(
            product_repo=product_repo,
            category_repo=category_repo,
            pricing_service=MagicMock(),
            uow=AsyncMock(),
        )
        with pytest.raises(ValueError, match="вже існує"):
            await use_case.execute(
                product_id=product_id,
                barcode=VALID_BARCODE,
            )


class TestDeleteProductUseCase:
    """Тести для DeleteProductUseCase."""

    @pytest.mark.asyncio
    async def test_delete_product_success(self):
        """Успішне видалення товару."""
        product_id = uuid4()
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = Product(
            id=product_id, name="Видалити",
            price=Money(Decimal("100")),
        )
        uow = AsyncMock()

        use_case = DeleteProductUseCase(product_repo=product_repo, uow=uow)
        await use_case.execute(product_id=product_id)

        product_repo.delete.assert_awaited_with(product_id)
        uow.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_delete_product_not_found_raises(self):
        """Помилка при видаленні неіснуючого товару."""
        product_repo = AsyncMock()
        product_repo.find_by_id.return_value = None
        use_case = DeleteProductUseCase(product_repo=product_repo, uow=AsyncMock())

        with pytest.raises(ValueError, match="не знайдено"):
            await use_case.execute(product_id=uuid4())
        product_repo.delete.assert_not_awaited()


class TestSearchProductsUseCase:
    """Тести для SearchProductsUseCase."""

    @pytest.mark.asyncio
    async def test_search_products_success(self):
        """Успішний пошук товарів."""
        product_repo = AsyncMock()
        product_repo.search.return_value = (
            [Product(id=uuid4(), name="Знайдено", price=Money(Decimal("100")))],
            1,
        )

        use_case = SearchProductsUseCase(product_repo=product_repo)
        result = await use_case.execute(query="Знайдено", page=1, size=20)

        assert result.total == 1
        assert len(result.items) == 1
        assert result.items[0].name == "Знайдено"
        assert result.page == 1
        assert result.size == 20

    @pytest.mark.asyncio
    async def test_search_empty_result(self):
        """Пошук без результатів."""
        product_repo = AsyncMock()
        product_repo.search.return_value = ([], 0)

        use_case = SearchProductsUseCase(product_repo=product_repo)
        result = await use_case.execute(query="Нічого", page=1, size=10)

        assert result.total == 0
        assert len(result.items) == 0

    @pytest.mark.asyncio
    async def test_search_invalid_page_raises(self):
        """Помилка при невалідному номері сторінки."""
        use_case = SearchProductsUseCase(product_repo=AsyncMock())

        with pytest.raises(ValueError, match="Номер сторінки має бути >= 1"):
            await use_case.execute(query="тест", page=0)

    @pytest.mark.asyncio
    async def test_search_invalid_size_raises(self):
        """Помилка при невалідному розмірі сторінки."""
        use_case = SearchProductsUseCase(product_repo=AsyncMock())

        with pytest.raises(ValueError, match="Розмір сторінки має бути від 1 до 100"):
            await use_case.execute(query="тест", size=0)
