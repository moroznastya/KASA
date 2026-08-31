"""
API роутер для роботи з постачальниками (Suppliers).

Ендпоінти:
  - GET    /suppliers                  — список постачальників з пагінацією
  - GET    /suppliers/all              — список всіх постачальників (без пагінації)
  - GET    /suppliers/{id}             — отримати постачальника за ID
  - POST   /suppliers                  — створити постачальника
  - PUT    /suppliers/{id}             — оновити постачальника
  - DELETE /suppliers/{id}             — видалити постачальника
  - GET    /suppliers/{id}/products    — товари постачальника з залишками
  - GET    /suppliers/{id}/products/{product_id}/movements — рух товару
"""

from decimal import Decimal
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.domain.services.auth_service import AuthService
from app.domain.services.supplier_product_service import SupplierProductService
from app.infrastructure.persistence.models.supplier import Supplier
from app.infrastructure.persistence.models.supplier_ledger import SupplierLedger
from app.schemas.supplier import (
    SupplierCreate,
    SupplierResponse,
    SupplierUpdate,
)
from app.schemas.supplier_products import (
    SupplierProductMovementsResponse,
    SupplierProductsResponse,
)

router = APIRouter(
    prefix="/suppliers",
    tags=["Постачальники"],
)


async def _get_supplier_balance(session: AsyncSession, supplier_id: UUID) -> Decimal:
    """Отримує поточний баланс постачальника."""
    result = await session.execute(
        select(func.coalesce(func.sum(SupplierLedger.amount), 0)).where(
            SupplierLedger.supplier_id == supplier_id
        )
    )
    return Decimal(str(result.scalar() or "0.00"))


async def _supplier_to_response(session: AsyncSession, supplier: Supplier) -> SupplierResponse:
    """Конвертує Supplier у SupplierResponse з балансом."""
    balance = await _get_supplier_balance(session, supplier.id)
    response = SupplierResponse.model_validate(supplier)
    response.current_balance = balance
    return response


@router.get("", response_model=dict)
async def list_suppliers(
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список постачальників з поточним балансом та пагінацією.

    Повертає:
    - items: список постачальників
    - total: загальна кількість
    - page: поточна сторінка
    - page_size: розмір сторінки
    - pages: загальна кількість сторінок
    """
    # Загальна кількість
    count_result = await session.execute(
        select(func.count(Supplier.id))
    )
    total = count_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    result = await session.execute(
        select(Supplier)
        .order_by(Supplier.name)
        .offset(offset)
        .limit(size)
    )
    suppliers = result.scalars().all()

    pages = max(1, (total + size - 1) // size) if total > 0 else 1

    return {
        "items": [await _supplier_to_response(session, s) for s in suppliers],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": pages,
    }


# ⚠️ /all МАЄ БУТИ ПЕРЕД /{supplier_id}, інакше FastAPI сприймає "all" як UUID
@router.get("/all", response_model=list[SupplierResponse])
async def list_all_suppliers(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує список всіх постачальників (без пагінації, для випадаючих списків)."""
    result = await session.execute(select(Supplier).order_by(Supplier.name))
    suppliers = result.scalars().all()
    return [await _supplier_to_response(session, s) for s in suppliers]


@router.get("/{supplier_id}", response_model=SupplierResponse)
async def get_supplier(
    supplier_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує постачальника за ID з поточним балансом."""
    result = await session.execute(
        select(Supplier).where(Supplier.id == supplier_id)
    )
    supplier = result.scalar_one_or_none()
    if not supplier:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Постачальника з ID '{supplier_id}' не знайдено",
        )
    return await _supplier_to_response(session, supplier)


@router.post("", response_model=SupplierResponse, status_code=201)
async def create_supplier(
    data: SupplierCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
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
    await session.refresh(supplier)
    return await _supplier_to_response(session, supplier)


@router.put("/{supplier_id}", response_model=SupplierResponse)
async def update_supplier(
    supplier_id: UUID,
    data: SupplierUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
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
    await session.refresh(supplier)
    return await _supplier_to_response(session, supplier)


@router.delete("/{supplier_id}", status_code=204)
async def delete_supplier(
    supplier_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
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


# ═══════════════════════════════════════════════════════════════════════════════
# Товари постачальника
# ═══════════════════════════════════════════════════════════════════════════════

@router.get("/{supplier_id}/products", response_model=SupplierProductsResponse)
async def get_supplier_products(
    supplier_id: UUID,
    search: str = Query(None, description="Пошук товару за назвою, штрих-кодом або артикулом"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список товарів постачальника з поточними залишками.

    Показує:
    - Всі товари, які прив'язані до цього постачальника
    - Поточний залишок кожного товару
    - Загальну вартість залишків (за собівартістю)
    """
    service = SupplierProductService(session)
    try:
        return await service.get_supplier_products(supplier_id, search=search)
    except ValueError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        )


@router.get(
    "/{supplier_id}/products/{product_id}/movements",
    response_model=SupplierProductMovementsResponse,
)
async def get_supplier_product_movements(
    supplier_id: UUID,
    product_id: UUID,
    limit: int = Query(100, ge=1, le=500, description="Максимальна кількість записів руху"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує історію руху конкретного товару від постачальника.

    Включає:
    - Прибуткові накладні (прихід)
    - Повернення постачальнику (витрата)
    - Продажі (чеки) (витрата)
    - Списання (витрата)
    - Переміщення (витрата)

    Дані сортуються від найновіших до найстаріших.
    """
    service = SupplierProductService(session)
    try:
        return await service.get_product_movements(supplier_id, product_id, limit=limit)
    except ValueError as e:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=str(e),
        )
