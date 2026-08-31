"""
Контракт модуля Product (Товари).

Визначає інтерфейс для роботи з товарами, категоріями та штрих-кодами.
Всі сервіси, які працюють з товарами, мають реалізовувати цей Protocol.
"""

from typing import TYPE_CHECKING, Protocol
from uuid import UUID

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.product import Product
    from app.schemas.product import ProductCreate, ProductSearchParams, ProductUpdate


class ProductModuleInterface(Protocol):
    """
    Інтерфейс модуля товарів.

    Визначає контракт для CRUD операцій з товарами,
    пошуку за штрих-кодом та фільтрації.

    Модулі, які залежать від ProductModule, використовують
    цей Protocol замість прямої залежності від ProductService.
    """

    # ─── Події, які публікує ─────────────────────────────────────────────
    # publishes:
    #   - "product.created"   — коли створено новий товар
    #   - "product.updated"   — коли оновлено дані товару
    #   - "product.deleted"   — коли видалено товар
    #
    # ─── Події, на які підписується ───────────────────────────────────────
    # subscribes:
    #   - "stock.changed"     — для оновлення кешу ціни/залишку

    # ─── CRUD: Create ─────────────────────────────────────────────────────

    async def create_product(self, data: "ProductCreate") -> "Product":
        """
        Створює новий товар.

        Після створення публікує подію "product.created".

        Args:
            data: Дані для створення товару (Pydantic схема).

        Returns:
            Створений об'єкт Product (ORM модель).

        Raises:
            ProductAlreadyExists: Якщо товар з таким штрих-кодом або артикулом вже існує.
        """
        ...

    # ─── CRUD: Read ───────────────────────────────────────────────────────

    async def get_product_by_id(self, product_id: UUID) -> "Product":
        """
        Отримує товар за ID.

        Args:
            product_id: UUID товару.

        Returns:
            Об'єкт Product.

        Raises:
            ProductNotFound: Якщо товар не знайдено.
        """
        ...

    async def get_product_by_barcode(self, barcode: str) -> "Product":
        """
        Отримує товар за штрих-кодом.

        Шукає спочатку в основному полі barcode товару,
        потім у таблиці додаткових штрих-кодів (Barcode).

        Args:
            barcode: Штрих-код для пошуку.

        Returns:
            Об'єкт Product.

        Raises:
            ProductNotFound: Якщо товар не знайдено.
        """
        ...

    async def search_products(
        self,
        params: "ProductSearchParams",
    ) -> tuple[list["Product"], int]:
        """
        Пошук товарів з фільтрацією та пагінацією.

        Підтримує фільтрацію за:
        - Текстовим пошуком (назва, штрих-код, артикул)
        - Категорією
        - Постачальником
        - Ціновим діапазоном
        - Типом товару (ваговий/штучний)

        Args:
            params: Параметри пошуку та фільтрації.

        Returns:
            Кортеж (список товарів, загальна кількість).
        """
        ...

    # ─── CRUD: Update ─────────────────────────────────────────────────────

    async def update_product(self, product_id: UUID, data: "ProductUpdate") -> "Product":
        """
        Оновлює дані товару.

        Після оновлення публікує подію "product.updated".
        Оновлюються тільки передані поля (часткове оновлення).

        Args:
            product_id: UUID товару.
            data: Дані для оновлення.

        Returns:
            Оновлений об'єкт Product.
        """
        ...

    # ─── CRUD: Delete ─────────────────────────────────────────────────────

    async def delete_product(self, product_id: UUID) -> None:
        """
        Видаляє товар за ID.

        Після видалення публікує подію "product.deleted".

        Args:
            product_id: UUID товару.
        """
        ...

    # ─── Додаткові методи ─────────────────────────────────────────────────

    async def get_products_by_category(self, category_id: UUID) -> list["Product"]:
        """
        Отримує всі товари в категорії.

        Args:
            category_id: UUID категорії.

        Returns:
            Список товарів у категорії.
        """
        ...

    async def get_products_by_supplier(self, supplier_id: UUID) -> list["Product"]:
        """
        Отримує всі товари постачальника.

        Args:
            supplier_id: UUID постачальника.

        Returns:
            Список товарів постачальника.
        """
        ...
