"""
API роутер для роботи з чеками продажу (Receipts).

Ендпоінти:
  - GET    /receipts            — історія чеків з фільтрацією
  - GET    /receipts/stats/today — статистика за сьогодні
  - GET    /receipts/{id}       — отримати чек за ID
  - POST   /receipts            — створити новий чек (продаж/повернення)
"""

from uuid import UUID
from datetime import datetime, date

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, desc, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.models.receipt import Receipt, ReceiptItem, ReceiptType
from app.models.product import Product
from app.schemas.receipt import (
    ReceiptCreate,
    ReceiptResponse,
    ReceiptItemResponse,
)
from app.services.auth_service import AuthService
from app.services.product_service import ProductService

router = APIRouter(
    prefix="/receipts",
    tags=["Чеки продажу"],
)


@router.get("/stats/today")
async def get_today_stats(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Повертає статистику чеків за сьогодні:
    - total_sales: загальна сума продажів
    - total_returns: загальна сума повернень
    - receipts_count: кількість чеків
    - items_sold: кількість проданих товарів
    """
    today_start = datetime.combine(date.today(), datetime.min.time())
    today_end = datetime.combine(date.today(), datetime.max.time())

    # Загальна сума продажів сьогодні
    sales_sum = await session.execute(
        select(func.coalesce(func.sum(Receipt.total_amount), 0))
        .where(Receipt.receipt_type == ReceiptType.SALE)
        .where(Receipt.created_at >= today_start)
        .where(Receipt.created_at <= today_end)
    )
    total_sales = float(sales_sum.scalar() or 0)

    # Загальна сума повернень сьогодні
    returns_sum = await session.execute(
        select(func.coalesce(func.sum(Receipt.total_amount), 0))
        .where(Receipt.receipt_type == ReceiptType.RETURN)
        .where(Receipt.created_at >= today_start)
        .where(Receipt.created_at <= today_end)
    )
    total_returns = float(returns_sum.scalar() or 0)

    # Кількість чеків сьогодні
    count_result = await session.execute(
        select(func.count(Receipt.id))
        .where(Receipt.created_at >= today_start)
        .where(Receipt.created_at <= today_end)
    )
    receipts_count = count_result.scalar() or 0

    # Кількість проданих товарів сьогодні
    items_result = await session.execute(
        select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
        .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
        .where(Receipt.receipt_type == ReceiptType.SALE)
        .where(Receipt.created_at >= today_start)
        .where(Receipt.created_at <= today_end)
    )
    items_sold = int(items_result.scalar() or 0)

    return {
        "total_sales": total_sales,
        "total_returns": total_returns,
        "receipts_count": receipts_count,
        "items_sold": items_sold,
        "date": date.today().isoformat(),
    }


@router.get("", response_model=dict)
async def list_receipts(
    cashier_id: UUID = Query(None, description="Фільтр за касиром"),
    receipt_type: ReceiptType = Query(None, description="Фільтр за типом"),
    date_from: datetime = Query(None, description="Початкова дата"),
    date_to: datetime = Query(None, description="Кінцева дата"),
    page: int = Query(1, ge=1, description="Сторінка"),
    size: int = Query(20, ge=1, le=100, description="Елементів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує історію чеків з фільтрацією та пагінацією.

    Підтримує фільтрацію за:
    - Касиром (cashier_id)
    - Типом чеку (sale/return)
    - Діапазоном дат
    """
    # Базовий запит
    query = select(Receipt)
    count_query = select(func.count(Receipt.id))

    # Фільтри
    if cashier_id:
        query = query.where(Receipt.cashier_id == cashier_id)
        count_query = count_query.where(Receipt.cashier_id == cashier_id)
    if receipt_type:
        query = query.where(Receipt.receipt_type == receipt_type)
        count_query = count_query.where(Receipt.receipt_type == receipt_type)
    if date_from:
        query = query.where(Receipt.created_at >= date_from)
        count_query = count_query.where(Receipt.created_at >= date_from)
    if date_to:
        query = query.where(Receipt.created_at <= date_to)
        count_query = count_query.where(Receipt.created_at <= date_to)

    # Загальна кількість
    total_result = await session.execute(count_query)
    total = total_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    query = (
        query
        .options(selectinload(Receipt.items))
        .order_by(desc(Receipt.created_at))
        .offset(offset)
        .limit(size)
    )

    result = await session.execute(query)
    receipts = result.scalars().all()

    return {
        "items": [ReceiptResponse.model_validate(r) for r in receipts],
        "total": total,
        "page": page,
        "size": size,
    }


@router.get("/{receipt_id}", response_model=ReceiptResponse)
async def get_receipt(
    receipt_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує чек за ID."""
    result = await session.execute(
        select(Receipt)
        .options(selectinload(Receipt.items))
        .where(Receipt.id == receipt_id)
    )
    receipt = result.scalar_one_or_none()
    if not receipt:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Чек з ID '{receipt_id}' не знайдено",
        )
    return ReceiptResponse.model_validate(receipt)


@router.post("", response_model=ReceiptResponse, status_code=201)
async def create_receipt(
    data: ReceiptCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Створює новий чек (продаж або повернення).

    При створенні чеку:
    - Для SALE: зменшує залишки товарів на складі
    - Для RETURN: збільшує залишки товарів на складі
    """
    product_service = ProductService(session)

    # Створюємо чек
    receipt = Receipt(
        receipt_number=data.receipt_number,
        receipt_type=data.receipt_type,
        cashier_id=data.cashier_id,
        total_amount=data.total_amount,
        is_return=data.is_return,
        notes=data.notes,
    )
    session.add(receipt)
    await session.flush()

    # Додаємо позиції та оновлюємо залишки
    for item_data in data.items:
        item = ReceiptItem(
            receipt_id=receipt.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
            price=item_data.price,
            total=item_data.total,
        )
        session.add(item)

        # Оновлюємо залишок товару
        if data.receipt_type == ReceiptType.SALE:
            # Продаж: зменшуємо залишок
            await product_service.update_stock(
                product_id=item_data.product_id,
                quantity_change=-item_data.quantity,
            )
        elif data.receipt_type == ReceiptType.RETURN:
            # Повернення: збільшуємо залишок
            await product_service.update_stock(
                product_id=item_data.product_id,
                quantity_change=item_data.quantity,
            )

    await session.flush()

    # Повертаємо з позиціями
    result = await session.execute(
        select(Receipt)
        .options(selectinload(Receipt.items))
        .where(Receipt.id == receipt.id)
    )
    receipt = result.scalar_one()
    return ReceiptResponse.model_validate(receipt)
