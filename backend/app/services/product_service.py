"""
Сервіс для роботи з товарами (Product).

Забезпечує:
  - CRUD операції
  - Пошук за штрих-кодом, назвою, артикулом
  - Фільтрацію за категорією, постачальником, ціною
  - Оновлення залишків на складі
  - Управління зображеннями та додатковими штрих-кодами
"""

from decimal import Decimal
from typing import Optional
from uuid import UUID

from fastapi import HTTPException, status
from sqlalchemy import select, func, or_
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.product import Product
from app.models.barcode import Barcode
from app.models.product_image import ProductImage
from app.schemas.product import ProductCreate, ProductUpdate, ProductSearchParams


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
        title = product.title.lower()
        barcode = (product.barcode or product.sku or "").lower()

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


class ProductService:
    """
    Сервіс для управління товарами.

    Вся бізнес-логіка роботи з товарами знаходиться тут.
    """

    def __init__(self, session: AsyncSession):
        """Ініціалізація сервісу з асинхронною сесією БД."""
        self.session = session

    # ─── CRUD: Create ────────────────────────────────────────────────────────

    async def create_product(self, data: ProductCreate) -> Product:
        """
        Створює новий товар.

        Args:
            data: Дані для створення товару.

        Returns:
            Створений об'єкт Product.

        Raises:
            HTTPException 409: Якщо товар з таким штрих-кодом або артикулом вже існує.
        """
        # Перевірка унікальності штрих-коду
        if data.barcode:
            existing = await self.session.execute(
                select(Product).where(Product.barcode == data.barcode)
            )
            if existing.scalar_one_or_none():
                raise HTTPException(
                    status_code=status.HTTP_409_CONFLICT,
                    detail=f"Товар зі штрих-кодом '{data.barcode}' вже існує",
                )

        # Перевірка унікальності артикулу
        if data.sku:
            existing = await self.session.execute(
                select(Product).where(Product.sku == data.sku)
            )
            if existing.scalar_one_or_none():
                raise HTTPException(
                    status_code=status.HTTP_409_CONFLICT,
                    detail=f"Товар з артикулом '{data.sku}' вже існує",
                )

        # Автоматичний розрахунок націнки, якщо не задана
        markup_value = data.markup
        if markup_value is None and data.cost_price and data.price and data.cost_price > 0:
            markup_value = round((data.price - data.cost_price) / data.cost_price * 100, 2)

        # Створюємо товар
        product = Product(
            barcode=data.barcode,
            sku=data.sku,
            title=data.title,
            description=data.description,
            price=data.price,
            cost_price=data.cost_price,
            stock=data.stock if data.stock is not None else Decimal("0.000"),
            recommended_qty=data.recommended_qty,
            uktzed=data.uktzed,
            scan_excise=data.scan_excise,
            tax_rate=data.tax_rate,
            tax_group=data.tax_group,
            is_weight=data.is_weight,
            markup=markup_value,
            unit=data.unit,
            category_id=data.category_id,
            supplier_id=data.supplier_id,
        )
        self.session.add(product)
        await self.session.flush()
        return product

    # ─── CRUD: Read ──────────────────────────────────────────────────────────

    async def get_product_by_id(self, product_id: UUID) -> Product:
        """
        Отримує товар за ID.

        Args:
            product_id: UUID товару.

        Returns:
            Об'єкт Product.

        Raises:
            HTTPException 404: Якщо товар не знайдено.
        """
        result = await self.session.execute(
            select(Product).where(Product.id == product_id)
        )
        product = result.scalar_one_or_none()
        if not product:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Товар з ID '{product_id}' не знайдено",
            )
        return product

    async def get_product_by_barcode(self, barcode: str) -> Product:
        """
        Отримує товар за штрих-кодом (шукає в product.barcode та barcodes).

        Args:
            barcode: Штрих-код для пошуку.

        Returns:
            Об'єкт Product.

        Raises:
            HTTPException 404: Якщо товар не знайдено.
        """
        # Спочатку шукаємо в основному полі barcode
        result = await self.session.execute(
            select(Product).where(Product.barcode == barcode)
        )
        product = result.scalar_one_or_none()

        # Якщо не знайшли, шукаємо в таблиці Barcode
        if not product:
            result = await self.session.execute(
                select(Barcode).where(Barcode.barcode == barcode)
            )
            barcode_obj = result.scalar_one_or_none()
            if barcode_obj:
                product = await self.get_product_by_id(barcode_obj.product_id)

        if not product:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Товар зі штрих-кодом '{barcode}' не знайдено",
            )
        return product

    async def search_products(
        self,
        params: ProductSearchParams,
    ) -> tuple[list[Product], int]:
        """
        Пошук товарів з фільтрацією, пагінацією та сортуванням за релевантністю.

        Args:
            params: Параметри пошуку та фільтрації.

        Returns:
            Кортеж (список товарів, загальна кількість).
        """
        # Базовий запит
        query = select(Product)
        count_query = select(func.count(func.distinct(Product.id)))

        # Фільтр за текстовим пошуком (назва, штрих-код, артикул)
        if params.query:
            search_pattern = f"%{params.query}%"
            # Пошук за назвою, штрих-кодом, артикулом та додатковими кодами
            query = (
                query.outerjoin(Product.barcodes)
                .where(
                    or_(
                        Product.title.ilike(search_pattern),
                        Product.barcode.ilike(search_pattern),
                        Product.sku.ilike(search_pattern),
                        Barcode.barcode.ilike(search_pattern),
                    )
                )
                .distinct()
            )
            # Для підрахунку — також JOIN, але з DISTINCT, щоб уникнути множення рядків
            count_query = (
                count_query.outerjoin(Product.barcodes)
                .where(
                    or_(
                        Product.title.ilike(search_pattern),
                        Product.barcode.ilike(search_pattern),
                        Product.sku.ilike(search_pattern),
                        Barcode.barcode.ilike(search_pattern),
                    )
                )
            )

        # Фільтр за штрих-кодом (точний збіг)
        if params.barcode:
            query = query.where(Product.barcode == params.barcode)
            count_query = count_query.where(Product.barcode == params.barcode)

        # Фільтр за категорією
        if params.category_id:
            query = query.where(Product.category_id == params.category_id)
            count_query = count_query.where(Product.category_id == params.category_id)

        # Фільтр за постачальником
        if params.supplier_id:
            query = query.where(Product.supplier_id == params.supplier_id)
            count_query = count_query.where(Product.supplier_id == params.supplier_id)

        # Фільтр за ціною
        if params.min_price is not None:
            query = query.where(Product.price >= params.min_price)
            count_query = count_query.where(Product.price >= params.min_price)
        if params.max_price is not None:
            query = query.where(Product.price <= params.max_price)
            count_query = count_query.where(Product.price <= params.max_price)

        # Фільтр вагових товарів
        if params.is_weight is not None:
            query = query.where(Product.is_weight == params.is_weight)
            count_query = count_query.where(Product.is_weight == params.is_weight)

        # Отримуємо загальну кількість
        total_result = await self.session.execute(count_query)
        total = total_result.scalar() or 0

        # Отримуємо всі знайдені товари (без сортування в SQL)
        result = await self.session.execute(query)
        products = list(result.scalars().all())

        # Сортуємо за релевантністю на рівні Python
        if params.query:
            sort_key = _relevance_sort_key(params.query)
            products.sort(key=sort_key)
        else:
            # Без пошукового запиту — за алфавітом
            products.sort(key=lambda p: p.title.lower())

        # Пагінація після сортування
        offset = (params.page - 1) * params.size
        page_products = products[offset : offset + params.size]

        return page_products, total

    # ─── CRUD: Update ────────────────────────────────────────────────────────

    async def update_product(self, product_id: UUID, data: ProductUpdate) -> Product:
        """
        Оновлює дані товару.

        Args:
            product_id: UUID товару.
            data: Дані для оновлення (часткове оновлення).

        Returns:
            Оновлений об'єкт Product.
        """
        product = await self.get_product_by_id(product_id)

        # Перевірка унікальності штрих-коду (якщо змінюється)
        if data.barcode is not None and data.barcode != product.barcode:
            existing = await self.session.execute(
                select(Product).where(
                    Product.barcode == data.barcode,
                    Product.id != product_id,
                )
            )
            if existing.scalar_one_or_none():
                raise HTTPException(
                    status_code=status.HTTP_409_CONFLICT,
                    detail=f"Товар зі штрих-кодом '{data.barcode}' вже існує",
                )

        # Перевірка унікальності артикулу (якщо змінюється)
        if data.sku is not None and data.sku != product.sku:
            existing = await self.session.execute(
                select(Product).where(
                    Product.sku == data.sku,
                    Product.id != product_id,
                )
            )
            if existing.scalar_one_or_none():
                raise HTTPException(
                    status_code=status.HTTP_409_CONFLICT,
                    detail=f"Товар з артикулом '{data.sku}' вже існує",
                )

        # Оновлюємо тільки передані поля
        update_data = data.model_dump(exclude_unset=True)

        # Автоматичний розрахунок націнки, якщо змінили ціну або собівартість
        # і націнка не передана явно
        if 'markup' not in update_data:
            cost_price = update_data.get('cost_price', product.cost_price)
            price = update_data.get('price', product.price)
            if cost_price and price and cost_price > 0:
                update_data['markup'] = round((price - cost_price) / cost_price * 100, 2)

        for field, value in update_data.items():
            setattr(product, field, value)

        await self.session.flush()
        return product

    # ─── CRUD: Delete ────────────────────────────────────────────────────────

    async def delete_product(self, product_id: UUID) -> None:
        """
        Видаляє товар за ID.

        Товар можна видалити тільки якщо його залишок (stock) дорівнює 0 або None.

        Args:
            product_id: UUID товару.

        Raises:
            HTTPException 400: Якщо товар має ненульовий залишок на складі.
        """
        product = await self.get_product_by_id(product_id)

        # Перевірка: товар можна видалити тільки з нульовим залишком
        if product.stock is not None and product.stock != 0:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=(
                    f"Неможливо видалити товар '{product.title}': "
                    f"залишок на складі {product.stock} шт. "
                    f"Спочатку списати залишок до нуля."
                ),
            )

        await self.session.delete(product)
        await self.session.flush()

    # ─── Оновлення залишків ──────────────────────────────────────────────────

    async def update_stock(
        self,
        product_id: UUID,
        quantity_change: Decimal,
    ) -> Product:
        """
        Оновлює залишок товару на складі.

        Args:
            product_id: UUID товару.
            quantity_change: Зміна кількості (додатна — збільшення, від'ємна — зменшення).

        Returns:
            Оновлений об'єкт Product.

        Raises:
            HTTPException 400: Якщо недостатньо товару на складі.
        """
        product = await self.get_product_by_id(product_id)

        # Перевіряємо, чи достатньо товару при зменшенні
        if quantity_change < 0 and product.stock is not None:
            if product.stock + quantity_change < 0:
                raise HTTPException(
                    status_code=status.HTTP_400_BAD_REQUEST,
                    detail=(
                        f"Недостатньо товару '{product.title}' на складі. "
                        f"Доступно: {product.stock}, потрібно: {abs(quantity_change)}"
                    ),
                )

        # Оновлюємо залишок
        if product.stock is None:
            product.stock = quantity_change
        else:
            product.stock += quantity_change

        await self.session.flush()
        return product

    # ─── Зображення ─────────────────────────────────────────────────────────

    async def add_image(self, product_id: UUID, url: str, is_main: bool = False) -> ProductImage:
        """Додає зображення до товару."""
        # Якщо це головне зображення, знімаємо прапорець з інших
        if is_main:
            await self.session.execute(
                ProductImage.__table__.update()
                .where(ProductImage.product_id == product_id)
                .values(is_main=False)
            )

        product = await self.get_product_by_id(product_id)
        # Рахуємо кількість зображень окремим запитом (уникнення MissingGreenlet)
        count_result = await self.session.execute(
            select(func.count()).select_from(ProductImage).where(ProductImage.product_id == product_id)
        )
        sort_order = count_result.scalar() or 0

        image = ProductImage(
            product_id=product_id,
            url=url,
            is_main=is_main,
            sort_order=sort_order,
        )
        self.session.add(image)
        await self.session.flush()
        return image

    async def delete_image(self, image_id: UUID) -> None:
        """Видаляє зображення товару."""
        image = await self.session.get(ProductImage, image_id)
        if not image:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Зображення не знайдено",
            )
        await self.session.delete(image)
        await self.session.flush()

    # ─── Додаткові штрих-коди ───────────────────────────────────────────────

    async def add_barcode(self, product_id: UUID, barcode: str, is_primary: bool = False) -> Barcode:
        """Додає додатковий штрих-код до товару."""
        # Перевірка унікальності
        existing = await self.session.execute(
            select(Barcode).where(Barcode.barcode == barcode)
        )
        if existing.scalar_one_or_none():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=f"Штрих-код '{barcode}' вже існує",
            )

        # Якщо це основний, знімаємо прапорець з інших
        if is_primary:
            await self.session.execute(
                Barcode.__table__.update()
                .where(Barcode.product_id == product_id)
                .values(is_primary=False)
            )

        new_barcode = Barcode(
            product_id=product_id,
            barcode=barcode,
            is_primary=is_primary,
        )
        self.session.add(new_barcode)
        await self.session.flush()
        return new_barcode

    async def delete_barcode(self, barcode_id: UUID) -> None:
        """Видаляє додатковий штрих-код."""
        barcode_obj = await self.session.get(Barcode, barcode_id)
        if not barcode_obj:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Штрих-код не знайдено",
            )
        await self.session.delete(barcode_obj)
        await self.session.flush()
