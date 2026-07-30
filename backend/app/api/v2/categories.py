"""Category API v2 — CRUD для категорій через репозиторій."""

from __future__ import annotations

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field

from app.domain.repositories import ICategoryRepository
from .deps import get_category_repository

router = APIRouter(prefix="/categories", tags=["categories_v2"])


# ─── Pydantic схеми ──────────────────────────────────────────────────────────

class CategoryResponse(BaseModel):
    id: UUID
    name: str
    parent_id: UUID | None = None
    description: str = ""
    sort_order: int = 0
    is_active: bool = True

    model_config = {"from_attributes": True}


class CategoryTreeResponse(BaseModel):
    id: UUID
    name: str
    parent_id: UUID | None = None
    description: str = ""
    children: list[CategoryTreeResponse] = []

    model_config = {"from_attributes": True}


class CreateCategoryRequest(BaseModel):
    name: str = Field(..., min_length=1, max_length=255)
    parent_id: UUID | None = None
    description: str = ""
    sort_order: int = 0


class UpdateCategoryRequest(BaseModel):
    name: str | None = Field(None, min_length=1, max_length=255)
    parent_id: UUID | None = None
    description: str | None = None
    sort_order: int | None = None
    is_active: bool | None = None


class CategoryListResponse(BaseModel):
    items: list[CategoryResponse]
    total: int
    page: int
    size: int


# ─── Ендпоінти ───────────────────────────────────────────────────────────────

@router.get("", response_model=CategoryListResponse)
async def list_categories(
    page: int = Query(1, ge=1),
    size: int = Query(50, ge=1, le=1000),
    search: str | None = None,
    repo: ICategoryRepository = Depends(get_category_repository),
):
    """Отримати список категорій з пагінацією."""
    categories, total = await repo.search(
        query=search,
        page=page,
        size=size,
    )
    return {
        "items": [CategoryResponse.model_validate(c) for c in categories],
        "total": total,
        "page": page,
        "size": size,
    }


@router.get("/tree", response_model=list[CategoryTreeResponse])
async def get_category_tree(
    repo: ICategoryRepository = Depends(get_category_repository),
):
    """Отримати дерево категорій (ієрархічна структура)."""
    all_categories = await repo.find_all()

    def build_tree(parent_id: UUID | None) -> list[CategoryTreeResponse]:
        """Рекурсивно будує дерево категорій."""
        children = []
        for cat in all_categories:
            if cat.parent_id == parent_id:
                child_tree = CategoryTreeResponse(
                    id=cat.id,
                    name=cat.name,
                    parent_id=cat.parent_id,
                    description=cat.description,
                    children=build_tree(cat.id),
                )
                children.append(child_tree)
        return children

    return build_tree(None)


@router.get("/{category_id}", response_model=CategoryResponse)
async def get_category(
    category_id: UUID,
    repo: ICategoryRepository = Depends(get_category_repository),
):
    """Отримати категорію за ID."""
    category = await repo.find_by_id(category_id)
    if not category:
        raise HTTPException(status_code=404, detail=f"Категорію з ID '{category_id}' не знайдено")
    return CategoryResponse.model_validate(category)


@router.post("", response_model=CategoryResponse, status_code=201)
async def create_category(
    data: CreateCategoryRequest,
    repo: ICategoryRepository = Depends(get_category_repository),
):
    """Створити нову категорію."""
    # Перевіряємо унікальність назви
    exists = await repo.exists_by_name(data.name)
    if exists:
        raise HTTPException(status_code=400, detail=f"Категорія з назвою '{data.name}' вже існує")

    # Перевіряємо батьківську категорію
    if data.parent_id:
        parent = await repo.find_by_id(data.parent_id)
        if not parent:
            raise HTTPException(status_code=404, detail=f"Батьківську категорію з ID '{data.parent_id}' не знайдено")

    from app.domain.entities.category import Category
    category = Category(
        name=data.name,
        parent_id=data.parent_id,
        description=data.description,
        sort_order=data.sort_order,
    )
    saved = await repo.save(category)
    return CategoryResponse.model_validate(saved)


@router.put("/{category_id}", response_model=CategoryResponse)
async def update_category(
    category_id: UUID,
    data: UpdateCategoryRequest,
    repo: ICategoryRepository = Depends(get_category_repository),
):
    """Оновити категорію."""
    category = await repo.find_by_id(category_id)
    if not category:
        raise HTTPException(status_code=404, detail=f"Категорію з ID '{category_id}' не знайдено")

    # Перевіряємо унікальність назви
    if data.name is not None and data.name != category.name:
        exists = await repo.exists_by_name(data.name, exclude_id=category_id)
        if exists:
            raise HTTPException(status_code=400, detail=f"Категорія з назвою '{data.name}' вже існує")

    # Перевіряємо батьківську категорію
    if data.parent_id is not None and data.parent_id != category.parent_id:
        if data.parent_id == category_id:
            raise HTTPException(status_code=400, detail="Категорія не може бути власною батьківською категорією")
        parent = await repo.find_by_id(data.parent_id)
        if not parent:
            raise HTTPException(status_code=404, detail=f"Батьківську категорію з ID '{data.parent_id}' не знайдено")

    # Оновлюємо поля
    if data.name is not None:
        category.name = data.name
    if data.parent_id is not None:
        category.parent_id = data.parent_id
    if data.description is not None:
        category.description = data.description
    if data.sort_order is not None:
        category.sort_order = data.sort_order
    if data.is_active is not None:
        category.is_active = data.is_active

    saved = await repo.update(category)
    return CategoryResponse.model_validate(saved)


@router.delete("/{category_id}", status_code=204)
async def delete_category(
    category_id: UUID,
    repo: ICategoryRepository = Depends(get_category_repository),
):
    """Видалити категорію."""
    category = await repo.find_by_id(category_id)
    if not category:
        raise HTTPException(status_code=404, detail=f"Категорію з ID '{category_id}' не знайдено")
    await repo.delete(category_id)
