"""Invoice API v2 — використовує InvoiceUseCases."""

from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field

from app.application.use_cases import InvoiceUseCases
from app.config import settings
from app.domain.services.cache_service import ICacheService
from app.schemas.print import InvoicePrintRequest, InvoicePrintResponse

from .cache_utils import cached, invalidate_invoice_cache, invalidate_ledger_cache, invalidate_product_cache
from .deps import get_cache_service, get_invoice_print_use_cases, get_invoice_use_cases

if TYPE_CHECKING:
    from app.application.use_cases.invoice_print_use_cases import InvoicePrintUseCases

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


class UpdateInvoiceRequest(BaseModel):
    number: str | None = Field(None, min_length=1, max_length=50)
    supplier_id: UUID | None = None
    notes: str | None = None
    is_fiscal: bool | None = None
    invoice_date: datetime | None = None
    items: list[InvoiceItemRequest] | None = None


class InvoicePaymentInfoResponse(BaseModel):
    invoice_id: UUID
    invoice_number: str
    invoice_date: datetime | None = None
    total_amount: float
    paid_amount: float
    remaining: float


class PriceChangeItemResponse(BaseModel):
    product_id: UUID
    title: str
    barcode: str | None = None
    article: str = ""
    invoice_price: str
    current_price: str
    changed: bool
    difference: str


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
    cache: ICacheService = Depends(get_cache_service),
):
    """Отримати список накладних з пагінацією та кешуванням (TTL: 30s)."""

    @cached(
        cache,
        prefix="invoices:list",
        ttl=settings.CACHE_TTL_INVOICES,
    )
    async def _fetch(
        page: int, size: int,
        search: str | None,
        supplier_id: UUID | None,
        status: str | None,
        date_from: datetime | None,
        date_to: datetime | None,
    ):
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

    return await _fetch(page, size, search, supplier_id, status, date_from, date_to)


@router.get("/{invoice_id}", response_model=InvoiceResponse)
async def get_invoice(
    invoice_id: UUID,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Отримати накладну за ID з кешуванням (TTL: 30s)."""

    @cached(
        cache,
        prefix="invoices:detail",
        ttl=settings.CACHE_TTL_INVOICES,
    )
    async def _fetch(invoice_id: UUID):
        try:
            invoice = await use_cases.get_invoice(invoice_id)
            return invoice
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(invoice_id)


@router.post("", response_model=InvoiceResponse, status_code=201)
async def create_invoice(
    data: CreateInvoiceRequest,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Створити нову прибуткову накладну (інвалідує invoice-кеш)."""
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
        result = await use_cases.create_invoice(dto)
        await invalidate_invoice_cache(cache)
        return result
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/confirm", response_model=InvoiceResponse)
async def confirm_invoice(
    data: ConfirmInvoiceRequest,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Підтвердити прибуткову накладну (інвалідує invoices/products/ledger кеш)."""
    try:
        result = await use_cases.confirm_invoice(data)
        await invalidate_invoice_cache(cache)
        await invalidate_product_cache(cache)
        await invalidate_ledger_cache(cache)
        return result
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.put("/{invoice_id}", response_model=InvoiceResponse)
async def update_invoice(
    invoice_id: UUID,
    data: UpdateInvoiceRequest,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Оновити прибуткову накладну (тільки чернетку, інвалідує invoice-кеш)."""
    try:
        from app.application.dto.invoice_dto import InvoiceItemDTO, InvoiceUpdateDTO
        items = None
        if data.items is not None:
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
        dto = InvoiceUpdateDTO(
            number=data.number,
            supplier_id=data.supplier_id,
            notes=data.notes,
            is_fiscal=data.is_fiscal,
            invoice_date=data.invoice_date,
            items=items,
        )
        result = await use_cases.update_invoice(invoice_id, dto)
        await invalidate_invoice_cache(cache)
        return result
    except ValueError as e:
        detail = str(e)
        status_code = 404 if "не знайдено" in detail else 400
        raise HTTPException(status_code=status_code, detail=detail)


@router.delete("/{invoice_id}", status_code=204)
async def delete_invoice(
    invoice_id: UUID,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Видалити прибуткову накладну (тільки чернетку, інвалідує invoice-кеш)."""
    try:
        await use_cases.delete_invoice(invoice_id)
        await invalidate_invoice_cache(cache)
    except ValueError as e:
        detail = str(e)
        status_code = 404 if "не знайдено" in detail else 400
        raise HTTPException(status_code=status_code, detail=detail)


@router.get("/{invoice_id}/payment-info", response_model=InvoicePaymentInfoResponse)
async def get_invoice_payment_info(
    invoice_id: UUID,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Інформація про оплату накладної з кешуванням (TTL: 30s)."""

    @cached(
        cache,
        prefix="invoices:payment",
        ttl=settings.CACHE_TTL_INVOICES,
    )
    async def _fetch(invoice_id: UUID):
        try:
            return await use_cases.get_invoice_payment_info(invoice_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(invoice_id)


@router.get("/{invoice_id}/price-changes", response_model=list[PriceChangeItemResponse])
async def get_invoice_price_changes(
    invoice_id: UUID,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Зміни цін товарів у накладній з кешуванням (TTL: 60s)."""

    @cached(
        cache,
        prefix="invoices:price",
        ttl=settings.CACHE_TTL_INVOICE_PRICE,
    )
    async def _fetch(invoice_id: UUID):
        try:
            return await use_cases.get_invoice_price_changes(invoice_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(invoice_id)


@router.post("/{invoice_id}/print-items", response_model=InvoicePrintResponse)
async def render_invoice_print_items(
    invoice_id: UUID,
    data: InvoicePrintRequest,
    use_cases: InvoicePrintUseCases = Depends(get_invoice_print_use_cases),
):
    """Друк цінників/етикеток з прибуткової накладної (тільки підтвердженої)."""
    try:
        return await use_cases.render_invoice_print_items(
            invoice_id=invoice_id,
            print_type=data.print_type,
            only_changed=data.only_changed,
            template_id=data.template_id,
            width_mm=data.width_mm,
            height_mm=data.height_mm,
            gap_mm=data.gap_mm,
            margin_mm=data.margin_mm,
            barcode_type=data.barcode_type,
            barcode_height_mm=data.barcode_height_mm,
            print_mode=data.print_mode,
        )
    except ValueError as e:
        detail = str(e)
        status_code = 404 if "не знайдено" in detail else 400
        raise HTTPException(status_code=status_code, detail=detail)


@router.post("/{invoice_id}/cancel", response_model=InvoiceResponse)
async def cancel_invoice(
    invoice_id: UUID,
    use_cases: InvoiceUseCases = Depends(get_invoice_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Скасувати прибуткову накладну (інвалідує invoices/products/ledger кеш)."""
    try:
        result = await use_cases.cancel_invoice(invoice_id)
        await invalidate_invoice_cache(cache)
        await invalidate_product_cache(cache)
        await invalidate_ledger_cache(cache)
        return result
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
