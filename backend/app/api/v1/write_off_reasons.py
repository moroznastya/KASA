"""
API роутер для довідника причин списання (WriteOffReasons).

Ендпоінти:
  - GET  /write-off-reasons      — список причин
  - POST /write-off-reasons      — створити нову причину (409 якщо дублікат)
"""

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.exc import IntegrityError

from app.database import get_session
from app.infrastructure.persistence.models.reasons import WriteOffReason
from app.schemas.write_off_reasons import WriteOffReasonCreate, WriteOffReasonResponse
from app.domain.services.auth_service import AuthService

router = APIRouter(
    prefix="/write-off-reasons",
    tags=["Списання"],
)


@router.get("", response_model=dict)
async def list_write_off_reasons(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список причин списання (всі, з прапором is_active).
    """
    result = await session.execute(
        select(WriteOffReason)
        .order_by(WriteOffReason.created_at.asc(), WriteOffReason.name.asc())
    )
    reasons = result.scalars().all()
    return {
        "items": [WriteOffReasonResponse.model_validate(r) for r in reasons],
        "total": len(reasons),
    }


@router.post("", response_model=WriteOffReasonResponse, status_code=201)
async def create_write_off_reason(
    data: WriteOffReasonCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """
    Створює нову причину списання.

    - name обрізається від пробілів; мінімум 2 символи (перевіряє Pydantic)
    - Дублікат назви (case-insensitive) → 409 Conflict
    """
    name = data.name.strip()
    if len(name) < 2:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
            detail="Назва причини має містити щонайменше 2 символи",
        )

    # Перевірка на дублікат (case-insensitive)
    result = await session.execute(
        select(func.count(WriteOffReason.id)).where(
            func.lower(WriteOffReason.name) == name.lower()
        )
    )
    if (result.scalar() or 0) > 0:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"Причина «{name}» вже існує",
        )

    reason = WriteOffReason(name=name)
    session.add(reason)
    try:
        await session.flush()
    except IntegrityError:
        # Гонка: хтось створив таку саму причину між перевіркою і вставкою
        await session.rollback()
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"Причина «{name}» вже існує",
        )
    return WriteOffReasonResponse.model_validate(reason)
