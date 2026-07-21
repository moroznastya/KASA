"""
API роутер для роботи з прибутковими накладними (Invoices).

Ендпоінти:
  - GET    /invoices            — список накладних
  - GET    /invoices/{id}       — отримати накладну за ID
  - POST   /invoices            — створити накладну
  - PUT    /invoices/{id}       — оновити накладну
  - DELETE /invoices/{id}       — видалити накладну
  - POST   /invoices/{id}/confirm  — підтвердити накладну
  - POST   /invoices/{id}/cancel   — скасувати накладну
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select, desc
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.models.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.schemas.invoice import (
    InvoiceCreate,
    InvoiceUpdate,
    InvoiceResponse,
    InvoiceItemResponse,
    InvoiceConfirmRequest,
)
from app.services.auth_service import AuthService
from app.services.document_service import DocumentService

router = APIRouter(
    prefix="/invoices",
    tags=["Прибуткові накладні"],
)


@router.get("", response_model=list[InvoiceResponse])
async def list_invoices(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує список всіх прибуткових накладних."""
    result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.items))
        .order_by(desc(Invoice.created_at))
    )
    invoices = result.scalars().all()
    return [InvoiceResponse.model_validate(inv) for inv in invoices]


@router.get("/{invoice_id}", response_model=InvoiceResponse)
async def get_invoice(
    invoice_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує прибуткову накладну за ID."""
    result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.items))
        .where(Invoice.id == invoice_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Накладну з ID '{invoice_id}' не знайдено",
        )
    return InvoiceResponse.model_validate(invoice)


@router.post("", response_model=InvoiceResponse, status_code=201)
async def create_invoice(
    data: InvoiceCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Створює нову прибуткову накладну."""
    # Виправлення: перетворюємо timezone-aware datetime в timezone-naive
    invoice_date = data.invoice_date.replace(tzinfo=None)

    # Розраховуємо загальну суму з позицій, якщо не передана
    total_amount = data.total_amount
    if total_amount is None and data.items:
        total_amount = sum(item.total for item in data.items)

    invoice = Invoice(
        number=data.number,
        supplier_id=data.supplier_id,
        invoice_date=invoice_date,
        payment_method=data.payment_method,
        is_fiscal=data.is_fiscal,
        notes=data.notes,
        total_amount=total_amount,
        status=InvoiceStatus.DRAFT,
    )
    session.add(invoice)
    await session.flush()

    # Додаємо позиції
    for item_data in data.items:
        item = InvoiceItem(
            invoice_id=invoice.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
            price=item_data.price,
            total=item_data.total,
        )
        session.add(item)

    await session.flush()

    # Повертаємо з позиціями
    result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.items))
        .where(Invoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    return InvoiceResponse.model_validate(invoice)


@router.put("/{invoice_id}", response_model=InvoiceResponse)
async def update_invoice(
    invoice_id: UUID,
    data: InvoiceUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Оновлює прибуткову накладну."""
    result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.items))
        .where(Invoice.id == invoice_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Накладну з ID '{invoice_id}' не знайдено",
        )

    if invoice.status != InvoiceStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна редагувати тільки чернетки",
        )

    update_data = data.model_dump(exclude_unset=True, exclude={"items"})
    for field, value in update_data.items():
        # Виправлення: перетворюємо timezone-aware datetime в timezone-naive
        if field == "invoice_date" and value is not None:
            value = value.replace(tzinfo=None)
        setattr(invoice, field, value)

    # Оновлюємо позиції, якщо передані
    if data.items is not None:
        # Видаляємо старі позиції
        for old_item in invoice.items:
            await session.delete(old_item)
        # Додаємо нові
        for item_data in data.items:
            item = InvoiceItem(
                invoice_id=invoice.id,
                product_id=item_data.product_id,
                quantity=item_data.quantity,
                price=item_data.price,
                total=item_data.total,
            )
            session.add(item)

        # Перераховуємо загальну суму
        invoice.total_amount = sum(item_data.total for item_data in data.items)

    await session.flush()

    # Повертаємо оновлену накладну
    result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.items))
        .where(Invoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    return InvoiceResponse.model_validate(invoice)


@router.delete("/{invoice_id}", status_code=204)
async def delete_invoice(
    invoice_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """Видаляє прибуткову накладну (тільки чернетку)."""
    result = await session.execute(
        select(Invoice).where(Invoice.id == invoice_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Накладну з ID '{invoice_id}' не знайдено",
        )
    if invoice.status != InvoiceStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Можна видалити тільки чернетку",
        )
    await session.delete(invoice)
    await session.flush()


@router.post("/{invoice_id}/confirm", response_model=InvoiceResponse)
async def confirm_invoice(
    invoice_id: UUID,
    data: InvoiceConfirmRequest,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """
    Підтверджує або скасовує прибуткову накладну.

    При підтвердженні:
    - Збільшує залишки товарів на складі
    - Створює запис у SupplierLedger

    При скасуванні:
    - Відкатує залишки товарів
    """
    doc_service = DocumentService(session)

    if data.status == InvoiceStatus.CONFIRMED:
        invoice = await doc_service.confirm_invoice(invoice_id)
    elif data.status == InvoiceStatus.CANCELLED:
        invoice = await doc_service.cancel_invoice(invoice_id)
    else:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Невірний статус. Використовуйте 'confirmed' або 'cancelled'",
        )

    # Повертаємо з позиціями
    result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.items))
        .where(Invoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    return InvoiceResponse.model_validate(invoice)
