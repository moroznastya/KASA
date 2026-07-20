"""
API роутер для роботи з чеками продажу (Receipts).

Ендпоінти:
  - GET    /receipts            — історія чеків з фільтрацією
  - GET    /receipts/stats/today — статистика за сьогодні
  - GET    /receipts/{id}       — отримати чек за ID
  - POST   /receipts            — створити новий чек (продаж/повернення)
"""

from uuid import UUID
from datetime import datetime, date, timezone, timedelta
from decimal import Decimal

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


def _calc_vat(price: Decimal, quantity: Decimal, tax_rate: Decimal | None) -> Decimal:
    """
    Розраховує суму ПДВ для позиції.
    
    Формула: ПДВ = (ціна_з_пдв * ставка_пдв) / (1 + ставка_пдв)
    Якщо tax_rate=None або 0, ПДВ = 0.
    """
    if tax_rate is None or tax_rate == 0:
        return Decimal("0.00")
    
    rate = tax_rate / Decimal("100")  # 20% -> 0.20
    total = price * quantity
    vat = (total * rate) / (Decimal("1") + rate)
    return vat.quantize(Decimal("0.01"))


def _fill_product_names_and_profit(receipts: list[Receipt]) -> None:
    """
    Заповнює product_name, purchase_price, profit та vat_amount для кожного item в чеку.
    Мутує об'єкти ReceiptItem прямо в пам'яті.
    """
    for receipt in receipts:
        total_profit = Decimal(0)
        total_vat = Decimal(0)
        for item in receipt.items:
            if item.product:
                item.product_name = item.product.title
                # Якщо purchase_price не збережено, беремо з продукту
                if item.purchase_price is None and item.product.cost_price is not None:
                    item.purchase_price = float(item.product.cost_price)
            else:
                item.product_name = ""

            # Рахуємо прибуток для цієї позиції
            if item.purchase_price is not None:
                item_total = Decimal(str(item.total))
                item_cost = Decimal(str(item.purchase_price)) * Decimal(str(item.quantity))
                item_profit = item_total - item_cost
                total_profit += item_profit

            # Рахуємо ПДВ для цієї позиції
            tax_rate = None
            if item.product and item.product.tax_rate is not None:
                tax_rate = Decimal(str(item.product.tax_rate))
            item_vat = _calc_vat(
                price=Decimal(str(item.price)),
                quantity=Decimal(str(item.quantity)),
                tax_rate=tax_rate,
            )
            item._vat_amount = float(item_vat)
            total_vat += item_vat

        # Зберігаємо загальний прибуток та ПДВ по чеку (як атрибут, не в БД)
        receipt._total_profit = float(total_profit)
        receipt._vat_amount = float(total_vat)


def _normalize_date_to(dt: datetime | None) -> datetime | None:
    """
    Якщо date_to має час 00:00:00 (тобто передано лише дату без часу),
    встановлює 23:59:59.999999, щоб включити всі чеки за цей день.
    """
    if dt is None:
        return None
    if dt.hour == 0 and dt.minute == 0 and dt.second == 0 and dt.microsecond == 0:
        return dt.replace(hour=23, minute=59, second=59, microsecond=999999)
    return dt


@router.get("/stats/today")
async def get_today_stats(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Повертає статистику чеків за сьогодні (в UTC):
    - total_sales: загальна сума продажів
    - total_returns: загальна сума повернень
    - total_profit: загальний чистий прибуток
    - receipts_count: кількість чеків
    - items_sold: кількість проданих товарів
    """
    # Використовуємо UTC datetime без timezone (naive)
    now_utc = datetime.now(timezone.utc)
    today_start = datetime(now_utc.year, now_utc.month, now_utc.day)
    today_end = datetime(now_utc.year, now_utc.month, now_utc.day, 23, 59, 59, 999999)

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

    # Чистий прибуток за сьогодні
    profit_result = await session.execute(
        select(ReceiptItem)
        .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
        .where(Receipt.receipt_type == ReceiptType.SALE)
        .where(Receipt.created_at >= today_start)
        .where(Receipt.created_at <= today_end)
        .options(selectinload(ReceiptItem.product))
    )
    today_items = list(profit_result.scalars().all())
    total_profit = Decimal(0)
    total_vat = Decimal(0)
    for item in today_items:
        purchase_price = item.purchase_price
        if purchase_price is None and item.product and item.product.cost_price is not None:
            purchase_price = float(item.product.cost_price)
        if purchase_price is not None:
            item_total = Decimal(str(item.total))
            item_cost = Decimal(str(purchase_price)) * Decimal(str(item.quantity))
            total_profit += item_total - item_cost

        # ПДВ
        tax_rate = None
        if item.product and item.product.tax_rate is not None:
            tax_rate = Decimal(str(item.product.tax_rate))
        total_vat += _calc_vat(
            price=Decimal(str(item.price)),
            quantity=Decimal(str(item.quantity)),
            tax_rate=tax_rate,
        )

    return {
        "total_sales": total_sales,
        "total_returns": total_returns,
        "total_profit": float(total_profit),
        "total_vat": float(total_vat),
        "receipts_count": receipts_count,
        "items_sold": items_sold,
        "date": now_utc.strftime("%Y-%m-%d"),
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
    """
    # Нормалізуємо date_to: якщо передано лише дату (00:00:00), встановлюємо 23:59:59
    date_to = _normalize_date_to(date_to)

    query = select(Receipt)
    count_query = select(func.count(Receipt.id))

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

    total_result = await session.execute(count_query)
    total = total_result.scalar() or 0

    offset = (page - 1) * size
    query = (
        query
        .options(selectinload(Receipt.items).selectinload(ReceiptItem.product))
        .order_by(desc(Receipt.created_at))
        .offset(offset)
        .limit(size)
    )

    result = await session.execute(query)
    receipts = list(result.scalars().all())

    _fill_product_names_and_profit(receipts)

    # Формуємо відповідь з total_profit та vat_amount
    items_response = []
    for r in receipts:
        r_dict = ReceiptResponse.model_validate(r).model_dump()
        r_dict["total_profit"] = getattr(r, "_total_profit", 0)
        r_dict["vat_amount"] = getattr(r, "_vat_amount", 0)
        # Додаємо vat_amount до кожного item
        for item_dict, item_obj in zip(r_dict.get("items", []), r.items):
            item_dict["vat_amount"] = getattr(item_obj, "_vat_amount", 0)
        items_response.append(r_dict)

    return {
        "items": items_response,
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
        .options(
            selectinload(Receipt.items).selectinload(ReceiptItem.product)
        )
        .where(Receipt.id == receipt_id)
    )
    receipt = result.scalar_one_or_none()
    if not receipt:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Чек з ID '{receipt_id}' не знайдено",
        )

    _fill_product_names_and_profit([receipt])

    resp = ReceiptResponse.model_validate(receipt)
    resp.total_profit = Decimal(str(getattr(receipt, "_total_profit", 0)))
    resp.vat_amount = Decimal(str(getattr(receipt, "_vat_amount", 0)))
    return resp


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
    - Зберігає purchase_price (собівартість) для кожного товару
    """
    product_service = ProductService(session)

    # Генеруємо номер чеку, якщо не передано
    if not data.receipt_number:
        last_receipt = await session.execute(
            select(Receipt).order_by(desc(Receipt.created_at)).limit(1)
        )
        last = last_receipt.scalar_one_or_none()
        last_num = 0
        if last and last.receipt_number:
            try:
                last_num = int(last.receipt_number.split("-")[-1])
            except (ValueError, IndexError):
                pass
        data.receipt_number = f"RCPT-{datetime.now().strftime('%Y%m%d')}-{last_num + 1:04d}"

    cashier_id = data.cashier_id or current_user.id

    # Створюємо чек
    receipt = Receipt(
        receipt_number=data.receipt_number,
        receipt_type=data.receipt_type,
        cashier_id=cashier_id,
        total_amount=data.total_amount,
        is_return=data.is_return,
        notes=data.notes,
    )
    session.add(receipt)
    await session.flush()

    # Додаємо позиції та оновлюємо залишки
    for item_data in data.items:
        item_total = item_data.total if item_data.total is not None else item_data.quantity * item_data.price

        # Отримуємо собівартість товару
        product_result = await session.execute(
            select(Product).where(Product.id == item_data.product_id)
        )
        product = product_result.scalar_one_or_none()
        purchase_price = None
        if product and product.cost_price is not None:
            purchase_price = float(product.cost_price)

        item = ReceiptItem(
            receipt_id=receipt.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
            price=item_data.price,
            total=item_total,
            purchase_price=purchase_price,
        )
        session.add(item)

        # Оновлюємо залишок товару
        if data.receipt_type == ReceiptType.SALE:
            await product_service.update_stock(
                product_id=item_data.product_id,
                quantity_change=-item_data.quantity,
            )
        elif data.receipt_type == ReceiptType.RETURN:
            await product_service.update_stock(
                product_id=item_data.product_id,
                quantity_change=item_data.quantity,
            )

    await session.flush()

    # Повертаємо з позиціями та назвами товарів
    result = await session.execute(
        select(Receipt)
        .options(
            selectinload(Receipt.items).selectinload(ReceiptItem.product)
        )
        .where(Receipt.id == receipt.id)
    )
    receipt = result.scalar_one()

    _fill_product_names_and_profit([receipt])

    resp = ReceiptResponse.model_validate(receipt)
    resp.total_profit = Decimal(str(getattr(receipt, "_total_profit", 0)))
    resp.vat_amount = Decimal(str(getattr(receipt, "_vat_amount", 0)))
    return resp
