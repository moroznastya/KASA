"""
API роутер для роботи з боржниками (Debtors).

Ендпоінти:
  - GET    /debtors/search?query=...  — пошук боржників за ім'ям
  - POST   /debtors                   — створити нового боржника
  - GET    /debtors                   — список всіх боржників
  - GET    /debtors/{id}              — отримати боржника за ID
  - PUT    /debtors/{id}              — оновити боржника
  - POST   /debtors/{id}/pay          — погашення боргу (внесення оплати)
"""

from decimal import Decimal
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, or_, desc
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.models.debtor import Debtor
from app.schemas.debtor import (
    DebtorCreate,
    DebtorUpdate,
    DebtorResponse,
    DebtorPayRequest,
)
from app.services.auth_service import AuthService

router = APIRouter(
    prefix="/debtors",
    tags=["Боржники"],
)


@router.get("/search", response_model=list[DebtorResponse])
async def search_debtors(
    query: str = Query(..., min_length=1, description="Пошуковий запит"),
    limit: int = Query(10, ge=1, le=50, description="Максимальна кількість результатів"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Пошук боржників за ім'ям (без урахування регістру).
    Повертає до `limit` результатів.
    """
    search_pattern = f"%{query}%"
    result = await session.execute(
        select(Debtor)
        .where(Debtor.name.ilike(search_pattern))
        .order_by(Debtor.name)
        .limit(limit)
    )
    debtors = list(result.scalars().all())
    return [DebtorResponse.model_validate(d) for d in debtors]


@router.get("", response_model=list[DebtorResponse])
async def list_debtors(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Повертає список всіх боржників, відсортованих за сумою боргу (спочатку найбільші).
    """
    result = await session.execute(
        select(Debtor)
        .order_by(desc(Debtor.total_debt))
    )
    debtors = list(result.scalars().all())
    return [DebtorResponse.model_validate(d) for d in debtors]


@router.post("", response_model=DebtorResponse, status_code=201)
async def create_debtor(
    data: DebtorCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Створює нового боржника."""
    debtor = Debtor(
        name=data.name,
        phone=data.phone,
        notes=data.notes,
    )
    session.add(debtor)
    await session.flush()
    await session.refresh(debtor)
    return DebtorResponse.model_validate(debtor)


@router.get("/{debtor_id}", response_model=DebtorResponse)
async def get_debtor(
    debtor_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує боржника за ID."""
    result = await session.execute(
        select(Debtor).where(Debtor.id == debtor_id)
    )
    debtor = result.scalar_one_or_none()
    if not debtor:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Боржника з ID '{debtor_id}' не знайдено",
        )
    return DebtorResponse.model_validate(debtor)


@router.put("/{debtor_id}", response_model=DebtorResponse)
async def update_debtor(
    debtor_id: UUID,
    data: DebtorUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Оновлює дані боржника."""
    result = await session.execute(
        select(Debtor).where(Debtor.id == debtor_id)
    )
    debtor = result.scalar_one_or_none()
    if not debtor:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Боржника з ID '{debtor_id}' не знайдено",
        )

    if data.name is not None:
        debtor.name = data.name
    if data.phone is not None:
        debtor.phone = data.phone
    if data.notes is not None:
        debtor.notes = data.notes

    await session.flush()
    await session.refresh(debtor)
    return DebtorResponse.model_validate(debtor)


@router.post("/{debtor_id}/pay", response_model=DebtorResponse)
async def pay_debt(
    debtor_id: UUID,
    data: DebtorPayRequest,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Погашення боргу. Зменшує загальну суму боргу на вказану суму.
    Сума має бути додатною і не більшою за поточний борг.
    """
    if data.amount <= 0:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Сума оплати має бути більше 0",
        )

    result = await session.execute(
        select(Debtor).where(Debtor.id == debtor_id)
    )
    debtor = result.scalar_one_or_none()
    if not debtor:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Боржника з ID '{debtor_id}' не знайдено",
        )

    if debtor.total_debt <= 0:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="У боржника немає боргу",
        )

    if data.amount > debtor.total_debt:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Сума оплати ({data.amount}) перевищує поточний борг ({debtor.total_debt})",
        )

    debtor.total_debt -= data.amount
    
    # Якщо борг став 0 — автоматично видаляємо боржника
    if float(debtor.total_debt) <= 0:
        # Зберігаємо дані для відповіді перед видаленням
        response_data = DebtorResponse.model_validate(debtor)
        await session.delete(debtor)
        await session.commit()
        return response_data
    
    await session.flush()
    await session.refresh(debtor)
    return DebtorResponse.model_validate(debtor)
