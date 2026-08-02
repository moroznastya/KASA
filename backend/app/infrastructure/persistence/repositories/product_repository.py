"""
Repository Implementation: SQLAlchemyProductRepository.

Реалізація IProductRepository з використанням SQLAlchemy.

Оптимізація N+1:
  - to-one (category, supplier)  → joinedload (LEFT OUTER JOIN, 1 запит)
  - to-many (barcodes, images)   → selectinload (окремий запит з IN)
"""

from typing import Optional
from uuid import UUID

from sqlalchemy import func, or_, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import joinedload, selectinload

from app.domain.repositories import IProductRepository
from app.infrastructure.persistence.models.barcode import Barcode
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.product_image import ProductImage

# Спільні опції eager-loading для товару (детальний перегляд)
_PRODUCT_DETAIL_OPTIONS = (
    joinedload(Product.category),
    joinedload(Product.supplier),
    selectinload(Product.barcodes),
    selectinload(Product.images),
)

# Спільні опції eager-loading для списків товарів (без важких to-many)
_PRODUCT_LIST_OPTIONS = (
    joinedload(Product.category),
    joinedload(Product.supplier),
)


class SQLAlchemyProductRepository(IProductRepository):
    """
    SQLAlchemy реалізація репозиторію товарів.

    Працює безпосередньо з ORM моделями Product та Barcode.
    Приймає як ORM-модель Product, так і доменну entity Product
    (конвертується в ORM через _to_orm).
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    @staticmethod
    def _to_orm(product) -> "Product":
        """Конвертує доменну Product entity в ORM Product (якщо це не ORM)."""
        if isinstance(product, Product):
            return product
        from app.domain.value_objects.money import Money
        from app.domain.value_objects.quantity import Quantity

        def _num(value):
            """Money/Quantity/Decimal/float -> float | None."""
            if value is None:
                return None
            if isinstance(value, Money):
                return float(value.amount)
            if isinstance(value, Quantity):
                return float(value.value)
            if hasattr(value, "percent"):  # TaxRate
                return float(value.percent)
            return float(value)

        return Product(
            id=product.id,
            barcode=str(product.barcode) if product.barcode else None,
            sku=product.sku or None,
            title=product.name,
            description=product.description or None,
            price=_num(product.price),
            cost_price=_num(product.cost_price),
            stock=_num(product.stock),
            unit=product.unit or None,
            category_id=product.category_id,
            supplier_id=product.supplier_id,
            tax_rate=_num(product.tax_rate) if product.tax_rate is not None else None,
            is_fiscal=product.is_fiscal,
            fiscal_stock=_num(product.fiscal_stock) or 0,
        )

    async def save(self, product: Product) -> Product:
        """Зберігає новий товар у БД (доменну entity або ORM-модель)."""
        orm = self._to_orm(product)
        self._session.add(orm)
        await self._session.flush()
        return orm

    async def update(self, product: Product) -> Product:
        """Оновлює існуючий товар у БД."""
        orm = self._to_orm(product)
        merged = await self._session.merge(orm)
        await self._session.flush()
        return merged

    async def find_by_id(self, product_id: UUID) -> Optional[Product]:
        """Знаходить товар за його UUID (з категорією, постачальником, штрих-кодами)."""
        stmt = (
            select(Product)
            .where(Product.id == product_id)
            .options(*_PRODUCT_DETAIL_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_barcode(self, barcode: str) -> Optional[Product]:
        """
        Знаходить товар за штрих-кодом.

        Спочатку шукає у основному полі barcode таблиці products,
        потім у таблиці barcodes (додаткові штрих-коди).
        """
        # Пошук за основним штрих-кодом
        stmt = (
            select(Product)
            .where(Product.barcode == barcode)
            .options(*_PRODUCT_DETAIL_OPTIONS)
        )
        result = await self._session.execute(stmt)
        product = result.scalar_one_or_none()
        if product is not None:
            return product

        # Пошук у додаткових штрих-кодах
        stmt = (
            select(Product)
            .join(Barcode, Barcode.product_id == Product.id)
            .where(Barcode.barcode == barcode)
            .options(*_PRODUCT_DETAIL_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_sku(self, sku: str) -> Optional[Product]:
        """Знаходить товар за артикулом (SKU)."""
        stmt = (
            select(Product)
            .where(Product.sku == sku)
            .options(*_PRODUCT_DETAIL_OPTIONS)
        )
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

        # Підрахунок загальної кількості (без eager-loading — рахуємо лише products)
        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        # Пагінація + eager-loading (category, supplier — to-one)
        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_PRODUCT_LIST_OPTIONS)
            .offset(offset)
            .limit(size)
        )
        result = await self._session.execute(stmt)
        products = list(result.scalars().all())

        return products, total

    async def find_by_category(self, category_id: UUID) -> list[Product]:
        """Знаходить всі товари в категорії (з категорією та постачальником)."""
        stmt = (
            select(Product)
            .where(Product.category_id == category_id)
            .options(*_PRODUCT_LIST_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def find_by_supplier(self, supplier_id: UUID) -> list[Product]:
        """Знаходить всі товари постачальника (з категорією та постачальником)."""
        stmt = (
            select(Product)
            .where(Product.supplier_id == supplier_id)
            .options(*_PRODUCT_LIST_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def delete(self, product_id: UUID) -> None:
        """Видаляє товар за ID."""
        from sqlalchemy.exc import IntegrityError
        from app.infrastructure.persistence.models.receipt import ReceiptItem

        # Якщо товар фігурує у чеках (receipt_items) — видалення неможливе
        # (жорстке видалення знищило б історію продажів).
        product = await self.find_by_id(product_id)
        if product is not None:
            try:
                await self._session.delete(product)
                await self._session.flush()
            except IntegrityError as exc:
                await self._session.rollback()
                raise ValueError(
                    f"Товар має пов'язані записи (чеки/накладні) — видалення неможливе"
                ) from exc

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

    # ─── Зображення товару ────────────────────────────────────────────────

    async def add_image(
        self,
        product_id: UUID,
        url: str,
        is_main: bool = False,
    ) -> ProductImage:
        """Додає зображення до товару."""
        # Якщо це головне зображення — знімаємо прапорець з інших
        if is_main:
            await self._session.execute(
                ProductImage.__table__.update()
                .where(ProductImage.product_id == product_id)
                .values(is_main=False)
            )

        # Рахуємо кількість зображень для sort_order
        count_result = await self._session.execute(
            select(func.count())
            .select_from(ProductImage)
            .where(ProductImage.product_id == product_id)
        )
        sort_order = count_result.scalar() or 0

        image = ProductImage(
            product_id=product_id,
            url=url,
            is_main=is_main,
            sort_order=sort_order,
        )
        self._session.add(image)
        await self._session.flush()
        return image

    async def delete_image(self, image_id: UUID) -> None:
        """Видаляє зображення товару."""
        image = await self._session.get(ProductImage, image_id)
        if not image:
            raise ValueError(f"Зображення з ID '{image_id}' не знайдено")
        await self._session.delete(image)
        await self._session.flush()

    # ─── Додаткові штрих-коди ─────────────────────────────────────────────

    async def add_barcode(
        self,
        product_id: UUID,
        barcode: str,
        is_primary: bool = False,
    ) -> Barcode:
        """Додає додатковий штрих-код до товару."""
        # Перевірка унікальності
        existing = await self._session.execute(
            select(Barcode).where(Barcode.barcode == barcode)
        )
        if existing.scalar_one_or_none():
            raise ValueError(f"Штрих-код '{barcode}' вже існує")

        # Якщо це основний — знімаємо прапорець з інших
        if is_primary:
            await self._session.execute(
                Barcode.__table__.update()
                .where(Barcode.product_id == product_id)
                .values(is_primary=False)
            )

        new_barcode = Barcode(
            product_id=product_id,
            barcode=barcode,
            is_primary=is_primary,
        )
        self._session.add(new_barcode)
        await self._session.flush()
        return new_barcode

    async def delete_barcode(self, barcode_id: UUID) -> None:
        """Видаляє додатковий штрих-код товару."""
        barcode_obj = await self._session.get(Barcode, barcode_id)
        if not barcode_obj:
            raise ValueError(f"Штрих-код з ID '{barcode_id}' не знайдено")
        await self._session.delete(barcode_obj)
        await self._session.flush()
