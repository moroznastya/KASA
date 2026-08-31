"""
API роутер для роботи з інвентаризацією (Inventory).

Ендпоінти:
  - GET    /inventory            — список інвентаризацій з підсумками (з пагінацією)
  - GET    /inventory/counts     — кількість інвентаризацій за статусами
  - GET    /inventory/{id}       — отримати інвентаризацію за ID з підсумками
  - POST   /inventory            — створити інвентаризацію
  - PUT    /inventory/{id}       — оновити інвентаризацію
  - DELETE /inventory/{id}       — видалити інвентаризацію (тільки чернетку)
  - POST   /inventory/{id}/confirm  — підтвердити/скасувати інвентаризацію
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import desc, func, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.domain.services.auth_service import AuthService
from app.infrastructure.persistence.models.inventory import Inventory, InventoryItem, InventoryStatus
from app.schemas.inventory import (
    InventoryConfirmRequest,
    InventoryCreate,
    InventoryItemResponse,
    InventoryResponse,
    InventorySummary,
    InventoryUpdate,
)


async def generate_inventory_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для інвентаризації.
    Формат: ІН-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    from datetime import datetime
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"ІН-{today}-"

    result = await session.execute(
        select(func.max(Inventory.number))
        .where(Inventory.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()
    last_seq = int(max_number[-3:]) if max_number else 0
    return f"{prefix}{last_seq + 1:03d}"



def calculate_item_totals(item: InventoryItemResponse) -> dict:
    """
    Розраховує total_cost та total_selling для позиції.

    Args:
        item: Об'єкт InventoryItemResponse.

    Returns:
        Словник з total_cost та total_selling.
    """
    return {
        "total_cost": item.actual_quantity * item.cost_price,
        "total_selling": item.actual_quantity * item.price,
    }


def calculate_summary(items: list[InventoryItemResponse]) -> dict:
    """
    Розраховує підсумки інвентаризації.

    Args:
        items: Список об'єктів InventoryItemResponse.

    Returns:
        Словник з total_cost, total_selling та total_deviation.
    """
    total_cost = sum(item.actual_quantity * item.cost_price for item in items)
    total_selling = sum(item.actual_quantity * item.price for item in items)
    total_deviation = sum(item.difference * item.cost_price for item in items)
    return {
        "total_cost": total_cost,
        "total_selling": total_selling,
        "total_deviation": total_deviation,
    }


from app.domain.services.document_service import DocumentService

router = APIRouter(
    prefix="/inventory",
    tags=["Інвентаризація"],
)


@router.get("", response_model=dict)
async def list_inventories(
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список всіх інвентаризацій з підсумками та пагінацією.

    Для кожної інвентаризації завантажує позиції (items) та розраховує підсумки:
    - total_cost: загальна сума собівартості (∑ actual_quantity * cost_price)
    - total_selling: загальна сума продажу (∑ actual_quantity * price)
    - total_deviation: загальна сума відхилення (∑ difference * cost_price)

    Повертає:
    - items: список інвентаризацій
    - total: загальна кількість
    - page: поточна сторінка
    - page_size: розмір сторінки
    - pages: загальна кількість сторінок
    """
    # Загальна кількість
    count_result = await session.execute(
        select(func.count(Inventory.id))
    )
    total = count_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    result = await session.execute(
        select(Inventory)
        .options(selectinload(Inventory.items).selectinload(InventoryItem.product))
        .order_by(desc(Inventory.created_at))
        .offset(offset)
        .limit(size)
    )
    inventories = result.scalars().all()

    pages = max(1, (total + size - 1) // size) if total > 0 else 1

    # Розрахувати підсумки для кожної інвентаризації
    response_list = []
    for inv in inventories:
        inv_response = InventoryResponse.model_validate(inv)
        items_data = [InventoryItemResponse.model_validate(item) for item in inv.items]
        inv_response.items = items_data
        inv_response.summary = InventorySummary(
            total_cost=sum(item.actual_quantity * item.cost_price for item in items_data),
            total_selling=sum(item.actual_quantity * item.price for item in items_data),
            total_deviation=sum(item.difference * item.cost_price for item in items_data),
        )
        response_list.append(inv_response)

    return {
        "items": [r.model_dump() for r in response_list],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": pages,
    }


@router.get("/counts")
async def get_inventory_counts(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Повертає кількість інвентаризацій за статусами."""
    total = await session.scalar(select(func.count(Inventory.id)))
    draft = await session.scalar(
        select(func.count(Inventory.id)).where(Inventory.status == InventoryStatus.DRAFT)
    )
    confirmed = await session.scalar(
        select(func.count(Inventory.id)).where(Inventory.status == InventoryStatus.CONFIRMED)
    )
    cancelled = await session.scalar(
        select(func.count(Inventory.id)).where(Inventory.status == InventoryStatus.CANCELLED)
    )

    return {
        "total": total or 0,
        "draft": draft or 0,
        "confirmed": confirmed or 0,
        "cancelled": cancelled or 0,
    }


@router.get("/{inventory_id}", response_model=InventoryResponse)
async def get_inventory(
    inventory_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує інвентаризацію за ID з підсумками.

    Повертає всі позиції (items) та розраховані підсумки (summary):
    - total_cost: загальна сума собівартості (∑ actual_quantity * cost_price)
    - total_selling: загальна сума продажу (∑ actual_quantity * price)
    - total_deviation: загальна сума відхилення (∑ difference * cost_price)
    """
    result = await session.execute(
        select(Inventory)
        .options(selectinload(Inventory.items).selectinload(InventoryItem.product))
        .where(Inventory.id == inventory_id)
    )
    inventory = result.scalar_one_or_none()
    if not inventory:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Інвентаризацію з ID '{inventory_id}' не знайдено",
        )

    # Перетворюємо в Pydantic
    inv_response = InventoryResponse.model_validate(inventory)

    # Розраховуємо підсумки
    items_data = [InventoryItemResponse.model_validate(item) for item in inventory.items]
    inv_response.items = items_data
    inv_response.summary = InventorySummary(
        total_cost=sum(item.actual_quantity * item.cost_price for item in items_data),
        total_selling=sum(item.actual_quantity * item.price for item in items_data),
        total_deviation=sum(item.difference * item.cost_price for item in items_data),
    )

    return inv_response


@router.post("", response_model=InventoryResponse, status_code=201)
async def create_inventory(
    data: InventoryCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Створює нову інвентаризацію з цінами (cost_price, price)."""
    # Автоматична генерація номера, якщо не вказано
    number = data.number
    if not number:
        number = await generate_inventory_number(session)

    inventory = Inventory(
        number=number,
        location=data.location or "",
        inventory_date=data.inventory_date,
        notes=data.notes,
        status=InventoryStatus.DRAFT,
        created_by_id=current_user.id,
    )
    session.add(inventory)
    await session.flush()

    for item_data in data.items:
        item = InventoryItem(
            inventory_id=inventory.id,
            product_id=item_data.product_id,
            actual_quantity=item_data.actual_quantity,
            accounting_quantity=item_data.accounting_quantity,
            difference=item_data.difference,
            cost_price=item_data.cost_price,
            price=item_data.price,
        )
        session.add(item)

    await session.flush()

    # Повертаємо з підсумками
    result = await session.execute(
        select(Inventory)
        .options(selectinload(Inventory.items).selectinload(InventoryItem.product))
        .where(Inventory.id == inventory.id)
    )
    inventory = result.scalar_one()

    inv_response = InventoryResponse.model_validate(inventory)
    items_data = [InventoryItemResponse.model_validate(item) for item in inventory.items]
    inv_response.items = items_data
    inv_response.summary = InventorySummary(
        total_cost=sum(item.actual_quantity * item.cost_price for item in items_data),
        total_selling=sum(item.actual_quantity * item.price for item in items_data),
        total_deviation=sum(item.difference * item.cost_price for item in items_data),
    )

    return inv_response


@router.put("/{inventory_id}", response_model=InventoryResponse)
async def update_inventory(
    inventory_id: UUID,
    data: InventoryUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Оновлює інвентаризацію."""
    result = await session.execute(
        select(Inventory)
        .options(selectinload(Inventory.items).selectinload(InventoryItem.product))
        .where(Inventory.id == inventory_id)
    )
    inventory = result.scalar_one_or_none()
    if not inventory:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Інвентаризацію з ID '{inventory_id}' не знайдено",
        )

    if inventory.status != InventoryStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна редагувати тільки чернетки",
        )

    update_data = data.model_dump(exclude_unset=True, exclude={"items"})
    for field, value in update_data.items():
        setattr(inventory, field, value)

    if data.items is not None:
        for old_item in inventory.items:
            await session.delete(old_item)
        for item_data in data.items:
            item = InventoryItem(
                inventory_id=inventory.id,
                product_id=item_data.product_id,
                actual_quantity=item_data.actual_quantity,
                accounting_quantity=item_data.accounting_quantity,
                difference=item_data.difference,
                cost_price=item_data.cost_price,
                price=item_data.price,
            )
            session.add(item)

    await session.flush()

    # Повертаємо з підсумками
    result = await session.execute(
        select(Inventory)
        .options(selectinload(Inventory.items).selectinload(InventoryItem.product))
        .where(Inventory.id == inventory.id)
    )
    inventory = result.scalar_one()

    inv_response = InventoryResponse.model_validate(inventory)
    items_data = [InventoryItemResponse.model_validate(item) for item in inventory.items]
    inv_response.items = items_data
    inv_response.summary = InventorySummary(
        total_cost=sum(item.actual_quantity * item.cost_price for item in items_data),
        total_selling=sum(item.actual_quantity * item.price for item in items_data),
        total_deviation=sum(item.difference * item.cost_price for item in items_data),
    )

    return inv_response


@router.delete("/{inventory_id}", status_code=204)
async def delete_inventory(
    inventory_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Видаляє інвентаризацію (тільки чернетку)."""
    result = await session.execute(
        select(Inventory).where(Inventory.id == inventory_id)
    )
    inventory = result.scalar_one_or_none()
    if not inventory:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Інвентаризацію з ID '{inventory_id}' не знайдено",
        )
    if inventory.status != InventoryStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна видалити тільки чернетку",
        )
    await session.delete(inventory)
    await session.flush()


@router.post("/{inventory_id}/confirm", response_model=InventoryResponse)
async def confirm_inventory(
    inventory_id: UUID,
    data: InventoryConfirmRequest,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Підтверджує або скасовує інвентаризацію."""
    doc_service = DocumentService(session)

    if data.status == InventoryStatus.CONFIRMED:
        inventory = await doc_service.confirm_inventory(inventory_id)
    elif data.status == InventoryStatus.CANCELLED:
        inventory = await doc_service.cancel_inventory(inventory_id)
    else:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Невірний статус. Використовуйте 'confirmed' або 'cancelled'",
        )

    # Повертаємо з підсумками
    result = await session.execute(
        select(Inventory)
        .options(selectinload(Inventory.items).selectinload(InventoryItem.product))
        .where(Inventory.id == inventory.id)
    )
    inventory = result.scalar_one()

    inv_response = InventoryResponse.model_validate(inventory)
    items_data = [InventoryItemResponse.model_validate(item) for item in inventory.items]
    inv_response.items = items_data
    inv_response.summary = InventorySummary(
        total_cost=sum(item.actual_quantity * item.cost_price for item in items_data),
        total_selling=sum(item.actual_quantity * item.price for item in items_data),
        total_deviation=sum(item.difference * item.cost_price for item in items_data),
    )

    return inv_response
