"""Receipt API v2 — використовує ReceiptUseCases."""

from __future__ import annotations

from datetime import datetime
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field

from app.application.use_cases import ReceiptUseCases
from .deps import get_receipt_use_cases

router = APIRouter(prefix="/receipts", tags=["receipts_v2"])


# ─── Pydantic схеми ──────────────────────────────────────────────────────────

class ReceiptItemResponse(BaseModel):
    product_id: UUID
    name: str
    quantity: float
    price: float
    tax_rate: int = 20

    model_config = {"from_attributes": True}


class ReceiptResponse(BaseModel):
    id: UUID
    number: str
    items: list[ReceiptItemResponse] = []
    total: float | None = None
    payment_method: str = "cash"
    created_at: datetime | None = None
    cash_amount: float | None = None
    card_amount: float | None = None
    change_amount: float | None = None
    customer_id: UUID | None = None
    notes: str = ""

    model_config = {"from_attributes": True}


class ReceiptItemRequest(BaseModel):
    product_id: UUID
    name: str = ""
    quantity: float = Field(..., gt=0)
    price: float = Field(..., gt=0)
    tax_rate: int = 20


class CreateReceiptRequest(BaseModel):
    items: list[ReceiptItemRequest] = Field(..., min_length=1)
    payment_method: str = "cash"
    cash_amount: float | None = None
    card_amount: float | None = None
    customer_id: UUID | None = None
    notes: str = ""


class ReceiptListResponse(BaseModel):
    items: list[ReceiptResponse]
    total: int
    page: int
    size: int


# ─── Ендпоінти ───────────────────────────────────────────────────────────────

@router.get("", response_model=ReceiptListResponse)
async def list_receipts(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    search: str | None = None,
    date_from: datetime | None = None,
    date_to: datetime | None = None,
    payment_method: str | None = None,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Отримати список чеків з пагінацією та фільтрацією."""
    receipts, total = await use_cases.get_receipts(
        query=search,
        date_from=date_from,
        date_to=date_to,
        payment_method=payment_method,
        page=page,
        size=size,
    )
    return {
        "items": receipts,
        "total": total,
        "page": page,
        "size": size,
    }


@router.get("/{receipt_id}", response_model=ReceiptResponse)
async def get_receipt(
    receipt_id: UUID,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Отримати чек за ID."""
    try:
        receipt = await use_cases.get_receipt(receipt_id)
        return receipt
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.post("/sale", response_model=ReceiptResponse, status_code=201)
async def create_sale_receipt(
    data: CreateReceiptRequest,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Створити чек продажу (зменшує залишки товарів)."""
    try:
        from app.application.dto.receipt_dto import ReceiptCreateDTO, ReceiptItemDTO
        items = [
            ReceiptItemDTO(
                product_id=item.product_id,
                name=item.name,
                quantity=__import__('decimal').Decimal(str(item.quantity)),
                price=__import__('decimal').Decimal(str(item.price)),
                tax_rate=item.tax_rate,
            )
            for item in data.items
        ]
        dto = ReceiptCreateDTO(
            items=items,
            payment_method=data.payment_method,
            cash_amount=__import__('decimal').Decimal(str(data.cash_amount)) if data.cash_amount else None,
            card_amount=__import__('decimal').Decimal(str(data.card_amount)) if data.card_amount else None,
            customer_id=data.customer_id,
            notes=data.notes,
        )
        return await use_cases.create_sale_receipt(dto)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/return", response_model=ReceiptResponse, status_code=201)
async def create_return_receipt(
    data: CreateReceiptRequest,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Створити чек повернення (збільшує залишки товарів)."""
    try:
        from app.application.dto.receipt_dto import ReceiptCreateDTO, ReceiptItemDTO
        items = [
            ReceiptItemDTO(
                product_id=item.product_id,
                name=item.name,
                quantity=__import__('decimal').Decimal(str(item.quantity)),
                price=__import__('decimal').Decimal(str(item.price)),
                tax_rate=item.tax_rate,
            )
            for item in data.items
        ]
        dto = ReceiptCreateDTO(
            items=items,
            payment_method=data.payment_method,
            cash_amount=__import__('decimal').Decimal(str(data.cash_amount)) if data.cash_amount else None,
            card_amount=__import__('decimal').Decimal(str(data.card_amount)) if data.card_amount else None,
            customer_id=data.customer_id,
            notes=data.notes,
        )
        return await use_cases.create_return_receipt(dto)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
