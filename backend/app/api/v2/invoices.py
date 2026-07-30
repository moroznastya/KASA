"""Invoice API v2 — використовує InvoiceUseCases."""

from __future__ import annotations

from datetime import datetime
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field

from app.application.use_cases import InvoiceUseCases
from .deps import get_invoice_use_cases

router = APIRouter(prefix="/invoices", tags=["invoices_v2"])


# ─── Pydantic схеми ──────────────────────────────────────────────────────────

class InvoiceItemResponse(BaseModel):
    product_id: UUID
    quantity: float
    price: float
    tax_rate: int = 20
    name: str = ""

    model_config = {"from_attributes": True}


class InvoiceResponse(BaseModel):
    id: UUID
    number: str
    supplier_id: UUID
    items: list[InvoiceItemResponse] = []
    total: float | None = None
    status: str = "draft"
    created_at: datetime | None = None
    confirmed_at: datetime | None = None
    notes: str = ""

    model_config = {"from_attributes": True}


class InvoiceItemRequest(BaseModel):
    product_id: UUID
    quantity: float = Field(..., gt=0)
    price: float = Field(..., gt=0)
    tax_rate: int = 20
    name: str = ""


class CreateInvoiceRequest(BaseModel):
    number: str = Field(..., min_length=1, max_length=50)
    supplier_id: UUID
    items: list[InvoiceItemRequest] = Field(..., min_length=1)
    notes: str = ""


class ConfirmInvoiceRequest(BaseModel):
    invoice_id: UUID


class InvoiceListResponse(BaseModel):
    items: list[InvoiceResponse]
    total: int
    page: int
    size: int


# ─── Ендпоінти ───────────────────────────────────────────────────────────────

@router.get("", response_model=InvoiceListResponse)
async def list_invoices(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    search: str | None = None,
    supplier_id: UUID | None = None,
    status: str | None = None,
    date_from: datetime | None = None,
    date_to: datetime | None = None,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
):
    """Отримати список накладних з пагінацією та фільтрацією."""
    invoices, total = await use_cases.get_invoices(
        query=search,
        supplier_id=supplier_id,
        status=status,
        date_from=date_from,
        date_to=date_to,
        page=page,
        size=size,
    )
    return {
        "items": invoices,
        "total": total,
        "page": page,
        "size": size,
    }


@router.get("/{invoice_id}", response_model=InvoiceResponse)
async def get_invoice(
    invoice_id: UUID,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
):
    """Отримати накладну за ID."""
    try:
        invoice = await use_cases.get_invoice(invoice_id)
        return invoice
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.post("", response_model=InvoiceResponse, status_code=201)
async def create_invoice(
    data: CreateInvoiceRequest,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
):
    """Створити нову прибуткову накладну."""
    try:
        from app.application.dto.invoice_dto import InvoiceCreateDTO, InvoiceItemDTO
        items = [
            InvoiceItemDTO(
                product_id=item.product_id,
                quantity=__import__('decimal').Decimal(str(item.quantity)),
                price=__import__('decimal').Decimal(str(item.price)),
                tax_rate=item.tax_rate,
                name=item.name,
            )
            for item in data.items
        ]
        dto = InvoiceCreateDTO(
            number=data.number,
            supplier_id=data.supplier_id,
            items=items,
            notes=data.notes,
        )
        return await use_cases.create_invoice(dto)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/confirm", response_model=InvoiceResponse)
async def confirm_invoice(
    data: ConfirmInvoiceRequest,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
):
    """Підтвердити прибуткову накладну (оновлює залишки та баланс)."""
    try:
        return await use_cases.confirm_invoice(data)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/{invoice_id}/cancel", response_model=InvoiceResponse)
async def cancel_invoice(
    invoice_id: UUID,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
):
    """Скасувати прибуткову накладну (відкочує залишки та баланс)."""
    try:
        return await use_cases.cancel_invoice(invoice_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
