"""Unit tests: ProductUseCases (app/application/use_cases/product_use_cases.py).

Покриває:
- create_product — успіх, дублікат штрих-коду, дублікат артикулу
- update_product — успіх, не знайдено, дублікат штрих-коду, дублікат артикулу
- delete_product — успіх, не знайдено
- get_product — успіх, не знайдено
- search_products — пошук з фільтрами
- get_product_by_barcode — знайдено / не знайдено
"""

from __future__ import annotations

from decimal import Decimal
from unittest.mock import AsyncMock
from uuid import uuid4

import pytest

from app.application.dto.product_dto import ProductCreateDTO, ProductUpdateDTO
from app.application.use_cases.product_use_cases import ProductUseCases
from app.domain.entities.product import Product
from app.domain.value_objects.barcode import Barcode
from app.domain.value_objects.money import Money

VALID_BARCODE = "TEST-BARCODE-001"


def _make_product(
    *,
    name: str = "Тестовий товар",
    barcode: str | None = VALID_BARCODE,
    sku: str = "SKU-001",
    price: Decimal = Decimal("100.00"),
) -> Product:
    return Product(
        id=uuid4(),
        name=name,
        barcode=Barcode(barcode) if barcode else None,
        price=Money(price),
        cost_price=Money(Decimal("50.00")),
        sku=sku,
    )


def _build_use_cases(
    *,
    product_repo: AsyncMock | None = None,
    uow: AsyncMock | None = None,
    event_bus: AsyncMock | None = None,
) -> ProductUseCases:
    return ProductUseCases(
        product_repo=product_repo or AsyncMock(),
        unit_of_work=uow or AsyncMock(),
        event_bus=event_bus or AsyncMock(),
    )


class TestCreateProduct:
    """Тести створення товару."""

    @pytest.mark.asyncio
    async def test_create_product_success(self):
        """Успішне створення товару."""
        saved = _make_product()
        repo = AsyncMock()
        repo.exists_by_barcode.return_value = False
        repo.exists_by_sku.return_value = False
        repo.save.return_value = saved
        uow = AsyncMock()
        event_bus = AsyncMock()

        uc = _build_use_cases(product_repo=repo, uow=uow, event_bus=event_bus)
        dto = await uc.create_product(
            ProductCreateDTO(
                name="Тестовий товар",
                barcode=VALID_BARCODE,
                price=Decimal("100.00"),
                cost_price=Decimal("50.00"),
                sku="SKU-001",
            )
        )

        assert dto.name == "Тестовий товар"
        assert dto.barcode == VALID_BARCODE
        repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()
        published = event_bus.publish.call_args.args[0]
        assert published.name == "Тестовий товар"
        assert str(published.barcode) == VALID_BARCODE

    @pytest.mark.asyncio
    async def test_create_product_duplicate_barcode_raises(self):
        """Помилка при дублюванні штрих-коду."""
        repo = AsyncMock()
        repo.exists_by_barcode.return_value = True

        uc = _build_use_cases(product_repo=repo)
        with pytest.raises(ValueError, match=r"штрих-кодом.*вже існує"):
            await uc.create_product(
                ProductCreateDTO(name="Тест", barcode=VALID_BARCODE)
            )
        repo.save.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_create_product_duplicate_sku_raises(self):
        """Помилка при дублюванні артикулу."""
        repo = AsyncMock()
        repo.exists_by_barcode.return_value = False
        repo.exists_by_sku.return_value = True

        uc = _build_use_cases(product_repo=repo)
        with pytest.raises(ValueError, match=r"артикулом.*вже існує"):
            await uc.create_product(
                ProductCreateDTO(name="Тест", barcode=VALID_BARCODE, sku="SKU-001")
            )
        repo.save.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_create_product_without_barcode_and_sku(self):
        """Створення без штрих-коду та артикулу (без перевірок унікальності)."""
        saved = _make_product(name="Тест", barcode=None, sku="")
        repo = AsyncMock()
        repo.save.return_value = saved

        uc = _build_use_cases(product_repo=repo)
        dto = await uc.create_product(ProductCreateDTO(name="Тест"))

        assert dto.name == "Тест"
        repo.exists_by_barcode.assert_not_awaited()
        repo.exists_by_sku.assert_not_awaited()


class TestUpdateProduct:
    """Тести оновлення товару."""

    @pytest.mark.asyncio
    async def test_update_product_success(self):
        """Успішне оновлення товару."""
        existing = _make_product(name="Стара назва")
        updated = _make_product(name="Нова назва")
        repo = AsyncMock()
        repo.find_by_id.return_value = existing
        repo.update.return_value = updated
        uow = AsyncMock()
        event_bus = AsyncMock()

        uc = _build_use_cases(product_repo=repo, uow=uow, event_bus=event_bus)
        dto = await uc.update_product(
            existing.id, ProductUpdateDTO(name="Нова назва")
        )

        assert dto.name == "Нова назва"
        repo.update.assert_awaited_once()
        uow.commit.assert_awaited_once()
        event_bus.publish.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_update_product_not_found_raises(self):
        """Помилка при оновленні неіснуючого товару."""
        repo = AsyncMock()
        repo.find_by_id.return_value = None

        uc = _build_use_cases(product_repo=repo)
        with pytest.raises(ValueError, match="не знайдено"):
            await uc.update_product(uuid4(), ProductUpdateDTO(name="Нова"))
        repo.update.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_update_product_duplicate_barcode_raises(self):
        """Помилка при зміні штрих-коду на існуючий."""
        existing = _make_product()
        repo = AsyncMock()
        repo.find_by_id.return_value = existing
        repo.exists_by_barcode.return_value = True

        uc = _build_use_cases(product_repo=repo)
        with pytest.raises(ValueError, match=r"штрих-кодом.*вже існує"):
            await uc.update_product(
                existing.id, ProductUpdateDTO(barcode="OTHER-BARCODE-999")
            )
        repo.update.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_update_product_same_barcode_skips_check(self):
        """Якщо штрих-код не змінюється — перевірка унікальності не виконується."""
        existing = _make_product(barcode=VALID_BARCODE)
        updated = _make_product(barcode=VALID_BARCODE)
        repo = AsyncMock()
        repo.find_by_id.return_value = existing
        repo.update.return_value = updated

        uc = _build_use_cases(product_repo=repo)
        dto = await uc.update_product(
            existing.id, ProductUpdateDTO(barcode=VALID_BARCODE)
        )

        assert dto.barcode == VALID_BARCODE
        repo.exists_by_barcode.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_update_product_duplicate_sku_raises(self):
        """Помилка при зміні артикулу на існуючий."""
        existing = _make_product(sku="SKU-001")
        repo = AsyncMock()
        repo.find_by_id.return_value = existing
        repo.exists_by_sku.return_value = True

        uc = _build_use_cases(product_repo=repo)
        with pytest.raises(ValueError, match=r"артикулом.*вже існує"):
            await uc.update_product(existing.id, ProductUpdateDTO(sku="SKU-002"))
        repo.update.assert_not_awaited()


class TestDeleteProduct:
    """Тести видалення товару."""

    @pytest.mark.asyncio
    async def test_delete_product_success(self):
        """Успішне видалення товару."""
        product = _make_product()
        repo = AsyncMock()
        repo.find_by_id.return_value = product
        uow = AsyncMock()
        event_bus = AsyncMock()

        uc = _build_use_cases(product_repo=repo, uow=uow, event_bus=event_bus)
        await uc.delete_product(product.id)

        repo.delete.assert_awaited_once_with(product.id)
        uow.commit.assert_awaited_once()
        published = event_bus.publish.call_args.args[0]
        assert published.product_id == product.id

    @pytest.mark.asyncio
    async def test_delete_product_not_found_raises(self):
        """Помилка при видаленні неіснуючого товару."""
        repo = AsyncMock()
        repo.find_by_id.return_value = None

        uc = _build_use_cases(product_repo=repo)
        with pytest.raises(ValueError, match="не знайдено"):
            await uc.delete_product(uuid4())
        repo.delete.assert_not_awaited()


class TestGetProduct:
    """Тести отримання товару."""

    @pytest.mark.asyncio
    async def test_get_product_success(self):
        """Успішне отримання товару за ID."""
        product = _make_product()
        repo = AsyncMock()
        repo.find_by_id.return_value = product

        uc = _build_use_cases(product_repo=repo)
        dto = await uc.get_product(product.id)

        assert dto.id == product.id
        assert dto.name == "Тестовий товар"

    @pytest.mark.asyncio
    async def test_get_product_not_found_raises(self):
        """Помилка якщо товар не знайдено."""
        repo = AsyncMock()
        repo.find_by_id.return_value = None

        uc = _build_use_cases(product_repo=repo)
        with pytest.raises(ValueError, match="не знайдено"):
            await uc.get_product(uuid4())


class TestSearchProducts:
    """Тести пошуку товарів."""

    @pytest.mark.asyncio
    async def test_search_products_with_filters(self):
        """Пошук з фільтрами та пагінацією."""
        products = [_make_product(), _make_product(name="Другий")]
        repo = AsyncMock()
        repo.search.return_value = (products, 2)

        uc = _build_use_cases(product_repo=repo)
        dtos, total = await uc.search_products(
            query="тест",
            category_id=uuid4(),
            supplier_id=uuid4(),
            is_active=True,
            page=1,
            size=20,
        )

        assert total == 2
        assert len(dtos) == 2
        repo.search.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_search_products_empty(self):
        """Пошук без результатів."""
        repo = AsyncMock()
        repo.search.return_value = ([], 0)

        uc = _build_use_cases(product_repo=repo)
        dtos, total = await uc.search_products(query="нічого")

        assert total == 0
        assert dtos == []


class TestGetProductByBarcode:
    """Тести отримання товару за штрих-кодом."""

    @pytest.mark.asyncio
    async def test_get_product_by_barcode_found(self):
        """Товар знайдено за штрих-кодом."""
        product = _make_product()
        repo = AsyncMock()
        repo.find_by_barcode.return_value = product

        uc = _build_use_cases(product_repo=repo)
        dto = await uc.get_product_by_barcode(VALID_BARCODE)

        assert dto is not None
        assert dto.barcode == VALID_BARCODE

    @pytest.mark.asyncio
    async def test_get_product_by_barcode_not_found(self):
        """Товар не знайдено за штрих-кодом — повертає None."""
        repo = AsyncMock()
        repo.find_by_barcode.return_value = None

        uc = _build_use_cases(product_repo=repo)
        dto = await uc.get_product_by_barcode("NO-SUCH-BARCODE")

        assert dto is None
