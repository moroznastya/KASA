"""
API роутер для роботи з переміщеннями товару (Transfers).

Ендпоінти:
  - GET    /transfers            — список переміщень (з пагінацією)
  - GET    /transfers/{id}       — отримати переміщення за ID
  - POST   /transfers            — створити переміщення
  - PUT    /transfers/{id}       — оновити переміщення
  - DELETE /transfers/{id}       — видалити переміщення
  - POST   /transfers/{id}/confirm  — підтвердити переміщення
  - POST   /transfers/{id}/cancel   — скасувати переміщення
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, desc, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.infrastructure.persistence.models.transfer import Transfer, TransferItem, TransferStatus
from app.schemas.transfer import (
    TransferCreate,
    TransferUpdate,
    TransferResponse,
    TransferConfirmRequest,
)
from app.domain.services.auth_service import AuthService


async def generate_transfer_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для переміщення.
    Формат: ПМ-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    from datetime import datetime
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"ПМ-{today}-"

    result = await session.execute(
        select(func.max(Transfer.number))
        .where(Transfer.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()
    last_seq = int(max_number[-3:]) if max_number else 0
    return f"{prefix}{last_seq + 1:03d}"



from app.domain.services.document_service import DocumentService

router = APIRouter(
    prefix="/transfers",
    tags=["Переміщення"],
)


@router.get("", response_model=dict)
async def list_transfers(
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список всіх переміщень з пагінацією.

    Повертає:
    - items: список переміщень
    - total: загальна кількість
    - page: поточна сторінка
    - page_size: розмір сторінки
    - pages: загальна кількість сторінок
    """
    # Загальна кількість
    count_result = await session.execute(
        select(func.count(Transfer.id))
    )
    total = count_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    result = await session.execute(
        select(Transfer)
        .options(selectinload(Transfer.items))
        .order_by(desc(Transfer.created_at))
        .offset(offset)
        .limit(size)
    )
    transfers = result.scalars().all()

    pages = max(1, (total + size - 1) // size) if total > 0 else 1

    return {
        "items": [TransferResponse.model_validate(t) for t in transfers],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": pages,
    }


@router.get("/{transfer_id}", response_model=TransferResponse)
async def get_transfer(
    transfer_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує переміщення за ID."""
    result = await session.execute(
        select(Transfer)
        .options(selectinload(Transfer.items))
        .where(Transfer.id == transfer_id)
    )
    transfer = result.scalar_one_or_none()
    if not transfer:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Переміщення з ID '{transfer_id}' не знайдено",
        )
    return TransferResponse.model_validate(transfer)


@router.post("", response_model=TransferResponse, status_code=201)
async def create_transfer(
    data: TransferCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Створює нове переміщення."""
    # Автоматична генерація номера, якщо не вказано
    number = data.number
    if not number:
        number = await generate_transfer_number(session)

    transfer = Transfer(
        number=number,
        from_location=data.from_location,
        to_location=data.to_location,
        transfer_date=data.transfer_date,
        notes=data.notes,
        status=TransferStatus.DRAFT,
        created_by_id=current_user.id,
    )
    session.add(transfer)
    await session.flush()

    for item_data in data.items:
        item = TransferItem(
            transfer_id=transfer.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
        )
        session.add(item)

    await session.flush()

    result = await session.execute(
        select(Transfer)
        .options(selectinload(Transfer.items))
        .where(Transfer.id == transfer.id)
    )
    transfer = result.scalar_one()
    return TransferResponse.model_validate(transfer)


@router.put("/{transfer_id}", response_model=TransferResponse)
async def update_transfer(
    transfer_id: UUID,
    data: TransferUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Оновлює переміщення."""
    result = await session.execute(
        select(Transfer)
        .options(selectinload(Transfer.items))
        .where(Transfer.id == transfer_id)
    )
    transfer = result.scalar_one_or_none()
    if not transfer:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Переміщення з ID '{transfer_id}' не знайдено",
        )

    if transfer.status != TransferStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна редагувати тільки чернетки",
        )

    update_data = data.model_dump(exclude_unset=True, exclude={"items"})
    for field, value in update_data.items():
        setattr(transfer, field, value)

    if data.items is not None:
        for old_item in transfer.items:
            await session.delete(old_item)
        for item_data in data.items:
            item = TransferItem(
                transfer_id=transfer.id,
                product_id=item_data.product_id,
                quantity=item_data.quantity,
            )
            session.add(item)

    await session.flush()

    result = await session.execute(
        select(Transfer)
        .options(selectinload(Transfer.items))
        .where(Transfer.id == transfer.id)
    )
    transfer = result.scalar_one()
    return TransferResponse.model_validate(transfer)


@router.delete("/{transfer_id}", status_code=204)
async def delete_transfer(
    transfer_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Видаляє переміщення (тільки чернетку)."""
    result = await session.execute(
        select(Transfer).where(Transfer.id == transfer_id)
    )
    transfer = result.scalar_one_or_none()
    if not transfer:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Переміщення з ID '{transfer_id}' не знайдено",
        )
    if transfer.status != TransferStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна видалити тільки чернетку",
        )
    await session.delete(transfer)
    await session.flush()


@router.post("/{transfer_id}/confirm", response_model=TransferResponse)
async def confirm_transfer(
    transfer_id: UUID,
    data: TransferConfirmRequest,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Підтверджує або скасовує переміщення."""
    doc_service = DocumentService(session)

    if data.status == TransferStatus.CONFIRMED:
        transfer = await doc_service.confirm_transfer(transfer_id)
    elif data.status == TransferStatus.CANCELLED:
        transfer = await doc_service.cancel_transfer(transfer_id)
    else:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Невірний статус. Використовуйте 'confirmed' або 'cancelled'",
        )

    result = await session.execute(
        select(Transfer)
        .options(selectinload(Transfer.items))
        .where(Transfer.id == transfer.id)
    )
    transfer = result.scalar_one()
    return TransferResponse.model_validate(transfer)
