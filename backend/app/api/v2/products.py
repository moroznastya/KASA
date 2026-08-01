"""Product API v2 — використовує ProductUseCases з кешуванням.

Кешування:
- GET /products — кеш списку (TTL: 30s), інвалідація при POST
- GET /products/{id} — кеш продукту (TTL: 60s), інвалідація при PUT/DELETE
- GET /products/barcode/{barcode} — кеш пошуку за штрих-кодом (TTL: 30s)
- POST/PUT/DELETE — інвалідація відповідних кешів
"""

from __future__ import annotations

from datetime import datetime
from uuid import UUID

import os
import shutil
import uuid as uuid_module

from fastapi import APIRouter, Depends, File, Form, HTTPException, Query, UploadFile, status
from pydantic import BaseModel, Field

from app.application.use_cases import ProductUseCases
from app.domain.services.cache_service import ICacheService
from app.config import settings
from .deps import get_product_use_cases, get_cache_service
from .cache_utils import cached, invalidate_product_cache

router = APIRouter(prefix="/products", tags=["products_v2"])


# ─── Pydantic схеми ──────────────────────────────────────────────────────────

class ProductResponse(BaseModel):
    id: UUID
    name: str
    barcode: str | None = None
    price: float | None = None
    cost_price: float | None = None
    quantity: float = 0
    unit: str = "шт"
    category_id: UUID | None = None
    supplier_id: UUID | None = None
    is_active: bool = True
    sku: str = ""
    description: str = ""

    model_config = {"from_attributes": True}


class CreateProductRequest(BaseModel):
    name: str = Field(..., min_length=1, max_length=255)
    barcode: str | None = Field(None, min_length=1, max_length=50)
    price: float | None = Field(None, gt=0)
    cost_price: float | None = None
    quantity: float = 0
    unit: str = "шт"
    category_id: UUID | None = None
    supplier_id: UUID | None = None
    sku: str = ""
    description: str = ""


class UpdateProductRequest(BaseModel):
    name: str | None = Field(None, min_length=1, max_length=255)
    barcode: str | None = Field(None, min_length=1, max_length=50)
    price: float | None = Field(None, gt=0)
    cost_price: float | None = None
    unit: str | None = None
    is_active: bool | None = None
    sku: str | None = None
    description: str | None = None


class ProductListResponse(BaseModel):
    items: list[ProductResponse]
    total: int
    page: int
    size: int


class ProductImageResponse(BaseModel):
    id: UUID
    product_id: UUID | None = None
    url: str
    is_main: bool = False
    sort_order: int = 0
    created_at: datetime | None = None

    model_config = {"from_attributes": True}


class ProductBarcodeResponse(BaseModel):
    id: UUID
    product_id: UUID | None = None
    barcode: str
    is_primary: bool = False

    model_config = {"from_attributes": True}


class BarcodeCreateRequest(BaseModel):
    barcode: str = Field(..., min_length=1, max_length=50)
    is_primary: bool = False


# ─── Ендпоінти ───────────────────────────────────────────────────────────────

@router.get("", response_model=ProductListResponse)
async def list_products(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    search: str | None = None,
    category_id: UUID | None = None,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Отримати список товарів з пагінацією, фільтрацією та кешуванням (TTL: 60s)."""

    @cached(
        cache,
        prefix="products:list",
        ttl=settings.CACHE_TTL_PRODUCTS,
    )
    async def _fetch(page: int, size: int, search: str | None, category_id: UUID | None):
        products, total = await use_cases.search_products(
            query=search,
            category_id=category_id,
            page=page,
            size=size,
        )
        return {
            "items": products,
            "total": total,
            "page": page,
            "size": size,
        }

    return await _fetch(page, size, search, category_id)


@router.get("/barcode/{barcode}", response_model=ProductResponse)
async def get_product_by_barcode(
    barcode: str,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Отримати товар за штрих-кодом (основне поле або додатковий штрих-код) з кешуванням (TTL: 30s)."""

    @cached(
        cache,
        prefix="products:barcode",
        ttl=settings.CACHE_TTL_BARCODE,
    )
    async def _fetch(barcode: str):
        try:
            product = await use_cases.get_product_by_barcode(barcode)
            if product is None:
                raise HTTPException(
                    status_code=404,
                    detail=f"Товар зі штрих-кодом '{barcode}' не знайдено",
                )
            return product
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(barcode)


@router.post("/{product_id}/images", response_model=ProductImageResponse)
async def upload_product_image(
    product_id: UUID,
    file: UploadFile = File(...),
    is_main: bool = Form(False),
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Завантажити зображення товару.

    Файл зберігається у uploads/products/{product_id}/, у БД — URL.
    """
    # Зберігаємо файл на диск (інфраструктурна операція)
    upload_dir = os.path.join("uploads", "products", str(product_id))
    os.makedirs(upload_dir, exist_ok=True)

    ext = os.path.splitext(file.filename or "image.jpg")[1]
    filename = f"{uuid_module.uuid4()}{ext}"
    filepath = os.path.join(upload_dir, filename)

    with open(filepath, "wb") as f:
        shutil.copyfileobj(file.file, f)

    url = f"/uploads/products/{product_id}/{filename}"
    try:
        return await use_cases.add_product_image(product_id, url, is_main)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.delete("/{product_id}/images/{image_id}", status_code=204)
async def delete_product_image(
    product_id: UUID,
    image_id: UUID,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Видалити зображення товару."""
    try:
        await use_cases.delete_product_image(image_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.post("/{product_id}/barcodes", response_model=ProductBarcodeResponse)
async def add_product_barcode(
    product_id: UUID,
    data: BarcodeCreateRequest,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Додати додатковий штрих-код до товару."""
    try:
        return await use_cases.add_product_barcode(
            product_id,
            data.barcode,
            data.is_primary,
        )
    except ValueError as e:
        detail = str(e)
        status_code = 409 if "вже існує" in detail else 404
        raise HTTPException(status_code=status_code, detail=detail)


@router.delete("/{product_id}/barcodes/{barcode_id}", status_code=204)
async def delete_product_barcode(
    product_id: UUID,
    barcode_id: UUID,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Видалити додатковий штрих-код товару."""
    try:
        await use_cases.delete_product_barcode(barcode_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.get("/{product_id}", response_model=ProductResponse)
async def get_product(
    product_id: UUID,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Отримати товар за ID з кешуванням (TTL: 300s)."""

    @cached(
        cache,
        prefix=f"product:{product_id}",
        ttl=settings.CACHE_TTL_PRODUCT_DETAIL,
    )
    async def _fetch(product_id: UUID):
        try:
            return await use_cases.get_product(product_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(product_id)


@router.post("", response_model=ProductResponse, status_code=201)
async def create_product(
    data: CreateProductRequest,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Створити новий товар та інвалідувати кеш списку."""
    try:
        from app.application.dto.product_dto import ProductCreateDTO
        dto = ProductCreateDTO(
            name=data.name,
            barcode=data.barcode,
            price=__import__('decimal').Decimal(str(data.price)) if data.price else None,
            cost_price=__import__('decimal').Decimal(str(data.cost_price)) if data.cost_price else None,
            stock=__import__('decimal').Decimal(str(data.quantity)) if data.quantity else None,
            category_id=data.category_id,
            supplier_id=data.supplier_id,
            unit=data.unit,
            sku=data.sku,
            description=data.description,
        )
        result = await use_cases.create_product(dto)

        # Інвалідація кешу списку продуктів
        await invalidate_product_cache(cache)

        return result
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.put("/{product_id}", response_model=ProductResponse)
async def update_product(
    product_id: UUID,
    data: UpdateProductRequest,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Оновити товар та інвалідувати кеш."""
    try:
        from app.application.dto.product_dto import ProductUpdateDTO
        dto = ProductUpdateDTO(
            name=data.name,
            barcode=data.barcode,
            price=__import__('decimal').Decimal(str(data.price)) if data.price is not None else None,
            cost_price=__import__('decimal').Decimal(str(data.cost_price)) if data.cost_price is not None else None,
            unit=data.unit,
            is_active=data.is_active,
            sku=data.sku,
            description=data.description,
        )
        result = await use_cases.update_product(product_id, dto)

        # Інвалідація кешу (список + конкретний продукт)
        await invalidate_product_cache(cache)

        return result
    except ValueError as e:
        raise HTTPException(status_code=400 if "не знайдено" not in str(e) else 404, detail=str(e))


@router.delete("/{product_id}", status_code=204)
async def delete_product(
    product_id: UUID,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Видалити товар та інвалідувати кеш."""
    try:
        await use_cases.delete_product(product_id)

        # Інвалідація кешу
        await invalidate_product_cache(cache)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))
