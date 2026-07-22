"""
API роутер для роботи із замовленнями постачальнику (PurchaseOrders).

Ендпоінти:
  - GET    /purchase-orders            — список замовлень
  - GET    /purchase-orders/{id}       — отримати замовлення за ID
  - POST   /purchase-orders            — створити замовлення
  - PUT    /purchase-orders/{id}       — оновити замовлення
  - DELETE /purchase-orders/{id}       — видалити замовлення
  - POST   /purchase-orders/{id}/confirm  — підтвердити замовлення
  - POST   /purchase-orders/{id}/cancel   — скасувати замовлення
"""

from uuid import UUID
from datetime import datetime

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select, desc, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.models.purchase_order import PurchaseOrder, PurchaseOrderItem, PurchaseOrderStatus
from app.models.invoice import Invoice, InvoiceItem, InvoiceStatus, PaymentMethod
from app.schemas.purchase_order import (
    PurchaseOrderCreate,
    PurchaseOrderUpdate,
    PurchaseOrderResponse,
    PurchaseOrderConfirmRequest,
)
from app.services.auth_service import AuthService
from app.services.product_service import ProductService

router = APIRouter(
    prefix="/purchase-orders",
    tags=["Замовлення постачальнику"],
)


async def generate_order_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для замовлення постачальнику.
    Формат: ЗАМ-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"ЗАМ-{today}-"

    result = await session.execute(
        select(func.max(PurchaseOrder.number))
        .where(PurchaseOrder.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()

    if max_number:
        last_seq = int(max_number[-3:])
        new_seq = last_seq + 1
    else:
        new_seq = 1

    return f"{prefix}{new_seq:03d}"


async def generate_invoice_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для прибуткової накладної.
    Формат: ПН-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"ПН-{today}-"

    result = await session.execute(
        select(func.max(Invoice.number))
        .where(Invoice.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()

    if max_number:
        last_seq = int(max_number[-3:])
        new_seq = last_seq + 1
    else:
        new_seq = 1

    return f"{prefix}{new_seq:03d}"


@router.get("", response_model=list[PurchaseOrderResponse])
async def list_purchase_orders(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує список всіх замовлень постачальнику."""
    result = await session.execute(
        select(PurchaseOrder)
        .options(
            selectinload(PurchaseOrder.items).selectinload(PurchaseOrderItem.product),
            selectinload(PurchaseOrder.invoice),
        )
        .order_by(desc(PurchaseOrder.created_at))
    )
    orders = result.scalars().all()
    return [PurchaseOrderResponse.model_validate(order) for order in orders]


@router.get("/{order_id}", response_model=PurchaseOrderResponse)
async def get_purchase_order(
    order_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує замовлення за ID."""
    result = await session.execute(
        select(PurchaseOrder)
        .options(
            selectinload(PurchaseOrder.items).selectinload(PurchaseOrderItem.product),
            selectinload(PurchaseOrder.invoice),
        )
        .where(PurchaseOrder.id == order_id)
    )
    order = result.scalar_one_or_none()
    if not order:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Замовлення з ID '{order_id}' не знайдено",
        )
    return PurchaseOrderResponse.model_validate(order)


@router.post("", response_model=PurchaseOrderResponse, status_code=201)
async def create_purchase_order(
    data: PurchaseOrderCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Створює нове замовлення постачальнику."""
    # Автоматична генерація номера, якщо не вказано
    number = data.number
    if not number:
        number = await generate_order_number(session)

    # Розраховуємо загальну суму з позицій, якщо не передана
    total_amount = data.total_amount
    if total_amount is None and data.items:
        total_amount = sum(item.total for item in data.items)

    order = PurchaseOrder(
        number=number,
        supplier_id=data.supplier_id,
        order_date=data.order_date.replace(tzinfo=None) if data.order_date.tzinfo else data.order_date,
        expected_date=data.expected_date.replace(tzinfo=None) if data.expected_date and data.expected_date.tzinfo else data.expected_date,
        is_fiscal=data.is_fiscal,
        notes=data.notes,
        total_amount=total_amount,
        status=PurchaseOrderStatus.DRAFT,
    )
    session.add(order)
    await session.flush()

    for item_data in data.items:
        item = PurchaseOrderItem(
            purchase_order_id=order.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
            price=item_data.price,
            total=item_data.total,
        )
        session.add(item)

    await session.flush()

    result = await session.execute(
        select(PurchaseOrder)
        .options(
            selectinload(PurchaseOrder.items).selectinload(PurchaseOrderItem.product),
            selectinload(PurchaseOrder.invoice),
        )
        .where(PurchaseOrder.id == order.id)
    )
    order = result.scalar_one()
    return PurchaseOrderResponse.model_validate(order)


@router.put("/{order_id}", response_model=PurchaseOrderResponse)
async def update_purchase_order(
    order_id: UUID,
    data: PurchaseOrderUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Оновлює замовлення постачальнику."""
    result = await session.execute(
        select(PurchaseOrder)
        .options(
            selectinload(PurchaseOrder.items).selectinload(PurchaseOrderItem.product),
            selectinload(PurchaseOrder.invoice),
        )
        .where(PurchaseOrder.id == order_id)
    )
    order = result.scalar_one_or_none()
    if not order:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Замовлення з ID '{order_id}' не знайдено",
        )

    if order.status != PurchaseOrderStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна редагувати тільки чернетки",
        )

    update_data = data.model_dump(exclude_unset=True, exclude={"items"})
    for field, value in update_data.items():
        setattr(order, field, value)

    if data.items is not None:
        for old_item in order.items:
            await session.delete(old_item)
        for item_data in data.items:
            item = PurchaseOrderItem(
                purchase_order_id=order.id,
                product_id=item_data.product_id,
                quantity=item_data.quantity,
                price=item_data.price,
                total=item_data.total,
            )
            session.add(item)

    await session.flush()

    result = await session.execute(
        select(PurchaseOrder)
        .options(
            selectinload(PurchaseOrder.items).selectinload(PurchaseOrderItem.product),
            selectinload(PurchaseOrder.invoice),
        )
        .where(PurchaseOrder.id == order.id)
    )
    order = result.scalar_one()
    return PurchaseOrderResponse.model_validate(order)


@router.delete("/{order_id}", status_code=204)
async def delete_purchase_order(
    order_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Видаляє замовлення (тільки чернетку)."""
    result = await session.execute(
        select(PurchaseOrder).where(PurchaseOrder.id == order_id)
    )
    order = result.scalar_one_or_none()
    if not order:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Замовлення з ID '{order_id}' не знайдено",
        )
    if order.status != PurchaseOrderStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна видалити тільки чернетку",
        )
    await session.delete(order)
    await session.flush()


@router.post("/{order_id}/confirm", response_model=PurchaseOrderResponse)
async def confirm_purchase_order(
    order_id: UUID,
    data: PurchaseOrderConfirmRequest,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """
    Підтверджує або скасовує замовлення постачальнику.

    При підтвердженні (confirmed):
    - Створює прибуткову накладну (Invoice) зі статусом DRAFT
    - Копіює всі товари з замовлення в накладну
    - Зв'язує замовлення з новою накладною

    При скасуванні (cancelled):
    - Просто змінює статус на CANCELLED
    """
    result = await session.execute(
        select(PurchaseOrder)
        .options(
            selectinload(PurchaseOrder.items).selectinload(PurchaseOrderItem.product),
            selectinload(PurchaseOrder.invoice),
        )
        .where(PurchaseOrder.id == order_id)
    )
    order = result.scalar_one_or_none()

    if not order:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Замовлення з ID '{order_id}' не знайдено",
        )

    if order.status != PurchaseOrderStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Замовлення вже має статус '{order.status.value}'",
        )

    if data.status == PurchaseOrderStatus.CONFIRMED:
        # Створюємо прибуткову накладну на основі замовлення
        invoice_number = await generate_invoice_number(session)

        new_invoice = Invoice(
            number=invoice_number,
            supplier_id=order.supplier_id,
            invoice_date=order.order_date,
            payment_method=PaymentMethod.CREDIT,
            is_fiscal=order.is_fiscal,
            notes=f"Автоматично створено із замовлення №{order.number}",
            total_amount=order.total_amount,
            status=InvoiceStatus.DRAFT,
        )
        session.add(new_invoice)
        await session.flush()

        # Копіюємо позиції з замовлення в накладну
        for item in order.items:
            invoice_item = InvoiceItem(
                invoice_id=new_invoice.id,
                product_id=item.product_id,
                quantity=item.quantity,
                price=item.price,
                total=item.total,
            )
            session.add(invoice_item)

        # Зв'язуємо замовлення з накладною
        order.invoice_id = new_invoice.id

        # Змінюємо статус
        order.status = PurchaseOrderStatus.CONFIRMED

    elif data.status == PurchaseOrderStatus.CANCELLED:
        order.status = PurchaseOrderStatus.CANCELLED

    else:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Невірний статус. Використовуйте 'confirmed' або 'cancelled'",
        )

    await session.flush()

    # Перезавантажуємо зі зв'язками
    result = await session.execute(
        select(PurchaseOrder)
        .options(
            selectinload(PurchaseOrder.items).selectinload(PurchaseOrderItem.product),
            selectinload(PurchaseOrder.invoice),
        )
        .where(PurchaseOrder.id == order.id)
    )
    order = result.scalar_one()
    return PurchaseOrderResponse.model_validate(order)
