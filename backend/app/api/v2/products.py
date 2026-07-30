"""Product API v2 — використовує ProductUseCases."""

from __future__ import annotations

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field

from app.application.use_cases import ProductUseCases
from .deps import get_product_use_cases

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


# ─── Ендпоінти ───────────────────────────────────────────────────────────────

@router.get("", response_model=ProductListResponse)
async def list_products(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    search: str | None = None,
    category_id: UUID | None = None,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Отримати список товарів з пагінацією та фільтрацією."""
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


@router.get("/{product_id}", response_model=ProductResponse)
async def get_product(
    product_id: UUID,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Отримати товар за ID."""
    try:
        product = await use_cases.get_product(product_id)
        return product
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.post("", response_model=ProductResponse, status_code=201)
async def create_product(
    data: CreateProductRequest,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Створити новий товар."""
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
        return await use_cases.create_product(dto)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.put("/{product_id}", response_model=ProductResponse)
async def update_product(
    product_id: UUID,
    data: UpdateProductRequest,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Оновити товар."""
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
        return await use_cases.update_product(product_id, dto)
    except ValueError as e:
        raise HTTPException(status_code=400 if "не знайдено" not in str(e) else 404, detail=str(e))


@router.delete("/{product_id}", status_code=204)
async def delete_product(
    product_id: UUID,
    use_cases: ProductUseCases = Depends(get_product_use_cases),
):
    """Видалити товар."""
    try:
        await use_cases.delete_product(product_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))
