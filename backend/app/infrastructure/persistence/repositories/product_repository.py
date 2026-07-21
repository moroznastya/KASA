"""
Infrastructure Layer: ProductRepository — реалізація IProductRepository.

Використовує SQLAlchemy ORM модель ProductModel для роботи з БД.
"""

from __future__ import annotations

import logging
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_, and_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.entities.product import Product
from app.domain.repositories.i_product_repository import IProductRepository
from app.infrastructure.persistence.models import ProductModel

logger = logging.getLogger(__name__)


def _relevance_sort_key(query: str):
    """
    Повертає функцію для сортування товарів за релевантністю до пошукового запиту.

    Пріоритет:
    0 - назва починається з запиту
    1 - назва містить слово, що починається з запиту
    2 - назва просто містить запит
    3 - штрих-код або артикул містять запит
    4 - інше
    """
    q = query.lower().strip()

    def sort_key(product: Product) -> tuple:
        title = product.name.lower()
        barcode = (product.sku or "").lower()

        if title.startswith(q):
            return (0, title)
        if f" {q}" in title:
            return (1, title)
        if q in title:
            return (2, title)
        if q in barcode:
            return (3, title)
        return (4, title)

    return sort_key


class ProductRepository(IProductRepository):
    """
    Репозиторій товарів.

    Реалізує IProductRepository використовуючи SQLAlchemy ORM.
    Маппить між Domain Entity (Product) та ORM Model (ProductModel).
    """

    def __init__(self) -> None:
        """Ініціалізація репозиторію."""
        self._session: AsyncSession | None = None

    @property
    def session(self) -> AsyncSession:
        """Поточна сесія БД."""
        if self._session is None:
            raise RuntimeError("Session not set. Use set_session() or use within UoW.")
        return self._session

    def set_session(self, session: AsyncSession) -> None:
        """Встановлює сесію БД (використовується Unit of Work)."""
        self._session = session

    # ─── CRUD ───────────────────────────────────────────────────────────────

    async def save(self, product: Product) -> Product:
        """Зберігає новий товар."""
        model = self._to_model(product)
        self.session.add(model)
        await self.session.flush()
        return self._to_domain(model)

    async def update(self, product: Product) -> Product:
        """Оновлює існуючий товар."""
        model = await self._get_model(product.id)
        if model is None:
            raise ValueError(f"Product with id {product.id} not found")
        self._update_model(model, product)
        await self.session.flush()
        return self._to_domain(model)

    async def find_by_id(self, product_id: UUID) -> Optional[Product]:
        """Знаходить товар за ID."""
        model = await self._get_model(product_id)
        return self._to_domain(model) if model else None

    async def find_by_barcode(self, barcode: str) -> Optional[Product]:
        """Знаходить товар за штрих-кодом."""
        result = await self.session.execute(
            select(ProductModel).where(ProductModel.barcode == barcode)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_by_sku(self, sku: str) -> Optional[Product]:
        """Знаходить товар за артикулом (SKU)."""
        result = await self.session.execute(
            select(ProductModel).where(ProductModel.sku == sku)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def search(
        self,
        query: Optional[str] = None,
        category_id: Optional[UUID] = None,
        supplier_id: Optional[UUID] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Product], int]:
        """Пошук товарів з фільтрацією, пагінацією та сортуванням за релевантністю."""
        stmt = select(ProductModel)
        count_stmt = select(func.count(ProductModel.id))

        conditions = []
        if query:
            like_pattern = f"%{query}%"
            conditions.append(
                or_(
                    ProductModel.title.ilike(like_pattern),
                    ProductModel.barcode.ilike(like_pattern),
                    ProductModel.sku.ilike(like_pattern),
                )
            )
        if category_id:
            conditions.append(ProductModel.category_id == category_id)
        if supplier_id:
            conditions.append(ProductModel.supplier_id == supplier_id)
        if is_active is not None:
            conditions.append(ProductModel.is_active == is_active)

        if conditions:
            stmt = stmt.where(and_(*conditions))
            count_stmt = count_stmt.where(and_(*conditions))

        # Загальна кількість
        total_result = await self.session.execute(count_stmt)
        total = total_result.scalar() or 0

        # Спочатку отримуємо всі знайдені товари (без пагінації та сортування)
        result = await self.session.execute(stmt)
        models = list(result.scalars().all())

        # Сортуємо за релевантністю на рівні Python
        if query:
            sort_key = _relevance_sort_key(query)
            models.sort(key=lambda m: sort_key(self._to_domain(m)))

        # Пагінація після сортування
        offset = (page - 1) * size
        page_models = models[offset : offset + size]

        return [self._to_domain(m) for m in page_models], total

    async def find_by_category(self, category_id: UUID) -> list[Product]:
        """Знаходить всі товари в категорії."""
        result = await self.session.execute(
            select(ProductModel).where(ProductModel.category_id == category_id)
        )
        return [self._to_domain(m) for m in result.scalars().all()]

    async def find_by_supplier(self, supplier_id: UUID) -> list[Product]:
        """Знаходить всі товари постачальника."""
        result = await self.session.execute(
            select(ProductModel).where(ProductModel.supplier_id == supplier_id)
        )
        return [self._to_domain(m) for m in result.scalars().all()]

    async def delete(self, product_id: UUID) -> None:
        """Видаляє товар за ID."""
        model = await self._get_model(product_id)
        if model:
            await self.session.delete(model)
            await self.session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість товарів."""
        result = await self.session.execute(
            select(func.count(ProductModel.id))
        )
        return result.scalar() or 0

    async def exists_by_barcode(
        self,
        barcode: str,
        exclude_id: Optional[UUID] = None,
    ) -> bool:
        """Перевіряє, чи існує товар з таким штрих-кодом."""
        stmt = select(ProductModel).where(ProductModel.barcode == barcode)
        if exclude_id:
            stmt = stmt.where(ProductModel.id != exclude_id)
        result = await self.session.execute(stmt)
        return result.scalar_one_or_none() is not None

    async def exists_by_sku(
        self,
        sku: str,
        exclude_id: Optional[UUID] = None,
    ) -> bool:
        """Перевіряє, чи існує товар з таким артикулом."""
        stmt = select(ProductModel).where(ProductModel.sku == sku)
        if exclude_id:
            stmt = stmt.where(ProductModel.id != exclude_id)
        result = await self.session.execute(stmt)
        return result.scalar_one_or_none() is not None

    # ─── Маппінг ────────────────────────────────────────────────────────────

    def _to_domain(self, model: ProductModel | None) -> Product | None:
        """Маппить ORM модель в Domain Entity."""
        if model is None:
            return None
        return Product(
            id=model.id,
            name=model.title,
            sku=model.sku or "",
            unit=model.unit or "шт",
            is_active=model.is_active,
            description=model.description or "",
            category_id=model.category_id,
            supplier_id=model.supplier_id,
        )

    def _to_model(self, domain: Product) -> ProductModel:
        """Маппить Domain Entity в ORM модель."""
        return ProductModel(
            id=domain.id,
            title=domain.name,
            sku=domain.sku or None,
            unit=domain.unit or "шт",
            is_active=domain.is_active,
            description=domain.description or None,
            category_id=domain.category_id,
            supplier_id=domain.supplier_id,
        )

    def _update_model(self, model: ProductModel, domain: Product) -> None:
        """Оновлює ORM модель даними з Domain Entity."""
        model.title = domain.name
        model.sku = domain.sku or None
        model.unit = domain.unit or "шт"
        model.is_active = domain.is_active
        model.description = domain.description or None
        model.category_id = domain.category_id
        model.supplier_id = domain.supplier_id

    async def _get_model(self, product_id: UUID) -> Optional[ProductModel]:
        """Отримує ORM модель за ID."""
        result = await self.session.execute(
            select(ProductModel).where(ProductModel.id == product_id)
        )
        return result.scalar_one_or_none()
