"""
API роутер для роботи з чеками продажу (Receipts).

Ендпоінти:
  - GET    /receipts                   — історія чеків з фільтрацією
  - GET    /receipts/stats/today       — статистика за сьогодні
  - GET    /receipts/search            — пошук чеків (для повернень)
  - GET    /receipts/by-barcode/{barcode}/recent-sales — останні продажі за штрих-кодом
  - GET    /receipts/products/{product_id}/returnable-quantity — скільки можна повернути
  - GET    /receipts/{receipt_id}/items        — товари чеку
  - GET    /receipts/{id}              — отримати чек за ID
  - POST   /receipts                   — створити новий чек (продаж/повернення)

⚠️ DEPRECATED: цей v1-роутер залишено для зворотної сумісності — використовуйте /api/v2/receipts/*.
"""

import contextlib
from datetime import UTC, datetime
from decimal import Decimal
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import desc, func, or_, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.application.services.settings_service import SettingsService
from app.database import get_session
from app.domain.services.auth_service import AuthService
from app.domain.services.product_service import ProductService
from app.domain.value_objects.rounding import round_amount
from app.infrastructure.persistence.models.debtor import Debtor, DebtorPayment
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem, ReceiptPaymentMethod, ReceiptType
from app.schemas.receipt import (
    ProductBriefInfo,
    ProductRecentSalesListResponse,
    ProductRecentSalesResponse,
    ReceiptCreate,
    ReceiptItemCreate,
    ReceiptItemResponse,
    ReceiptResponse,
    ReceiptSearchResult,
    RecentSaleInfo,
)

router = APIRouter(
    prefix="/receipts",
    tags=["Чеки продажу"],
)

# Константа — ID товару "Борг" (barcode: DEBT-PAYMENT)
DEBT_PRODUCT_ID = UUID("c230fe32-78ef-4501-a21d-71467a668fc4")


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
    Заповнює product_name, product_barcode, purchase_price, profit та vat_amount для кожного item в чеку.
    Мутує об'єкти ReceiptItem прямо в пам'яті.
    """
    for receipt in receipts:
        total_profit = Decimal(0)
        total_vat = Decimal(0)
        for item in receipt.items:
            if item.product:
                item.product_name = item.product.title
                item.product_barcode = item.product.barcode
                # Якщо purchase_price не збережено, беремо з продукту
                if item.purchase_price is None and item.product.cost_price is not None:
                    item.purchase_price = float(item.product.cost_price)
            else:
                item.product_name = ""
                item.product_barcode = None

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


async def get_settings_service(
    session: AsyncSession = Depends(get_session),
) -> SettingsService:
    """Dependency для отримання SettingsService."""
    return SettingsService(session)


async def get_returnable_quantity_for_product(
    session: AsyncSession,
    product_id: UUID,
) -> Decimal:
    """
    Повертає скільки одиниць товару ще можна повернути.
    Віднімає від загальної кількості проданого вже повернуту кількість.
    """
    # Продано
    sold_result = await session.execute(
        select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
        .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
        .where(ReceiptItem.product_id == product_id)
        .where(Receipt.receipt_type == ReceiptType.SALE)
    )
    total_sold = sold_result.scalar() or Decimal("0")

    # Вже повернуто
    returned_result = await session.execute(
        select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
        .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
        .where(ReceiptItem.product_id == product_id)
        .where(Receipt.receipt_type == ReceiptType.RETURN)
    )
    total_returned = returned_result.scalar() or Decimal("0")

    return max(Decimal("0"), total_sold - total_returned)


# ═══════════════════════════════════════════════════════════════════
# ЕНДПОІНТИ (впорядковано: спочатку статичні шляхи, потім динамічні)
# ═══════════════════════════════════════════════════════════════════


@router.get("/stats/today", deprecated=True)
async def get_today_stats(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
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
    now_utc = datetime.now(UTC)
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


@router.get("/search", response_model=dict, deprecated=True)
async def search_receipts(
    q: str = Query("", min_length=0, max_length=100, description="Пошук за номером чеку або назвою товару"),
    date_from: datetime = Query(None, description="Початкова дата"),
    date_to: datetime = Query(None, description="Кінцева дата"),
    receipt_type: ReceiptType = Query(ReceiptType.SALE, description="Тип чеку (за замовчуванням sale)"),
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Пошук оригінальних чеків для повернення.

    Шукає за номером чеку (ILIKE) або за назвою товару в позиціях.
    Фільтрує за датою та типом чеку.
    """
    date_to = _normalize_date_to(date_to)

    # Будуємо базовий запит з фільтрацією
    base_query = select(Receipt).where(Receipt.receipt_type == receipt_type)
    count_query = select(func.count(Receipt.id)).where(Receipt.receipt_type == receipt_type)

    if date_from:
        base_query = base_query.where(Receipt.created_at >= date_from)
        count_query = count_query.where(Receipt.created_at >= date_from)
    if date_to:
        base_query = base_query.where(Receipt.created_at <= date_to)
        count_query = count_query.where(Receipt.created_at <= date_to)

    # Пошук за номером чеку або назвою товару
    if q.strip():
        search_pattern = f"%{q.strip()}%"
        # Шукаємо чеки, де receipt_number містить q
        # АБО де в позиціях є товар з назвою, що містить q
        base_query = base_query.join(ReceiptItem, Receipt.id == ReceiptItem.receipt_id).join(
            Product, ReceiptItem.product_id == Product.id
        ).where(
            or_(
                Receipt.receipt_number.ilike(search_pattern),
                Product.title.ilike(search_pattern),
            )
        )
        count_query = count_query.join(ReceiptItem, Receipt.id == ReceiptItem.receipt_id).join(
            Product, ReceiptItem.product_id == Product.id
        ).where(
            or_(
                Receipt.receipt_number.ilike(search_pattern),
                Product.title.ilike(search_pattern),
            )
        )

    # Загальна кількість
    total_result = await session.execute(count_query)
    total = total_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    query = (
        base_query
        .options(
            selectinload(Receipt.items).selectinload(ReceiptItem.product),
            selectinload(Receipt.cashier),
        )
        .order_by(desc(Receipt.created_at))
        .offset(offset)
        .limit(size)
        .distinct()  # уникаємо дублікатів через JOIN
    )

    result = await session.execute(query)
    receipts = list(result.scalars().all())

    # Формуємо спрощену відповідь для пошуку
    items_result = []
    for r in receipts:
        cashier_name = r.cashier.name if r.cashier else ""
        items_count = len(r.items)
        items_result.append(ReceiptSearchResult(
            id=r.id,
            receipt_number=r.receipt_number,
            receipt_type=r.receipt_type,
            total_amount=Decimal(str(r.total_amount)),
            created_at=r.created_at,
            cashier_name=cashier_name,
            items_count=items_count,
        ))

    return {
        "items": [i.model_dump() for i in items_result],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": max(1, (total + size - 1) // size) if total > 0 else 1,
    }


@router.get("/by-product/{query}/recent-sales", deprecated=True)
async def get_recent_sales_by_product(
    query: str,
    limit: int = Query(5, ge=1, le=20, description="Кількість останніх продажів"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує останні продажі товару за штрих-кодом або назвою (для повернення без чеку).

    Повертає СПИСОК усіх товарів, що відповідають запиту, з інформацією
    про останні продажі, кількість проданого/повернутого для кожного.
    """
    # 1. Знайти ВСІ товари за штрих-кодом (точний збіг) або за назвою (ILIKE)
    product_result = await session.execute(
        select(Product).where(
            or_(
                Product.barcode == query,
                Product.title.ilike(f"%{query}%"),
            )
        ).order_by(Product.title).limit(20)  # обмежуємо 20, щоб не перевантажити
    )
    products = list(product_result.scalars().all())

    if not products:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Товарів за запитом '{query}' не знайдено. Спробуйте ввести штрих-код або назву товару",
        )

    # 2. Для кожного товару збираємо дані
    items_list = []
    for product in products:
        # 2a. Знайти останні N продажів цього товару
        recent_items_result = await session.execute(
            select(ReceiptItem)
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(ReceiptItem.product_id == product.id)
            .where(Receipt.receipt_type == ReceiptType.SALE)
            .order_by(desc(Receipt.created_at))
            .limit(limit)
            .options(selectinload(ReceiptItem.receipt))
        )
        recent_items = list(recent_items_result.scalars().all())

        # 2b. Порахувати загальну кількість проданого та повернутого
        total_sold_result = await session.execute(
            select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(ReceiptItem.product_id == product.id)
            .where(Receipt.receipt_type == ReceiptType.SALE)
        )
        total_sold = total_sold_result.scalar() or Decimal("0")

        total_returned_result = await session.execute(
            select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(ReceiptItem.product_id == product.id)
            .where(Receipt.receipt_type == ReceiptType.RETURN)
        )
        total_returned = total_returned_result.scalar() or Decimal("0")

        returnable = max(Decimal("0"), total_sold - total_returned)

        # 2c. Формуємо recent_sales для цього товару
        recent_sales = []
        for item in recent_items:
            recent_sales.append(RecentSaleInfo(
                receipt_id=item.receipt_id,
                receipt_number=item.receipt.receipt_number if item.receipt else "",
                created_at=item.created_at,
                quantity=Decimal(str(item.quantity)),
                price=Decimal(str(item.price)),
            ))

        product_info = ProductBriefInfo(
            id=product.id,
            title=product.title,
            barcode=product.barcode,
            price=Decimal(str(product.price)),
            unit=product.unit,
        )

        resp_item = ProductRecentSalesResponse(
            product=product_info,
            total_sold=total_sold,
            total_returned=total_returned,
            returnable=returnable,
            recent_sales=recent_sales,
        )
        items_list.append(resp_item)

    # 3. Повертаємо список
    return ProductRecentSalesListResponse(
        items=items_list,
        total=len(items_list),
    ).model_dump()


@router.get("/products/{product_id}/returnable-quantity", deprecated=True)
async def get_returnable_quantity(
    product_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Перевіряє скільки одиниць товару можна повернути.

    Повертає загальну кількість проданого, вже повернутого
    та скільки ще можна повернути.
    """
    # Перевіряємо чи існує товар
    product_result = await session.execute(
        select(Product).where(Product.id == product_id)
    )
    product = product_result.scalar_one_or_none()
    if not product:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Товар з ID '{product_id}' не знайдено",
        )

    returnable = await get_returnable_quantity_for_product(session, product_id)

    # Окремо отримуємо total_sold та total_returned для відповіді
    sold_result = await session.execute(
        select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
        .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
        .where(ReceiptItem.product_id == product_id)
        .where(Receipt.receipt_type == ReceiptType.SALE)
    )
    total_sold = sold_result.scalar() or Decimal("0")

    returned_result = await session.execute(
        select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
        .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
        .where(ReceiptItem.product_id == product_id)
        .where(Receipt.receipt_type == ReceiptType.RETURN)
    )
    total_returned = returned_result.scalar() or Decimal("0")

    return {
        "product_id": str(product_id),
        "total_sold": float(total_sold),
        "total_returned": float(total_returned),
        "returnable": float(returnable),
    }


@router.get("/{receipt_id}/items", response_model=list[ReceiptItemResponse], deprecated=True)
async def get_receipt_items(
    receipt_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує всі товари (позиції) для вказаного чеку.
    Використовується для вибору товарів при поверненні.
    """
    # Перевіряємо чи існує чек
    receipt_result = await session.execute(
        select(Receipt).where(Receipt.id == receipt_id)
    )
    receipt = receipt_result.scalar_one_or_none()
    if not receipt:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Чек з ID '{receipt_id}' не знайдено",
        )

    # Отримуємо позиції з підвантаженими товарами
    items_result = await session.execute(
        select(ReceiptItem)
        .where(ReceiptItem.receipt_id == receipt_id)
        .options(selectinload(ReceiptItem.product))
        .order_by(ReceiptItem.created_at)
    )
    items = list(items_result.scalars().all())

    # Заповнюємо назви та штрих-коди товарів
    result = []
    for item in items:
        item_dict = ReceiptItemResponse.model_validate(item)
        if item.product:
            item_dict.product_name = item.product.title
            item_dict.product_barcode = item.product.barcode
        result.append(item_dict)

    return result


@router.get("/{receipt_id}", response_model=ReceiptResponse, deprecated=True)
async def get_receipt(
    receipt_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує чек за ID."""
    result = await session.execute(
        select(Receipt)
        .options(
            selectinload(Receipt.items).selectinload(ReceiptItem.product),
            selectinload(Receipt.cashier),
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
    resp.change_amount = Decimal(str(getattr(receipt, "change_amount", 0)))
    resp.cashier_name = receipt.cashier.name if receipt.cashier else "Невідомо"
    resp.payment_method = receipt.payment_method.value if receipt.payment_method else None
    return resp


@router.get("", response_model=dict, deprecated=True)
async def list_receipts(
    cashier_id: UUID = Query(None, description="Фільтр за касиром"),
    receipt_type: ReceiptType = Query(None, description="Фільтр за типом"),
    date_from: datetime = Query(None, description="Початкова дата"),
    date_to: datetime = Query(None, description="Кінцева дата"),
    page: int = Query(1, ge=1, description="Сторінка"),
    size: int = Query(20, ge=1, le=100, description="Елементів на сторінці"),
    payment_method: ReceiptPaymentMethod = Query(None, description="Фільтр за способом оплати"),
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
    if payment_method:
        query = query.where(Receipt.payment_method == payment_method)
        count_query = count_query.where(Receipt.payment_method == payment_method)

    total_result = await session.execute(count_query)
    total = total_result.scalar() or 0

    offset = (page - 1) * size
    query = (
        query
        .options(
            selectinload(Receipt.items).selectinload(ReceiptItem.product),
            selectinload(Receipt.cashier),
        )
        .order_by(desc(Receipt.created_at))
        .offset(offset)
        .limit(size)
    )

    result = await session.execute(query)
    receipts = list(result.scalars().all())

    _fill_product_names_and_profit(receipts)

    # Формуємо відповідь з total_profit, vat_amount, cashier_name, payment_method
    items_response = []
    for r in receipts:
        r_dict = ReceiptResponse.model_validate(r).model_dump()
        r_dict["total_profit"] = getattr(r, "_total_profit", 0)
        r_dict["vat_amount"] = getattr(r, "_vat_amount", 0)
        r_dict["cashier_name"] = r.cashier.name if r.cashier else "Невідомо"
        r_dict["payment_method"] = r.payment_method.value if r.payment_method else None
        # Додаємо vat_amount до кожного item
        for item_dict, item_obj in zip(r_dict.get("items", []), r.items, strict=False):
            item_dict["vat_amount"] = getattr(item_obj, "_vat_amount", 0)
        items_response.append(r_dict)

    return {
        "items": items_response,
        "total": total,
        "page": page,
        "page_size": size,
        "pages": max(1, (total + size - 1) // size) if total > 0 else 1,
    }


@router.post("", response_model=ReceiptResponse, status_code=201, deprecated=True)
async def create_receipt(
    data: ReceiptCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
    settings_service: SettingsService = Depends(get_settings_service),
):
    """
    Створює новий чек (продаж або повернення).

    Підтримує два режими роботи з боргом:

    1. **Звичайний борг** (`debtor_id` вказано, але `debt_payment` не вказано):
       - Якщо `paid_amount < total_amount` — різниця додається до боргу боржника

    2. **Оплата боргу через касу** (`debt_payment` вказано):
       - Створюється чек з товаром "Борг" (barcode: DEBT-PAYMENT)
       - Створюється запис `DebtorPayment` (зменшення боргу)
       - Залишок товару "Борг" НЕ оновлюється
       - Якщо борг став 0 — боржник автоматично видаляється

    Для SALE: зменшує залишки товарів на складі (крім товару "Борг")
    Для RETURN: збільшує залишки товарів на складі

    Інтегровані системні налаштування:
    - `allow_negative_stock` (bool): дозволяє продаж при нульовому залишку
    - `price_rounding` (select): код заокруглення суми в чеку (1, 10, 50, 100, 500)
    """
    product_service = ProductService(session)
    is_debt_payment = data.debt_payment is not None

    # ─── Валідація та підготовка оплати боргу ─────────────────────
    debt_payment_debtor = None
    if is_debt_payment:
        # ВАЛІДАЦІЯ: перевірити чи існує боржник
        debtor_result = await session.execute(
            select(Debtor).where(Debtor.id == data.debt_payment.debtor_id)
        )
        debt_payment_debtor = debtor_result.scalar_one_or_none()
        if not debt_payment_debtor:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Боржника з ID '{data.debt_payment.debtor_id}' не знайдено",
            )

        # ВАЛІДАЦІЯ: перевірити чи не перевищує сума оплати поточний борг
        current_debt = Decimal(str(debt_payment_debtor.total_debt))
        if data.debt_payment.amount > current_debt:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=(
                    f"Сума оплати боргу ({data.debt_payment.amount}) "
                    f"перевищує поточний борг ({current_debt})"
                ),
            )

        # ВАЛІДАЦІЯ: перевірити чи є товар "Борг" серед items
        has_debt_item = any(
            item.product_id == DEBT_PRODUCT_ID for item in data.items
        )
        if not has_debt_item:
            # Якщо немає — додати автоматично позицію з товаром "Борг"
            debt_amount = data.debt_payment.amount
            data.items.append(ReceiptItemCreate(
                product_id=DEBT_PRODUCT_ID,
                quantity=Decimal("1"),
                price=debt_amount,
                total=debt_amount,
            ))

        # Встановлюємо debtor_id для зв'язку з чеком
        data.debtor_id = data.debt_payment.debtor_id

    # ─── Генерація номера чеку ────────────────────────────────────
    if not data.receipt_number:
        last_receipt = await session.execute(
            select(Receipt).order_by(desc(Receipt.created_at)).limit(1)
        )
        last = last_receipt.scalar_one_or_none()
        last_num = 0
        if last and last.receipt_number:
            with contextlib.suppress(ValueError, IndexError):
                last_num = int(last.receipt_number.split("-")[-1])
        data.receipt_number = f"RCPT-{datetime.now().strftime('%Y%m%d')}-{last_num + 1:04d}"

    cashier_id = data.cashier_id or current_user.id

    # Визначаємо paid_amount: якщо не передано — повна оплата
    paid_amount = data.paid_amount
    if paid_amount is None:
        paid_amount = data.total_amount

    # Валідація: paid_amount не може бути від'ємним
    if paid_amount < 0:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Сума оплати (paid_amount) не може бути від'ємною",
        )

    # ─── Валідація кількості для повернень ─────────────────────────
    if data.receipt_type == ReceiptType.RETURN:
        for item in data.items:
            # Пропускаємо товар "Борг"
            if item.product_id == DEBT_PRODUCT_ID:
                continue

            # Отримуємо скільки можна повернути
            returnable = await get_returnable_quantity_for_product(session, item.product_id)
            if item.quantity > returnable:
                # Отримуємо назву товару для повідомлення
                product_result = await session.execute(
                    select(Product).where(Product.id == item.product_id)
                )
                product = product_result.scalar_one_or_none()
                product_name = product.title if product else str(item.product_id)

                raise HTTPException(
                    status_code=status.HTTP_400_BAD_REQUEST,
                    detail=(
                        f"Товар '{product_name}': можна повернути не більше "
                        f"{returnable} од. (продано: {returnable}, "
                        f"вже повернуто: {0})"
                    ),
                )

    # Обчислюємо здачу (change) якщо paid_amount > total_amount
    change_amount = Decimal("0.00")
    if paid_amount > data.total_amount:
        change_amount = paid_amount - data.total_amount
        # Зберігаємо оригінальний paid_amount (скільки клієнт фактично вніс)
        # Не змінюємо paid_amount на total_amount

    # ─── Заокруглення суми чеку ───────────────────────────────────
    rounding_code = await settings_service.get_int("price_rounding", 1)
    if rounding_code != 1:
        rounded_total = round_amount(data.total_amount, rounding_code)
        # Коригуємо paid_amount якщо він дорівнював total_amount
        if paid_amount == data.total_amount:
            paid_amount = rounded_total
        data.total_amount = rounded_total

    # ─── Отримуємо debtor (якщо вказано) ──────────────────────────
    debtor = None
    if data.debtor_id and not is_debt_payment:
        # Для звичайного боргу — отримуємо боржника
        debtor_result = await session.execute(
            select(Debtor).where(Debtor.id == data.debtor_id)
        )
        debtor = debtor_result.scalar_one_or_none()
        if not debtor:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Боржника з ID '{data.debtor_id}' не знайдено",
            )
    elif is_debt_payment:
        # Для оплати боргу — використовуємо вже знайденого боржника
        debtor = debt_payment_debtor

    # ─── Створюємо чек ────────────────────────────────────────────
    receipt = Receipt(
        receipt_number=data.receipt_number,
        receipt_type=data.receipt_type,
        cashier_id=cashier_id,
        total_amount=data.total_amount,
        paid_amount=paid_amount,
        change_amount=change_amount if change_amount > 0 else None,
        debtor_id=data.debtor_id,
        payment_method=data.payment_method,
        is_return=data.is_return,
        notes=data.notes,
        original_receipt_id=data.original_receipt_id,
    )
    session.add(receipt)
    await session.flush()

    # ─── Додаємо позиції та оновлюємо залишки ─────────────────────
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

        # Оновлюємо залишок товару (тільки якщо це НЕ товар "Борг")
        is_debt_item = item_data.product_id == DEBT_PRODUCT_ID
        if not is_debt_item:
            if data.receipt_type == ReceiptType.SALE:
                try:
                    await product_service.update_stock(
                        product_id=item_data.product_id,
                        quantity_change=-item_data.quantity,
                    )
                except HTTPException as e:
                    # Якщо allow_negative_stock=True — ігноруємо помилку
                    # недостатнього залишку
                    if e.status_code == 400:
                        allow_negative = await settings_service.get_bool(
                            "allow_negative_stock", False
                        )
                        if not allow_negative:
                            raise
                        # Якщо дозволено — оновлюємо залишок вручну
                        if product.stock is None:
                            product.stock = -item_data.quantity
                        else:
                            product.stock += -item_data.quantity
                        await session.flush()
                    else:
                        raise
            elif data.receipt_type == ReceiptType.RETURN:
                await product_service.update_stock(
                    product_id=item_data.product_id,
                    quantity_change=item_data.quantity,
                )

    # ─── Логіка боргу ─────────────────────────────────────────────
    if is_debt_payment:
        # ── Оплата боргу через касу ──
        # Створюємо запис про оплату боргу
        payment = DebtorPayment(
            debtor_id=data.debt_payment.debtor_id,
            amount=data.debt_payment.amount,
            payment_method='cash',  # оплата через касу — завжди готівка
        )
        session.add(payment)

        # Зменшуємо загальний борг боржника
        current_debt = Decimal(str(debt_payment_debtor.total_debt))
        debt_payment_debtor.total_debt = current_debt - data.debt_payment.amount

        # Якщо борг став 0 або менше — видаляємо боржника
        if float(debt_payment_debtor.total_debt) <= 0:
            await session.delete(debt_payment_debtor)

    elif debtor and paid_amount < data.total_amount:
        # ── Звичайний борг: додаємо різницю до боргу ──
        debt_amount = data.total_amount - paid_amount
        current_debt = Decimal(str(debtor.total_debt))
        debtor.total_debt = current_debt + debt_amount

        # Якщо борг став 0 або менше — видаляємо боржника
        if float(debtor.total_debt) <= 0:
            await session.delete(debtor)

    await session.commit()

    # ─── Повертаємо створений чек з позиціями ────────────────────
    result = await session.execute(
        select(Receipt)
        .options(
            selectinload(Receipt.items).selectinload(ReceiptItem.product),
            selectinload(Receipt.cashier),
        )
        .where(Receipt.id == receipt.id)
    )
    receipt = result.scalar_one()

    _fill_product_names_and_profit([receipt])

    resp = ReceiptResponse.model_validate(receipt)
    resp.total_profit = Decimal(str(getattr(receipt, "_total_profit", 0)))
    resp.vat_amount = Decimal(str(getattr(receipt, "_vat_amount", 0)))
    resp.change_amount = Decimal(str(getattr(receipt, "change_amount", 0)))
    resp.cashier_name = receipt.cashier.name if receipt.cashier else "Невідомо"
    resp.payment_method = receipt.payment_method.value if receipt.payment_method else None
    return resp
