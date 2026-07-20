"""
API роутер для роботи з товарами (Products).

Ендпоінти:
  - GET    /products          — список товарів з пошуком та фільтрацією
  - GET    /products/{id}     — отримати товар за ID
  - GET    /products/barcode/{barcode} — отримати товар за штрих-кодом
  - POST   /products          — створити новий товар
  - PUT    /products/{id}     — оновити товар
  - DELETE /products/{id}     — видалити товар
"""

from uuid import UUID

from fastapi import APIRouter, Depends, Query
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.schemas.product import (
    ProductCreate,
    ProductUpdate,
    ProductResponse,
    ProductListResponse,
    ProductSearchParams,
)
from app.services.product_service import ProductService
from app.services.auth_service import AuthService

# Створюємо роутер з тегом "Товари"
router = APIRouter(
    prefix="/products",
    tags=["Товари"],
)


@router.get("", response_model=ProductListResponse)
async def list_products(
    query: str = Query(None, description="Пошуковий запит"),
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
    params = ProductSearchParams(
        query=query,
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
    return ProductListResponse(
        items=[ProductResponse.model_validate(p) for p in products],
        total=total,
        page=page,
        size=size,
    )


@router.get("/barcode/{barcode}", response_model=ProductResponse)
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
    return ProductResponse.model_validate(product)


@router.get("/{product_id}", response_model=ProductResponse)
async def get_product(
    product_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує товар за ID."""
    service = ProductService(session)
    product = await service.get_product_by_id(product_id)
    return ProductResponse.model_validate(product)


@router.post("", response_model=ProductResponse, status_code=201)
async def create_product(
    data: ProductCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Створює новий товар."""
    service = ProductService(session)
    product = await service.create_product(data)
    return ProductResponse.model_validate(product)


@router.put("/{product_id}", response_model=ProductResponse)
async def update_product(
    product_id: UUID,
    data: ProductUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Оновлює дані товару."""
    service = ProductService(session)
    product = await service.update_product(product_id, data)
    return ProductResponse.model_validate(product)


@router.delete("/{product_id}", status_code=204)
async def delete_product(
    product_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Видаляє товар."""
    service = ProductService(session)
    await service.delete_product(product_id)
