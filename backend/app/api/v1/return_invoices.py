"""
API роутер для роботи з поверненнями постачальнику (ReturnInvoices).

Ендпоінти:
  - GET    /return-invoices            — список повернень
  - GET    /return-invoices/{id}       — отримати повернення за ID
  - POST   /return-invoices            — створити повернення
  - PUT    /return-invoices/{id}       — оновити повернення
  - DELETE /return-invoices/{id}       — видалити повернення
  - POST   /return-invoices/{id}/confirm  — підтвердити повернення
  - POST   /return-invoices/{id}/cancel   — скасувати повернення
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select, desc
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.models.return_invoice import ReturnInvoice, ReturnInvoiceItem, ReturnInvoiceStatus
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


@router.get("", response_model=list[ReturnInvoiceResponse])
async def list_return_invoices(
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує список всіх повернень постачальнику."""
    result = await session.execute(
        select(ReturnInvoice)
        .options(selectinload(ReturnInvoice.items))
        .order_by(desc(ReturnInvoice.created_at))
    )
    invoices = result.scalars().all()
    return [ReturnInvoiceResponse.model_validate(inv) for inv in invoices]


@router.get("/{return_id}", response_model=ReturnInvoiceResponse)
async def get_return_invoice(
    return_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує повернення за ID."""
    result = await session.execute(
        select(ReturnInvoice)
        .options(selectinload(ReturnInvoice.items))
        .where(ReturnInvoice.id == return_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Повернення з ID '{return_id}' не знайдено",
        )
    return ReturnInvoiceResponse.model_validate(invoice)


@router.post("", response_model=ReturnInvoiceResponse, status_code=201)
async def create_return_invoice(
    data: ReturnInvoiceCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Створює нове повернення постачальнику."""
    invoice = ReturnInvoice(
        number=data.number,
        supplier_id=data.supplier_id,
        return_date=data.return_date,
        notes=data.notes,
        total_amount=data.total_amount,
        status=ReturnInvoiceStatus.DRAFT,
    )
    session.add(invoice)
    await session.flush()

    for item_data in data.items:
        item = ReturnInvoiceItem(
            return_invoice_id=invoice.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
            price=item_data.price,
            total=item_data.total,
        )
        session.add(item)

    await session.flush()

    result = await session.execute(
        select(ReturnInvoice)
        .options(selectinload(ReturnInvoice.items))
        .where(ReturnInvoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    return ReturnInvoiceResponse.model_validate(invoice)


@router.put("/{return_id}", response_model=ReturnInvoiceResponse)
async def update_return_invoice(
    return_id: UUID,
    data: ReturnInvoiceUpdate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Оновлює повернення постачальнику."""
    result = await session.execute(
        select(ReturnInvoice)
        .options(selectinload(ReturnInvoice.items))
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

    update_data = data.model_dump(exclude_unset=True, exclude={"items"})
    for field, value in update_data.items():
        setattr(invoice, field, value)

    if data.items is not None:
        for old_item in invoice.items:
            await session.delete(old_item)
        for item_data in data.items:
            item = ReturnInvoiceItem(
                return_invoice_id=invoice.id,
                product_id=item_data.product_id,
                quantity=item_data.quantity,
                price=item_data.price,
                total=item_data.total,
            )
            session.add(item)

    await session.flush()

    result = await session.execute(
        select(ReturnInvoice)
        .options(selectinload(ReturnInvoice.items))
        .where(ReturnInvoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    return ReturnInvoiceResponse.model_validate(invoice)


@router.delete("/{return_id}", status_code=204)
async def delete_return_invoice(
    return_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
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
    current_user = Depends(AuthService.get_current_user),
):
    """Підтверджує або скасовує повернення постачальнику."""
    doc_service = DocumentService(session)

    if data.status == ReturnInvoiceStatus.CONFIRMED:
        invoice = await doc_service.confirm_return_invoice(return_id)
    elif data.status == ReturnInvoiceStatus.CANCELLED:
        invoice = await doc_service.cancel_return_invoice(return_id)
    else:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Невірний статус. Використовуйте 'confirmed' або 'cancelled'",
        )

    result = await session.execute(
        select(ReturnInvoice)
        .options(selectinload(ReturnInvoice.items))
        .where(ReturnInvoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    return ReturnInvoiceResponse.model_validate(invoice)
