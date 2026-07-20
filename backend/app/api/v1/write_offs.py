"""
API роутер для роботи зі списаннями товару (WriteOffs).

Ендпоінти:
  - GET    /write-offs            — список списань
  - GET    /write-offs/{id}       — отримати списання за ID
  - POST   /write-offs            — створити списання
  - PUT    /write-offs/{id}       — оновити списання
  - DELETE /write-offs/{id}       — видалити списання
  - POST   /write-offs/{id}/confirm — підтвердити списання
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select, desc
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.models.write_off import WriteOff, WriteOffItem
from app.schemas.write_off import (
    WriteOffCreate,
    WriteOffUpdate,
    WriteOffResponse,
)
from app.services.auth_service import AuthService
from app.services.document_service import DocumentService

router = APIRouter(
    prefix="/write-offs",
    tags=["Списання"],
)


@router.get("", response_model=list[WriteOffResponse])
async def list_write_offs(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує список всіх списань."""
    result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .order_by(desc(WriteOff.created_at))
    )
    write_offs = result.scalars().all()
    return [WriteOffResponse.model_validate(w) for w in write_offs]


@router.get("/{write_off_id}", response_model=WriteOffResponse)
async def get_write_off(
    write_off_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує списання за ID."""
    result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .where(WriteOff.id == write_off_id)
    )
    write_off = result.scalar_one_or_none()
    if not write_off:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Списання з ID '{write_off_id}' не знайдено",
        )
    return WriteOffResponse.model_validate(write_off)


@router.post("", response_model=WriteOffResponse, status_code=201)
async def create_write_off(
    data: WriteOffCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Створює нове списання."""
    write_off = WriteOff(
        number=data.number,
        reason=data.reason,
        write_off_date=data.write_off_date,
        notes=data.notes,
    )
    session.add(write_off)
    await session.flush()

    for item_data in data.items:
        item = WriteOffItem(
            write_off_id=write_off.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
        )
        session.add(item)

    await session.flush()

    # Автоматично підтверджуємо списання (оновлюємо залишки)
    doc_service = DocumentService(session)
    await doc_service.confirm_write_off(write_off.id)

    result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .where(WriteOff.id == write_off.id)
    )
    write_off = result.scalar_one()
    return WriteOffResponse.model_validate(write_off)


@router.put("/{write_off_id}", response_model=WriteOffResponse)
async def update_write_off(
    write_off_id: UUID,
    data: WriteOffUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Оновлює списання."""
    result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .where(WriteOff.id == write_off_id)
    )
    write_off = result.scalar_one_or_none()
    if not write_off:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Списання з ID '{write_off_id}' не знайдено",
        )

    update_data = data.model_dump(exclude_unset=True, exclude={"items"})
    for field, value in update_data.items():
        setattr(write_off, field, value)

    if data.items is not None:
        for old_item in write_off.items:
            await session.delete(old_item)
        for item_data in data.items:
            item = WriteOffItem(
                write_off_id=write_off.id,
                product_id=item_data.product_id,
                quantity=item_data.quantity,
            )
            session.add(item)

    await session.flush()

    result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .where(WriteOff.id == write_off.id)
    )
    write_off = result.scalar_one()
    return WriteOffResponse.model_validate(write_off)


@router.delete("/{write_off_id}", status_code=204)
async def delete_write_off(
    write_off_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Видаляє списання."""
    result = await session.execute(
        select(WriteOff).where(WriteOff.id == write_off_id)
    )
    write_off = result.scalar_one_or_none()
    if not write_off:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Списання з ID '{write_off_id}' не знайдено",
        )
    await session.delete(write_off)
    await session.flush()


@router.post("/{write_off_id}/confirm", response_model=WriteOffResponse)
async def confirm_write_off(
    write_off_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Підтверджує списання (оновлює залишки товарів)."""
    doc_service = DocumentService(session)
    write_off = await doc_service.confirm_write_off(write_off_id)

    result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .where(WriteOff.id == write_off.id)
    )
    write_off = result.scalar_one()
    return WriteOffResponse.model_validate(write_off)
