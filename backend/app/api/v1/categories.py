"""
API роутер для роботи з категоріями (Categories).

Ендпоінти:
  - GET    /categories          — список категорій (з пагінацією)
  - GET    /categories/tree     — дерево категорій
  - GET    /categories/{id}     — отримати категорію за ID
  - POST   /categories          — створити категорію
  - PUT    /categories/{id}     — оновити категорію
  - DELETE /categories/{id}     — видалити категорію
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, func
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.infrastructure.persistence.models.category import Category
from app.schemas.category import (
    CategoryCreate,
    CategoryUpdate,
    CategoryResponse,
    CategoryTreeResponse,
)
from app.domain.services.auth_service import AuthService

router = APIRouter(
    prefix="/categories",
    tags=["Категорії"],
)


@router.get("", response_model=dict)
async def list_categories(
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список категорій з пагінацією.

    Повертає:
    - items: список категорій
    - total: загальна кількість
    - page: поточна сторінка
    - page_size: розмір сторінки
    - pages: загальна кількість сторінок
    """
    # Загальна кількість
    count_result = await session.execute(
        select(func.count(Category.id))
    )
    total = count_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    result = await session.execute(
        select(Category)
        .order_by(Category.name)
        .offset(offset)
        .limit(size)
    )
    categories = result.scalars().all()

    pages = max(1, (total + size - 1) // size) if total > 0 else 1

    return {
        "items": [CategoryResponse.model_validate(c) for c in categories],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": pages,
    }


@router.get("/tree", response_model=list[CategoryTreeResponse])
async def get_category_tree(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує дерево категорій (ієрархічна структура).

    Повертає тільки кореневі категорії з вкладеними дочірніми.
    """
    # Отримуємо всі категорії
    result = await session.execute(select(Category).order_by(Category.name))
    all_categories = result.scalars().all()

    # Будуємо дерево: спочатку кореневі (parent_id is None)
    def build_tree(parent_id: UUID | None) -> list[CategoryTreeResponse]:
        """Рекурсивно будує дерево категорій."""
        children = []
        for cat in all_categories:
            if cat.parent_id == parent_id:
                child_tree = CategoryTreeResponse(
                    id=cat.id,
                    name=cat.name,
                    description=cat.description,
                    parent_id=cat.parent_id,
                    children=build_tree(cat.id),
                    created_at=cat.created_at,
                    updated_at=cat.updated_at,
                )
                children.append(child_tree)
        return children

    return build_tree(None)


@router.get("/{category_id}", response_model=CategoryResponse)
async def get_category(
    category_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує категорію за ID."""
    result = await session.execute(
        select(Category).where(Category.id == category_id)
    )
    category = result.scalar_one_or_none()
    if not category:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Категорію з ID '{category_id}' не знайдено",
        )
    return CategoryResponse.model_validate(category)


@router.post("", response_model=CategoryResponse, status_code=201)
async def create_category(
    data: CategoryCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Створює нову категорію."""
    # Перевіряємо, чи існує батьківська категорія
    if data.parent_id:
        result = await session.execute(
            select(Category).where(Category.id == data.parent_id)
        )
        if not result.scalar_one_or_none():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Батьківську категорію з ID '{data.parent_id}' не знайдено",
            )

    category = Category(
        name=data.name,
        description=data.description,
        parent_id=data.parent_id,
    )
    session.add(category)
    await session.flush()
    return CategoryResponse.model_validate(category)


@router.put("/{category_id}", response_model=CategoryResponse)
async def update_category(
    category_id: UUID,
    data: CategoryUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Оновлює категорію."""
    result = await session.execute(
        select(Category).where(Category.id == category_id)
    )
    category = result.scalar_one_or_none()
    if not category:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Категорію з ID '{category_id}' не знайдено",
        )

    # Перевіряємо батьківську категорію
    if data.parent_id is not None and data.parent_id != category.parent_id:
        if data.parent_id == category_id:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Категорія не може бути власною батьківською категорією",
            )
        result = await session.execute(
            select(Category).where(Category.id == data.parent_id)
        )
        if not result.scalar_one_or_none():
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Батьківську категорію з ID '{data.parent_id}' не знайдено",
            )

    update_data = data.model_dump(exclude_unset=True)
    for field, value in update_data.items():
        setattr(category, field, value)

    await session.flush()
    return CategoryResponse.model_validate(category)


@router.delete("/{category_id}", status_code=204)
async def delete_category(
    category_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Видаляє категорію."""
    result = await session.execute(
        select(Category).where(Category.id == category_id)
    )
    category = result.scalar_one_or_none()
    if not category:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Категорію з ID '{category_id}' не знайдено",
        )
    await session.delete(category)
    await session.flush()
