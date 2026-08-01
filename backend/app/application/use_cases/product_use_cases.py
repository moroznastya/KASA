"""
Use Cases для Product (Товар).

Реалізує бізнес-логіку для роботи з товарами:
- CreateProduct: створення нового товару
- UpdateProduct: оновлення існуючого товару
- DeleteProduct: видалення товару
- SearchProducts: пошук товарів з фільтрацією
- GetProduct: отримання товару за ID
"""

from __future__ import annotations

from typing import Optional
from uuid import UUID

from app.domain.entities.product import Product
from app.domain.repositories import IProductRepository
from app.domain.repositories.i_unit_of_work import IUnitOfWork
from app.application.dto.product_dto import ProductDTO, ProductCreateDTO, ProductUpdateDTO
from app.application.mappers.product_mapper import ProductMapper
from app.application.interfaces.i_event_bus import IEventBus
from app.domain.events import (
    ProductCreated,
    ProductUpdated,
    ProductDeleted,
)


class ProductUseCases:
    """
    Use Cases для товарів.

    Використовує Dependency Injection через конструктор.
    Залежності: IProductRepository, IUnitOfWork, IEventBus.
    """

    def __init__(
        self,
        product_repo: IProductRepository,
        unit_of_work: IUnitOfWork,
        event_bus: IEventBus,
    ):
        """
        Ініціалізація Use Cases.

        Args:
            product_repo: Репозиторій товарів.
            unit_of_work: Unit of Work для транзакцій.
            event_bus: Event Bus для публікації подій.
        """
        self._product_repo = product_repo
        self._uow = unit_of_work
        self._event_bus = event_bus

    async def create_product(self, dto: ProductCreateDTO) -> ProductDTO:
        """
        Створює новий товар.

        Args:
            dto: DTO з даними для створення товару.

        Returns:
            ProductDTO створеного товару.

        Raises:
            ValueError: Якщо товар з таким штрих-кодом або артикулом вже існує.
        """
        # Перевіряємо унікальність штрих-коду
        if dto.barcode:
            exists = await self._product_repo.exists_by_barcode(dto.barcode)
            if exists:
                raise ValueError(f"Товар з штрих-кодом '{dto.barcode}' вже існує")

        # Перевіряємо унікальність артикулу
        if dto.sku:
            exists = await self._product_repo.exists_by_sku(dto.sku)
            if exists:
                raise ValueError(f"Товар з артикулом '{dto.sku}' вже існує")

        # Конвертуємо DTO в Entity
        product = ProductMapper.create_dto_to_entity(dto)

        # Зберігаємо через репозиторій
        async with self._uow:
            saved = await self._product_repo.save(product)
            await self._uow.commit()

        # Публікуємо подію ProductCreated
        event = ProductCreated(
            product_id=saved.id,
            name=saved.name,
            barcode=saved.barcode or "",
            category_id=saved.category_id,
            supplier_id=saved.supplier_id,
        )
        await self._event_bus.publish(event)

        return ProductMapper.entity_to_dto(saved)

    async def update_product(self, product_id: UUID, dto: ProductUpdateDTO) -> ProductDTO:
        """
        Оновлює існуючий товар.

        Args:
            product_id: ID товару для оновлення.
            dto: DTO з полями для оновлення.

        Returns:
            ProductDTO оновленого товару.

        Raises:
            ValueError: Якщо товар не знайдено.
        """
        # Знаходимо товар
        product = await self._product_repo.find_by_id(product_id)
        if not product:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")

        # Перевіряємо унікальність штрих-коду (якщо змінюється)
        if dto.barcode is not None and dto.barcode != str(product.barcode or ""):
            exists = await self._product_repo.exists_by_barcode(dto.barcode, exclude_id=product_id)
            if exists:
                raise ValueError(f"Товар з штрих-кодом '{dto.barcode}' вже існує")

        # Перевіряємо унікальність артикулу (якщо змінюється)
        if dto.sku is not None and dto.sku != product.sku:
            exists = await self._product_repo.exists_by_sku(dto.sku, exclude_id=product_id)
            if exists:
                raise ValueError(f"Товар з артикулом '{dto.sku}' вже існує")

        # Застосовуємо оновлення
        updated = ProductMapper.apply_update(product, dto)

        # Зберігаємо через репозиторій
        async with self._uow:
            saved = await self._product_repo.update(updated)
            await self._uow.commit()

        # Публікуємо подію ProductUpdated
        event = ProductUpdated(
            product_id=saved.id,
            changes={},  # TODO: track actual changes
        )
        await self._event_bus.publish(event)

        return ProductMapper.entity_to_dto(saved)

    async def delete_product(self, product_id: UUID) -> None:
        """
        Видаляє товар за ID.

        Args:
            product_id: ID товару для видалення.

        Raises:
            ValueError: Якщо товар не знайдено.
        """
        product = await self._product_repo.find_by_id(product_id)
        if not product:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")

        async with self._uow:
            await self._product_repo.delete(product_id)
            await self._uow.commit()

        # Публікуємо подію ProductDeleted
        event = ProductDeleted(product_id=product_id)
        await self._event_bus.publish(event)

    async def get_product(self, product_id: UUID) -> ProductDTO:
        """
        Отримує товар за ID.

        Args:
            product_id: ID товару.

        Returns:
            ProductDTO товару.

        Raises:
            ValueError: Якщо товар не знайдено.
        """
        product = await self._product_repo.find_by_id(product_id)
        if not product:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")
        return ProductMapper.entity_to_dto(product)

    async def search_products(
        self,
        query: Optional[str] = None,
        category_id: Optional[UUID] = None,
        supplier_id: Optional[UUID] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[ProductDTO], int]:
        """
        Пошук товарів з фільтрацією та пагінацією.

        Args:
            query: Текстовий пошук (назва, штрих-код, артикул).
            category_id: Фільтр за категорією.
            supplier_id: Фільтр за постачальником.
            is_active: Фільтр за активністю.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список ProductDTO, загальна кількість).
        """
        products, total = await self._product_repo.search(
            query=query,
            category_id=category_id,
            supplier_id=supplier_id,
            is_active=is_active,
            page=page,
            size=size,
        )
        return [ProductMapper.entity_to_dto(p) for p in products], total

    async def get_product_by_barcode(self, barcode: str) -> Optional[ProductDTO]:
        """
        Отримує товар за штрих-кодом.

        Args:
            barcode: Штрих-код товару.

        Returns:
            ProductDTO або None, якщо не знайдено.
        """
        product = await self._product_repo.find_by_barcode(barcode)
        if not product:
            return None
        return ProductMapper.entity_to_dto(product)

    # ─── Зображення товару ────────────────────────────────────────────────

    async def add_product_image(
        self,
        product_id: UUID,
        url: str,
        is_main: bool = False,
    ):
        """
        Додає зображення до товару.

        Args:
            product_id: ID товару.
            url: URL або шлях до файлу зображення.
            is_main: Чи є зображення головним.

        Returns:
            Об'єкт зображення (ProductImage).

        Raises:
            ValueError: Якщо товар не знайдено.
        """
        product = await self._product_repo.find_by_id(product_id)
        if not product:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")
        return await self._product_repo.add_image(product_id, url, is_main)

    async def delete_product_image(self, image_id: UUID) -> None:
        """
        Видаляє зображення товару.

        Args:
            image_id: ID зображення.

        Raises:
            ValueError: Якщо зображення не знайдено.
        """
        await self._product_repo.delete_image(image_id)

    # ─── Додаткові штрих-коди ─────────────────────────────────────────────

    async def add_product_barcode(
        self,
        product_id: UUID,
        barcode: str,
        is_primary: bool = False,
    ):
        """
        Додає додатковий штрих-код до товару.

        Args:
            product_id: ID товару.
            barcode: Значення штрих-коду.
            is_primary: Чи є штрих-код основним.

        Returns:
            Об'єкт штрих-коду (Barcode).

        Raises:
            ValueError: Якщо товар не знайдено або штрих-код вже існує.
        """
        product = await self._product_repo.find_by_id(product_id)
        if not product:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")
        return await self._product_repo.add_barcode(product_id, barcode, is_primary)

    async def delete_product_barcode(self, barcode_id: UUID) -> None:
        """
        Видаляє додатковий штрих-код товару.

        Args:
            barcode_id: ID штрих-коду.

        Raises:
            ValueError: Якщо штрих-код не знайдено.
        """
        await self._product_repo.delete_barcode(barcode_id)
