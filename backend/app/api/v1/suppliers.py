"""
API роутер для роботи з постачальниками (Suppliers).

Ендпоінти:
  - GET    /suppliers          — список постачальників
  - GET    /suppliers/{id}     — отримати постачальника за ID
  - POST   /suppliers          — створити постачальника
  - PUT    /suppliers/{id}     — оновити постачальника
  - DELETE /suppliers/{id}     — видалити постачальника
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.models.supplier import Supplier
from app.schemas.supplier import (
    SupplierCreate,
    SupplierUpdate,
    SupplierResponse,
)
from app.services.auth_service import AuthService

router = APIRouter(
    prefix="/suppliers",
    tags=["Постачальники"],
)


@router.get("", response_model=list[SupplierResponse])
async def list_suppliers(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує список всіх постачальників."""
    result = await session.execute(select(Supplier).order_by(Supplier.name))
    suppliers = result.scalars().all()
    return [SupplierResponse.model_validate(s) for s in suppliers]


@router.get("/{supplier_id}", response_model=SupplierResponse)
async def get_supplier(
    supplier_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує постачальника за ID."""
    result = await session.execute(
        select(Supplier).where(Supplier.id == supplier_id)
    )
    supplier = result.scalar_one_or_none()
    if not supplier:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Постачальника з ID '{supplier_id}' не знайдено",
        )
    return SupplierResponse.model_validate(supplier)


@router.post("", response_model=SupplierResponse, status_code=201)
async def create_supplier(
    data: SupplierCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Створює нового постачальника."""
    supplier = Supplier(
        name=data.name,
        edrpou=data.edrpou,
        phone=data.phone,
        email=data.email,
        address=data.address,
        notes=data.notes,
    )
    session.add(supplier)
    await session.flush()
    return SupplierResponse.model_validate(supplier)


@router.put("/{supplier_id}", response_model=SupplierResponse)
async def update_supplier(
    supplier_id: UUID,
    data: SupplierUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Оновлює дані постачальника."""
    result = await session.execute(
        select(Supplier).where(Supplier.id == supplier_id)
    )
    supplier = result.scalar_one_or_none()
    if not supplier:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Постачальника з ID '{supplier_id}' не знайдено",
        )

    update_data = data.model_dump(exclude_unset=True)
    for field, value in update_data.items():
        setattr(supplier, field, value)

    await session.flush()
    return SupplierResponse.model_validate(supplier)


@router.delete("/{supplier_id}", status_code=204)
async def delete_supplier(
    supplier_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Видаляє постачальника."""
    result = await session.execute(
        select(Supplier).where(Supplier.id == supplier_id)
    )
    supplier = result.scalar_one_or_none()
    if not supplier:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Постачальника з ID '{supplier_id}' не знайдено",
        )
    await session.delete(supplier)
    await session.flush()
