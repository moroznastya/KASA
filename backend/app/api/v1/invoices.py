"""
API роутер для роботи з прибутковими накладними (Invoices).

Ендпоінти:
  - GET    /invoices            — список накладних (з пагінацією)
  - GET    /invoices/{id}       — отримати накладну за ID
  - POST   /invoices            — створити накладну
  - PUT    /invoices/{id}       — оновити накладну
  - DELETE /invoices/{id}       — видалити накладну
  - POST   /invoices/{id}/confirm  — підтвердити накладну
  - POST   /invoices/{id}/cancel   — скасувати накладну
"""

from uuid import UUID
from decimal import Decimal

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, desc, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.domain.services.document_service import generate_invoice_number
from app.infrastructure.persistence.models.supplier import Supplier
from app.schemas.invoice import (
    InvoiceCreate,
    InvoiceUpdate,
    InvoiceResponse,
    InvoiceItemResponse,
    InvoiceConfirmRequest,
    InvoicePaymentInfo,
)
from app.domain.services.auth_service import AuthService
from app.domain.services.document_service import DocumentService
from app.infrastructure.persistence.models.supplier_ledger import SupplierLedger, LedgerOperationType

router = APIRouter(
    prefix="/invoices",
    tags=["Прибуткові накладні"],
)


@router.get("/", response_model=dict)
async def list_invoices(
    supplier_id: UUID = Query(None, description="Фільтр за постачальником (повертає тільки підтверджені накладні для оплати)"),
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує список прибуткових накладних з пагінацією.

    Якщо передано `supplier_id`, повертає тільки підтверджені накладні
    для цього постачальника (для вибору при оплаті).

    Повертає:
    - items: список накладних
    - total: загальна кількість
    - page: поточна сторінка
    - page_size: розмір сторінки
    - pages: загальна кількість сторінок
    """
    # Базовий запит для підрахунку
    count_query = select(func.count(Invoice.id))
    query = select(Invoice).options(
        selectinload(Invoice.items).selectinload(InvoiceItem.product),
        selectinload(Invoice.supplier),
    )

    if supplier_id:
        query = query.where(Invoice.supplier_id == supplier_id)
        query = query.where(Invoice.status == InvoiceStatus.CONFIRMED)
        count_query = count_query.where(Invoice.supplier_id == supplier_id)
        count_query = count_query.where(Invoice.status == InvoiceStatus.CONFIRMED)

    # Загальна кількість
    count_result = await session.execute(count_query)
    total = count_result.scalar() or 0

    # Пагінація
    offset = (page - 1) * size
    query = query.order_by(desc(Invoice.created_at)).offset(offset).limit(size)

    result = await session.execute(query)
    invoices = result.scalars().all()

    pages = max(1, (total + size - 1) // size) if total > 0 else 1

    response_list = []
    for inv in invoices:
        result_item = InvoiceResponse.model_validate(inv)
        result_item.supplier_name = inv.supplier.name if inv.supplier else None
        response_list.append(result_item)

    return {
        "items": [r.model_dump() for r in response_list],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": pages,
    }


@router.get("/{invoice_id}", response_model=InvoiceResponse)
async def get_invoice(
    invoice_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """Отримує прибуткову накладну за ID."""
    result = await session.execute(
        select(Invoice)
        .options(
            selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(Invoice.supplier),
        )
        .where(Invoice.id == invoice_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Накладну з ID '{invoice_id}' не знайдено",
        )
    result_item = InvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item


@router.post("/", response_model=InvoiceResponse, status_code=201)
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

    # Автоматична генерація номера, якщо не вказано
    number = data.number
    if not number:
        number = await generate_invoice_number(session)

    invoice = Invoice(
        number=number,
        supplier_id=data.supplier_id,
        invoice_date=invoice_date,
        payment_method=data.payment_method,
        is_fiscal=data.is_fiscal,
        notes=data.notes,
        total_amount=total_amount,
        status=InvoiceStatus.DRAFT,
        created_by_id=current_user.id,
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
            cost_price=item_data.cost_price or item_data.price,
            markup_percent=item_data.markup_percent or 0,
        )
        session.add(item)

    await session.flush()

    # Повертаємо з позиціями
    result = await session.execute(
        select(Invoice)
        .options(
            selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(Invoice.supplier),
        )
        .where(Invoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    result_item = InvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item


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
        .options(
            selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(Invoice.supplier),
        )
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
                cost_price=item_data.cost_price or item_data.price,
                markup_percent=item_data.markup_percent or 0,
            )
            session.add(item)

        # Перераховуємо загальну суму
        invoice.total_amount = sum(item_data.total for item_data in data.items)

    await session.flush()

    # Повертаємо оновлену накладну
    result = await session.execute(
        select(Invoice)
        .options(
            selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(Invoice.supplier),
        )
        .where(Invoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    result_item = InvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item


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




@router.get("/{invoice_id}/payment-info", response_model=InvoicePaymentInfo)
async def get_invoice_payment_info(
    invoice_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Повертає інформацію про оплату накладної:
    - total_amount: загальна сума накладної
    - paid_amount: скільки вже сплачено/компенсовано (сума payment + return записів 
                   у ledger з цим document_id)
    - remaining: залишок до сплати

    Враховано:
    - PAYMENT запити (оплати) з цим document_id
    - RETURN запити (повернення постачальнику) з цим document_id,
      які зменшують суму боргу за накладну
    """
    # Отримуємо накладну
    result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.supplier))
        .where(Invoice.id == invoice_id)
    )
    invoice = result.scalar_one_or_none()
    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Накладну з ID '{invoice_id}' не знайдено",
        )

    # Розраховуємо сплачену суму:
    # 1. PAYMENT записи з цим document_id (оплати, що зменшують борг)
    # 2. RETURN записи з цим document_id (повернення, що зменшують борг)
    ledger_result = await session.execute(
        select(SupplierLedger.amount)
        .where(SupplierLedger.document_id == invoice_id)
        .where(
            SupplierLedger.operation_type.in_([
                LedgerOperationType.PAYMENT,
                LedgerOperationType.RETURN,
            ])
        )
    )
    ledger_rows = ledger_result.all()
    
    # PAYMENT та RETURN записи мають від'ємну суму (зменшують борг),
    # тому беремо абсолютне значення
    paid_amount = sum(abs(Decimal(str(row[0]))) for row in ledger_rows)
    paid_amount = Decimal(str(paid_amount)).quantize(Decimal("0.01"))

    total_amount = Decimal(str(invoice.total_amount or 0)).quantize(Decimal("0.01"))
    remaining = (total_amount - paid_amount).quantize(Decimal("0.01"))

    return InvoicePaymentInfo(
        invoice_id=invoice.id,
        invoice_number=invoice.number,
        invoice_date=invoice.invoice_date,
        total_amount=total_amount,
        paid_amount=paid_amount,
        remaining=remaining,
    )

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
        .options(
            selectinload(Invoice.items).selectinload(InvoiceItem.product),
            selectinload(Invoice.supplier),
        )
        .where(Invoice.id == invoice.id)
    )
    invoice = result.scalar_one()
    result_item = InvoiceResponse.model_validate(invoice)
    result_item.supplier_name = invoice.supplier.name if invoice.supplier else None
    return result_item
