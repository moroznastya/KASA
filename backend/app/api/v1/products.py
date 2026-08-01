"""
API роутер для роботи з товарами (Products).

Ендпоінти:
  - GET    /products                — список товарів з пошуком та фільтрацією
  - GET    /products/{id}           — отримати товар за ID (з зображеннями та штрих-кодами)
  - GET    /products/barcode/{barcode} — отримати товар за штрих-кодом
  - POST   /products                — створити новий товар
  - PUT    /products/{id}           — оновити товар
  - DELETE /products/{id}           — видалити товар
  - POST   /products/{id}/images    — завантажити зображення товару
  - DELETE /products/{id}/images/{image_id} — видалити зображення товару
  - POST   /products/{id}/barcodes  — додати додатковий штрих-код
  - DELETE /products/{id}/barcodes/{barcode_id} — видалити додатковий штрих-код

⚠️ DEPRECATED: цей v1-роутер залишено для зворотної сумісності — використовуйте /api/v2/products/*.
"""

import os
import shutil
import uuid
from uuid import UUID

from fastapi import APIRouter, Depends, Query, UploadFile, File, Form, HTTPException, status
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.product_image import ProductImage
from app.infrastructure.persistence.models.barcode import Barcode
from app.schemas.product import (
    ProductCreate,
    ProductUpdate,
    ProductResponse,
    ProductListResponse,
    ProductSearchParams,
    ProductImageResponse,
    BarcodeResponse,
)
from app.domain.services.product_service import ProductService
from app.domain.services.auth_service import AuthService


# ─── Допоміжна функція: завантажити товар зі зв'язками ─────────────────────

async def _get_product_with_relations(
    session: AsyncSession,
    product_id: UUID,
) -> Product | None:
    """
    Отримує товар за ID з попередньо завантаженими images та barcodes.
    Використовується для уникнення MissingGreenlet при lazy-loading.
    """
    result = await session.execute(
        select(Product)
        .options(
            selectinload(Product.images).load_only(
                ProductImage.id, ProductImage.url, ProductImage.is_main,
                ProductImage.sort_order, ProductImage.created_at,
            ),
            selectinload(Product.barcodes).load_only(
                Barcode.id, Barcode.barcode, Barcode.is_primary, Barcode.created_at,
            ),
        )
        .where(Product.id == product_id)
    )
    return result.scalar_one_or_none()


# Створюємо роутер з тегом "Товари"
router = APIRouter(
    prefix="/products",
    tags=["Товари"],
)


@router.get("", response_model=ProductListResponse, deprecated=True)
async def list_products(
    query: str = Query(None, description="Пошуковий запит"),
    search: str = Query(None, description="Аліас для query (сумісність)"),
    barcode: str = Query(None, description="Штрих-код"),
    category_id: UUID = Query(None, description="ID категорії"),
    supplier_id: UUID = Query(None, description="ID постачальника"),
    min_price: float = Query(None, description="Мінімальна ціна"),
    max_price: float = Query(None, description="Максимальна ціна"),
    is_weight: bool = Query(None, description="Ваговий товар"),
    page: int = Query(1, ge=1, description="Сторінка"),
    size: int = Query(20, ge=1, le=100, description="Елементів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user: AuthService = Depends(AuthService.get_current_user),
):
    """
    Отримує список товарів з можливістю пошуку та фільтрації.

    Підтримує:
    - Пошук за назвою, штрих-кодом, артикулом
    - Фільтрацію за категорією, постачальником, ціною
    - Пагінацію
    """
    service = ProductService(session)
    effective_query = query or search
    params = ProductSearchParams(
        query=effective_query,
        barcode=barcode,
        category_id=category_id,
        supplier_id=supplier_id,
        min_price=min_price,
        max_price=max_price,
        is_weight=is_weight,
        page=page,
        size=size,
    )
    products, total = await service.search_products(params)

    # Завантажуємо зв'язки для кожного товару в списку
    items = []
    for p in products:
        full = await _get_product_with_relations(session, p.id)
        items.append(ProductResponse.model_validate(full or p))

    pages = max(1, (total + size - 1) // size) if total > 0 else 1
    return ProductListResponse(
        items=items,
        total=total,
        page=page,
        page_size=size,
        pages=pages,
    )


@router.get("/barcode/{barcode}", response_model=ProductResponse, deprecated=True)
async def get_product_by_barcode(
    barcode: str,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує товар за штрих-кодом.

    Шукає спочатку в основному полі barcode товару,
    потім у таблиці додаткових штрих-кодів.
    """
    service = ProductService(session)
    product = await service.get_product_by_barcode(barcode)
    # Перезавантажуємо зі зв'язками
    full = await _get_product_with_relations(session, product.id)
    return ProductResponse.model_validate(full or product)


@router.get("/{product_id}", response_model=ProductResponse, deprecated=True)
async def get_product(
    product_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує товар за ID з усіма зв'язаними даними.

    Повертає:
    - Основні поля товару
    - images: список зображень
    - barcodes: список додаткових штрих-кодів
    """
    product = await _get_product_with_relations(session, product_id)
    if not product:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Товар з ID '{product_id}' не знайдено",
        )
    return ProductResponse.model_validate(product)


@router.post("", response_model=ProductResponse, status_code=201, deprecated=True)
async def create_product(
    data: ProductCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Створює новий товар."""
    service = ProductService(session)
    product = await service.create_product(data)
    # Перезавантажуємо зі зв'язками для відповіді
    full = await _get_product_with_relations(session, product.id)
    return ProductResponse.model_validate(full or product)


@router.put("/{product_id}", response_model=ProductResponse, deprecated=True)
async def update_product(
    product_id: UUID,
    data: ProductUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Оновлює дані товару."""
    service = ProductService(session)
    product = await service.update_product(product_id, data)
    # Перезавантажуємо зі зв'язками для відповіді
    full = await _get_product_with_relations(session, product.id)
    return ProductResponse.model_validate(full or product)


@router.delete("/{product_id}", status_code=204, deprecated=True)
async def delete_product(
    product_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Видаляє товар."""
    service = ProductService(session)
    await service.delete_product(product_id)


# ─── Зображення товару ───────────────────────────────────────────────────────


@router.post("/{product_id}/images", response_model=ProductImageResponse, deprecated=True)
async def upload_product_image(
    product_id: UUID,
    file: UploadFile = File(...),
    is_main: bool = Form(False),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Завантажити зображення товару."""
    service = ProductService(session)

    # Перевіряємо, чи існує товар
    await service.get_product_by_id(product_id)

    # Зберігаємо файл
    upload_dir = os.path.join("uploads", "products", str(product_id))
    os.makedirs(upload_dir, exist_ok=True)

    # Генеруємо унікальне ім'я файлу
    ext = os.path.splitext(file.filename or "image.jpg")[1]
    filename = f"{uuid.uuid4()}{ext}"
    filepath = os.path.join(upload_dir, filename)

    with open(filepath, "wb") as f:
        shutil.copyfileobj(file.file, f)

    url = f"/uploads/products/{product_id}/{filename}"
    image = await service.add_image(product_id, url, is_main)
    return image


@router.delete("/{product_id}/images/{image_id}", status_code=status.HTTP_204_NO_CONTENT, deprecated=True)
async def delete_product_image(
    product_id: UUID,
    image_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Видалити зображення товару."""
    service = ProductService(session)
    image = await session.get(ProductImage, image_id)
    if not image:
        raise HTTPException(status_code=404, detail="Зображення не знайдено")

    # Видаляємо файл
    if os.path.exists(image.url.lstrip("/")):
        os.remove(image.url.lstrip("/"))

    await service.delete_image(image_id)


# ─── Додаткові штрих-коди ────────────────────────────────────────────────────


class BarcodeCreate(BaseModel):
    """Схема створення додаткового штрих-коду."""
    barcode: str = Field(..., max_length=50, description="Штрих-код")
    is_primary: bool = Field(False, description="Чи є основним")


@router.post("/{product_id}/barcodes", response_model=BarcodeResponse, deprecated=True)
async def add_product_barcode(
    product_id: UUID,
    data: BarcodeCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Додати додатковий штрих-код до товару."""
    service = ProductService(session)
    barcode = await service.add_barcode(product_id, data.barcode, data.is_primary)
    return barcode


@router.delete("/{product_id}/barcodes/{barcode_id}", status_code=status.HTTP_204_NO_CONTENT, deprecated=True)
async def delete_product_barcode(
    product_id: UUID,
    barcode_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Видалити додатковий штрих-код товару."""
    service = ProductService(session)
    await service.delete_barcode(barcode_id)
