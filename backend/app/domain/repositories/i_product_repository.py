"""
Repository Interface: IProductRepository.

Визначає контракт для роботи з товарами.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from typing import Optional, Protocol
from uuid import UUID

from ..entities.product import Product


class IProductRepository(Protocol):
    """
    Інтерфейс репозиторію товарів.

    Визначає CRUD операції та методи пошуку для Product entity.
    """

    async def save(self, product: Product) -> Product:
        """
        Зберігає новий товар.

        Args:
            product: Entity товару для збереження.

        Returns:
            Збережений товар з оновленими полями (ID, дати).
        """
        ...

    async def update(self, product: Product) -> Product:
        """
        Оновлює існуючий товар.

        Args:
            product: Entity товару з оновленими даними.

        Returns:
            Оновлений товар.

        Raises:
            ProductNotFound: Якщо товар не знайдено.
        """
        ...

    async def find_by_id(self, product_id: UUID) -> Optional[Product]:
        """
        Знаходить товар за ID.

        Args:
            product_id: UUID товару.

        Returns:
            Product або None, якщо не знайдено.
        """
        ...

    async def find_by_barcode(self, barcode: str) -> Optional[Product]:
        """
        Знаходить товар за штрих-кодом.

        Args:
            barcode: Штрих-код товару.

        Returns:
            Product або None, якщо не знайдено.
        """
        ...

    async def find_by_sku(self, sku: str) -> Optional[Product]:
        """
        Знаходить товар за артикулом (SKU).

        Args:
            sku: Артикул товару.

        Returns:
            Product або None, якщо не знайдено.
        """
        ...

    async def search(
        self,
        query: Optional[str] = None,
        category_id: Optional[UUID] = None,
        supplier_id: Optional[UUID] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Product], int]:
        """
        Пошук товарів з фільтрацією та пагінацією.

        Args:
            query: Текстовий пошук (назва, штрих-код, артикул).
            category_id: Фільтр за категорією.
            supplier_id: Фільтр за постачальником.
            is_active: Фільтр за активністю.
            page: Номер сторінки (1-based).
            size: Кількість на сторінці.

        Returns:
            Кортеж (список товарів, загальна кількість).
        """
        ...

    async def find_by_category(self, category_id: UUID) -> list[Product]:
        """
        Знаходить всі товари в категорії.

        Args:
            category_id: UUID категорії.

        Returns:
            Список товарів.
        """
        ...

    async def find_by_supplier(self, supplier_id: UUID) -> list[Product]:
        """
        Знаходить всі товари постачальника.

        Args:
            supplier_id: UUID постачальника.

        Returns:
            Список товарів.
        """
        ...

    async def delete(self, product_id: UUID) -> None:
        """
        Видаляє товар за ID.

        Args:
            product_id: UUID товару.

        Raises:
            ProductNotFound: Якщо товар не знайдено.
        """
        ...

    async def count(self) -> int:
        """
        Повертає загальну кількість товарів.

        Returns:
            Кількість товарів.
        """
        ...

    async def exists_by_barcode(self, barcode: str, exclude_id: Optional[UUID] = None) -> bool:
        """
        Перевіряє, чи існує товар з таким штрих-кодом.

        Args:
            barcode: Штрих-код для перевірки.
            exclude_id: ID товару для виключення (при оновленні).

        Returns:
            True якщо існує.
        """
        ...

    async def exists_by_sku(self, sku: str, exclude_id: Optional[UUID] = None) -> bool:
        """
        Перевіряє, чи існує товар з таким артикулом.

        Args:
            sku: Артикул для перевірки.
            exclude_id: ID товару для виключення (при оновленні).

        Returns:
            True якщо існує.
        """
        ...

    async def add_image(
        self,
        product_id: UUID,
        url: str,
        is_main: bool = False,
    ):
        """
        Додає зображення до товару.

        Args:
            product_id: UUID товару.
            url: URL або шлях до файлу зображення.
            is_main: Чи є зображення головним.

        Returns:
            ProductImage (ORM модель).

        Raises:
            ValueError: Якщо товар не знайдено.
        """
        ...

    async def delete_image(self, image_id: UUID) -> None:
        """
        Видаляє зображення товару.

        Args:
            image_id: UUID зображення.

        Raises:
            ValueError: Якщо зображення не знайдено.
        """
        ...

    async def add_barcode(
        self,
        product_id: UUID,
        barcode: str,
        is_primary: bool = False,
    ):
        """
        Додає додатковий штрих-код до товару.

        Args:
            product_id: UUID товару.
            barcode: Значення штрих-коду.
            is_primary: Чи є штрих-код основним.

        Returns:
            Barcode (ORM модель).

        Raises:
            ValueError: Якщо штрих-код вже існує або товар не знайдено.
        """
        ...

    async def delete_barcode(self, barcode_id: UUID) -> None:
        """
        Видаляє додатковий штрих-код товару.

        Args:
            barcode_id: UUID штрих-коду.

        Raises:
            ValueError: Якщо штрих-код не знайдено.
        """
        ...
