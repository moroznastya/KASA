"""
Repository Implementation: SQLAlchemyProductRepository.

Реалізація IProductRepository з використанням SQLAlchemy.
"""

from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.repositories import IProductRepository
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.barcode import Barcode


class SQLAlchemyProductRepository(IProductRepository):
    """
    SQLAlchemy реалізація репозиторію товарів.

    Працює безпосередньо з ORM моделями Product та Barcode.
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, product: Product) -> Product:
        """Зберігає новий товар у БД."""
        self._session.add(product)
        await self._session.flush()
        return product

    async def update(self, product: Product) -> Product:
        """Оновлює існуючий товар у БД."""
        merged = await self._session.merge(product)
        await self._session.flush()
        return merged

    async def find_by_id(self, product_id: UUID) -> Optional[Product]:
        """Знаходить товар за його UUID."""
        stmt = select(Product).where(Product.id == product_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_barcode(self, barcode: str) -> Optional[Product]:
        """
        Знаходить товар за штрих-кодом.

        Спочатку шукає у основному полі barcode таблиці products,
        потім у таблиці barcodes (додаткові штрих-коди).
        """
        # Пошук за основним штрих-кодом
        stmt = select(Product).where(Product.barcode == barcode)
        result = await self._session.execute(stmt)
        product = result.scalar_one_or_none()
        if product is not None:
            return product

        # Пошук у додаткових штрих-кодах
        stmt = (
            select(Product)
            .join(Barcode, Barcode.product_id == Product.id)
            .where(Barcode.barcode == barcode)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_sku(self, sku: str) -> Optional[Product]:
        """Знаходить товар за артикулом (SKU)."""
        stmt = select(Product).where(Product.sku == sku)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

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

        Підтримує фільтри: текстовий пошук (назва, штрих-код, артикул),
        категорія, постачальник, активність.
        """
        base_stmt = select(Product)

        # Фільтри
        if query:
            like_pattern = f"%{query}%"
            base_stmt = base_stmt.where(
                or_(
                    Product.title.ilike(like_pattern),
                    Product.barcode.ilike(like_pattern),
                    Product.sku.ilike(like_pattern),
                )
            )
        if category_id is not None:
            base_stmt = base_stmt.where(Product.category_id == category_id)
        if supplier_id is not None:
            base_stmt = base_stmt.where(Product.supplier_id == supplier_id)

        # Підрахунок загальної кількості
        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        # Пагінація
        offset = (page - 1) * size
        stmt = base_stmt.offset(offset).limit(size)
        result = await self._session.execute(stmt)
        products = list(result.scalars().all())

        return products, total

    async def find_by_category(self, category_id: UUID) -> list[Product]:
        """Знаходить всі товари в категорії."""
        stmt = select(Product).where(Product.category_id == category_id)
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def find_by_supplier(self, supplier_id: UUID) -> list[Product]:
        """Знаходить всі товари постачальника."""
        stmt = select(Product).where(Product.supplier_id == supplier_id)
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def delete(self, product_id: UUID) -> None:
        """Видаляє товар за ID."""
        product = await self.find_by_id(product_id)
        if product is not None:
            await self._session.delete(product)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість товарів."""
        stmt = select(func.count()).select_from(Product)
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def exists_by_barcode(
        self, barcode: str, exclude_id: Optional[UUID] = None
    ) -> bool:
        """Перевіряє, чи існує товар з таким штрих-кодом."""
        stmt = select(Product).where(Product.barcode == barcode)
        if exclude_id is not None:
            stmt = stmt.where(Product.id != exclude_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none() is not None

    async def exists_by_sku(
        self, sku: str, exclude_id: Optional[UUID] = None
    ) -> bool:
        """Перевіряє, чи існує товар з таким артикулом."""
        stmt = select(Product).where(Product.sku == sku)
        if exclude_id is not None:
            stmt = stmt.where(Product.id != exclude_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none() is not None
