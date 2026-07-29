"""
API роутер для роботи з поверненнями постачальнику (ReturnInvoices).

Ендпоінти:
  - GET    /return-invoices            — список повернень (з пагінацією)
  - GET    /return-invoices/{id}       — отримати повернення за ID
  - POST   /return-invoices            — створити повернення
  - PUT    /return-invoices/{id}       — оновити повернення
  - DELETE /return-invoices/{id}       — видалити повернення
  - POST   /return-invoices/{id}/confirm  — підтвердити повернення
  - POST   /return-invoices/{id}/cancel   — скасувати повернення
"""

from uuid import UUID
from datetime import datetime

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, desc, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.infrastructure.persistence.models.return_invoice import ReturnInvoice, ReturnInvoiceItem, ReturnInvoiceStatus, ReturnActionType
from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem
from app.infrastructure.persistence.models.supplier import Supplier
from app.infrastructure.persistence.models.product import Product
from app.schemas.return_invoice import (
    ReturnInvoiceCreate,
    ReturnInvoiceUpdate,
    ReturnInvoiceResponse,
    ReturnInvoiceConfirmRequest,
)
from app.services.auth_service import AuthService
from app.services.document_service import DocumentService

router = APIRouter(
    prefix="/return-invoices",
    tags=["Повернення постачальнику"],
)


async def generate_return_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для повернення постачальнику.
    Формат: ПВ-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"ПВ-{today}-"

    # Знаходимо максимальний номер за сьогодні
    result = await session.execute(
        select(func.max(ReturnInvoice.number))
        .where(ReturnInvoice.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()

    if max_number:
        # Беремо останні 3 символи як номер
        last_seq = int(max_number[-3:])
        new_seq = last_seq + 1
    else:
        new_seq = 1

    return f"{prefix}{new_seq:03d}"


def calc_markup_percent(price: float, cost_price: float | None) -> float | None:
    """
    Обчислює відсоток націнки на основі ціни продажу та собівартості.
    Формула: ((price - cost_price) / cost_price) * 100
    """
    if cost_price and cost_price > 0 and price and price > 0:
        return round(((price - cost_price) / cost_price) * 100, 2)
    return None


async def get_product_cost_info(session: AsyncSession, product_id: UUID):
    """
    Отримує інформацію про собівартість товару.
    Повертає (cost_price, markup_percent).
    """
    result = await session.execute(
        select(Product).where(Product.id == product_id)
    )
    product = result.scalar_one_or_none()
    if product:
        return product.cost_price, None
    return None, None


@router.get("/", response_model=dict)
async def list_return_invoices(
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список повернень постачальнику з пагінацією.

    Повертає:
    - items: список повернень
    - total: загальна кількість
    - page: поточна сторінка
    - page_size: розмір сторінки
    - pages: загальна кількість сторінок
    """
    # Загальна кількість
    count_result = await session.execute(
        select(func.count(ReturnInvoice.id))
    )
    total = count_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    result = await session.execute(
        select(ReturnInvoice)
        .options(
            selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
            selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(ReturnInvoice.source_invoice),
            selectinload(ReturnInvoice.supplier),
        )
        .order_by(desc(ReturnInvoice.created_at))
        .offset(offset)
        .limit(size)
    )
    invoices = result.scalars().all()

    pages = max(1, (total + size - 1) // size) if total > 0 else 1

    response_list = []
    for inv in invoices:
        result_item = ReturnInvoiceResponse.model_validate(inv)
        result_item.supplier_name = inv.supplier.name if inv.supplier else None
        response_list.append(result_item)

    return {
        "items": [r.model_dump() for r in response_list],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": pages,
    }


@router.get("/{return_id}", response_model=ReturnInvoiceResponse)
async def get_return_invoice(
    return_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує повернення за ID."""
    result = await session.execute(
        select(ReturnInvoice)
        .options(
            selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
            selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(ReturnInvoice.source_invoice),
            selectinload(ReturnInvoice.supplier),
        )
        .where(ReturnInvoice.id == return_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Повернення з ID '{return_id}' не знайдено",
        )
    result_item = ReturnInvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item


@router.post("/", response_model=ReturnInvoiceResponse, status_code=201)
async def create_return_invoice(
    data: ReturnInvoiceCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Створює нове повернення постачальнику."""
    # Автоматична генерація номера, якщо не вказано
    number = data.number
    if not number:
        number = await generate_return_number(session)

    # Розраховуємо загальну суму з позицій, якщо не передана
    total_amount = data.total_amount
    if total_amount is None and data.items:
        total_amount = sum(item.total for item in data.items)

    # Валідація: якщо return_action = exchange, exchange_items обов'язкові
    if data.return_action == ReturnActionType.EXCHANGE and not data.exchange_items:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Для обміну (exchange) необхідно вказати exchange_items — "
                   "список товарів, на які відбувається обмін",
        )

    invoice = ReturnInvoice(
        number=number,
        supplier_id=data.supplier_id,
        return_date=data.return_date.replace(tzinfo=None) if data.return_date.tzinfo else data.return_date,
        return_action=data.return_action,
        is_fiscal=data.is_fiscal,
        notes=data.notes,
        total_amount=total_amount,
        source_invoice_id=data.source_invoice_id,
        status=ReturnInvoiceStatus.DRAFT,
        created_by_id=current_user.id,
    )
    session.add(invoice)
    await session.flush()

    for item_data in data.items:
        # Якщо cost_price не передано — беремо з продукту
        cost_price = item_data.cost_price
        if cost_price is None:
            cost_price, _ = await get_product_cost_info(session, item_data.product_id)
        # Обчислюємо націнку автоматично
        markup_percent = calc_markup_percent(
            float(item_data.price),
            float(cost_price) if cost_price else None,
        )

        item = ReturnInvoiceItem(
            return_invoice_id=invoice.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
            price=item_data.price,
            cost_price=cost_price,
            markup_percent=markup_percent,
            total=item_data.total,
        )
        session.add(item)

    await session.flush()

    result = await session.execute(
        select(ReturnInvoice)
        .options(
            selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
            selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(ReturnInvoice.source_invoice),
            selectinload(ReturnInvoice.supplier),
        )
        .where(ReturnInvoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    result_item = ReturnInvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item


@router.put("/{return_id}", response_model=ReturnInvoiceResponse)
async def update_return_invoice(
    return_id: UUID,
    data: ReturnInvoiceUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Оновлює повернення постачальнику."""
    result = await session.execute(
        select(ReturnInvoice)
        .options(
            selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
            selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(ReturnInvoice.source_invoice),
            selectinload(ReturnInvoice.supplier),
        )
        .where(ReturnInvoice.id == return_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Повернення з ID '{return_id}' не знайдено",
        )

    if invoice.status != ReturnInvoiceStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна редагувати тільки чернетки",
        )

    update_data = data.model_dump(exclude_unset=True, exclude={"items", "exchange_items"})
    for field, value in update_data.items():
        setattr(invoice, field, value)

    if data.items is not None:
        for old_item in invoice.items:
            await session.delete(old_item)
        for item_data in data.items:
            # Якщо cost_price не передано — беремо з продукту
            cost_price = item_data.cost_price
            if cost_price is None:
                cost_price, _ = await get_product_cost_info(session, item_data.product_id)
            # Обчислюємо націнку автоматично
            markup_percent = calc_markup_percent(
                float(item_data.price),
                float(cost_price) if cost_price else None,
            )

            item = ReturnInvoiceItem(
                return_invoice_id=invoice.id,
                product_id=item_data.product_id,
                quantity=item_data.quantity,
                price=item_data.price,
                cost_price=cost_price,
                markup_percent=markup_percent,
                total=item_data.total,
            )
            session.add(item)

    await session.flush()

    result = await session.execute(
        select(ReturnInvoice)
        .options(
            selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
            selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(ReturnInvoice.source_invoice),
            selectinload(ReturnInvoice.supplier),
        )
        .where(ReturnInvoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    result_item = ReturnInvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item


@router.delete("/{return_id}", status_code=204)
async def delete_return_invoice(
    return_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Видаляє повернення (тільки чернетку)."""
    result = await session.execute(
        select(ReturnInvoice).where(ReturnInvoice.id == return_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Повернення з ID '{return_id}' не знайдено",
        )
    if invoice.status != ReturnInvoiceStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна видалити тільки чернетку",
        )
    await session.delete(invoice)
    await session.flush()


@router.post("/{return_id}/confirm", response_model=ReturnInvoiceResponse)
async def confirm_return_invoice(
    return_id: UUID,
    data: ReturnInvoiceConfirmRequest,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """
    Підтверджує або скасовує повернення постачальнику.

    При підтвердженні з return_action = exchange:
    - Зменшує залишок повернутого товару
    - Створює прибуткову накладну на новий товар
    - Збільшує залишок нового товару
    - Зв'язує повернення з новою накладною

    exchange_items передаються в тілі запиту як частина ReturnInvoiceConfirmRequest.
    """
    doc_service = DocumentService(session)

    if data.status == ReturnInvoiceStatus.CONFIRMED:
        # Отримуємо повернення, щоб дізнатися return_action
        result = await session.execute(
            select(ReturnInvoice).where(ReturnInvoice.id == return_id)
        )
        return_invoice = result.scalar_one_or_none()
        if not return_invoice:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Повернення з ID '{return_id}' не знайдено",
            )

        # Конвертуємо exchange_items з Pydantic моделей в dict
        exchange_items = None
        if data.exchange_items:
            exchange_items = [
                {
                    "product_id": item.product_id,
                    "quantity": item.quantity,
                    "price": item.price,
                    "total": item.total,
                }
                for item in data.exchange_items
            ]

        invoice = await doc_service.confirm_return_invoice(
            return_id,
            exchange_items=exchange_items,
        )
    elif data.status == ReturnInvoiceStatus.CANCELLED:
        invoice = await doc_service.cancel_return_invoice(return_id)
    else:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Невірний статус. Використовуйте 'confirmed' або 'cancelled'",
        )

    result = await session.execute(
        select(ReturnInvoice)
        .options(
            selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
            selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(ReturnInvoice.source_invoice),
            selectinload(ReturnInvoice.supplier),
        )
        .where(ReturnInvoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    result_item = ReturnInvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item
