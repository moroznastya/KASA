"""
API роутер для роботи зі списаннями товару (WriteOffs).

Ендпоінти:
  - GET    /write-offs            — список списань (з пагінацією)
  - GET    /write-offs/{id}       — отримати списання за ID
  - POST   /write-offs            — створити списання
  - PUT    /write-offs/{id}       — оновити списання
  - DELETE /write-offs/{id}       — видалити списання
  - POST   /write-offs/{id}/confirm — підтвердити списання
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, desc, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.infrastructure.persistence.models.write_off import WriteOff, WriteOffItem
from app.schemas.write_off import (
    WriteOffCreate,
    WriteOffUpdate,
    WriteOffResponse,
)
from app.domain.services.auth_service import AuthService


async def generate_write_off_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для списання.
    Формат: СП-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    from datetime import datetime
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"СП-{today}-"

    result = await session.execute(
        select(func.max(WriteOff.number))
        .where(WriteOff.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()
    last_seq = int(max_number[-3:]) if max_number else 0
    return f"{prefix}{last_seq + 1:03d}"



from app.domain.services.document_service import DocumentService

router = APIRouter(
    prefix="/write-offs",
    tags=["Списання"],
)


@router.get("", response_model=dict)
async def list_write_offs(
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список всіх списань з пагінацією.

    Повертає:
    - items: список списань
    - total: загальна кількість
    - page: поточна сторінка
    - page_size: розмір сторінки
    - pages: загальна кількість сторінок
    """
    # Загальна кількість
    count_result = await session.execute(
        select(func.count(WriteOff.id))
    )
    total = count_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .order_by(desc(WriteOff.created_at))
        .offset(offset)
        .limit(size)
    )
    write_offs = result.scalars().all()

    pages = max(1, (total + size - 1) // size) if total > 0 else 1

    return {
        "items": [WriteOffResponse.model_validate(w) for w in write_offs],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": pages,
    }


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
    current_user = Depends(AuthService.require_admin),
):
    """Створює нове списання."""
    # Автоматична генерація номера, якщо не вказано
    number = data.number
    if not number:
        number = await generate_write_off_number(session)

    write_off = WriteOff(
        number=number,
        reason=data.reason,
        write_off_date=data.write_off_date,
        notes=data.notes,
        created_by_id=current_user.id,
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
    current_user = Depends(AuthService.require_admin),
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
    current_user = Depends(AuthService.require_admin),
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
    current_user = Depends(AuthService.require_admin),
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
