"""Receipt API v2 — використовує ReceiptUseCases."""

from __future__ import annotations

from datetime import datetime
from uuid import UUID

from fastapi import APIRouter, BackgroundTasks, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field

from app.application.use_cases import ReceiptUseCases
from app.application.use_cases.prro.prro_use_cases import PrroUseCases
from app.infrastructure.services.prro.qr_url import build_fiscal_check_url
from .deps import get_prro_use_cases, get_receipt_use_cases

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
    # ── Фіскалізація ────────────────────────────────────────────────────────
    is_fiscal: bool = False
    fiscal_status: str = "none"
    fiscal_number: str | None = None
    fiscal_serial: str | None = None
    fiscal_sent_at: datetime | None = None
    fiscal_error: str | None = None
    split_group_id: UUID | None = None
    fiscal_check_url: str | None = Field(
        default=None,
        description="URL перевірки фіскального чеку (для QR-коду)",
    )

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


class ReceiptTodayStatsResponse(BaseModel):
    total_sales: float
    total_returns: float
    total_profit: float
    total_vat: float
    receipts_count: int
    items_sold: int
    date: str


class ReceiptSearchItem(BaseModel):
    id: UUID
    receipt_number: str
    receipt_type: str
    total_amount: float
    created_at: datetime | None = None
    cashier_name: str = ""
    items_count: int = 0


class ReceiptSearchResponse(BaseModel):
    items: list[ReceiptSearchItem]
    total: int
    page: int
    page_size: int
    pages: int


class RecentSaleInfo(BaseModel):
    receipt_id: UUID
    receipt_number: str = ""
    created_at: datetime | None = None
    quantity: float
    price: float


class ProductBriefInfo(BaseModel):
    id: UUID
    title: str
    barcode: str | None = None
    price: float | None = None
    unit: str | None = None


class ProductRecentSalesItem(BaseModel):
    product: ProductBriefInfo
    total_sold: float
    total_returned: float
    returnable: float
    recent_sales: list[RecentSaleInfo] = []


class ProductRecentSalesListResponse(BaseModel):
    items: list[ProductRecentSalesItem]
    total: int


class ReturnableQuantityResponse(BaseModel):
    product_id: str
    total_sold: float
    total_returned: float
    returnable: float


class ReceiptItemsResponse(BaseModel):
    id: UUID
    product_id: UUID
    product_name: str = ""
    product_barcode: str | None = None
    quantity: float
    price: float
    total: float
    purchase_price: float | None = None
    created_at: datetime | None = None


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
    prro: PrroUseCases = Depends(get_prro_use_cases),
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
    prro_fn = await prro.get_prro_fn()
    for receipt in receipts:
        receipt.fiscal_check_url = _fiscal_check_url(receipt, prro_fn)
    return {
        "items": receipts,
        "total": total,
        "page": page,
        "size": size,
    }


@router.get("/stats/today", response_model=ReceiptTodayStatsResponse)
async def get_today_stats(
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Статистика чеків за сьогодні (продажі, повернення, прибуток, ПДВ)."""
    return await use_cases.get_today_stats()


@router.get("/search", response_model=ReceiptSearchResponse)
async def search_receipts(
    q: str = Query("", max_length=100, description="Пошук за номером чеку або назвою товару"),
    date_from: datetime | None = Query(None, description="Початкова дата"),
    date_to: datetime | None = Query(None, description="Кінцева дата"),
    receipt_type: str | None = Query("sale", description="Тип чеку: sale/return"),
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Пошук чеків для повернень (за номером чеку або назвою товару)."""
    try:
        items, total = await use_cases.search_receipts(
            q=q,
            date_from=date_from,
            date_to=date_to,
            receipt_type=receipt_type,
            page=page,
            size=size,
        )
        return {
            "items": items,
            "total": total,
            "page": page,
            "page_size": size,
            "pages": max(1, (total + size - 1) // size) if total > 0 else 1,
        }
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.get("/by-product/{query}/recent-sales", response_model=ProductRecentSalesListResponse)
async def get_recent_sales_by_product(
    query: str,
    limit: int = Query(5, ge=1, le=20, description="Кількість останніх продажів"),
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Останні продажі товару за штрих-кодом або назвою (для повернення без чеку)."""
    try:
        items = await use_cases.get_recent_sales_by_product(query, limit)
        return {"items": items, "total": len(items)}
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.get("/products/{product_id}/returnable-quantity", response_model=ReturnableQuantityResponse)
async def get_returnable_quantity(
    product_id: UUID,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Скільки одиниць товару можна повернути (продано - вже повернуто)."""
    try:
        return await use_cases.get_returnable_quantity(product_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.get("/{receipt_id}/items", response_model=list[ReceiptItemsResponse])
async def get_receipt_items(
    receipt_id: UUID,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Позиції чеку (для вибору товарів при поверненні)."""
    try:
        return await use_cases.get_receipt_items(receipt_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.get("/{receipt_id}", response_model=ReceiptResponse)
async def get_receipt(
    receipt_id: UUID,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    prro: PrroUseCases = Depends(get_prro_use_cases),
):
    """Отримати чек за ID."""
    try:
        receipt = await use_cases.get_receipt(receipt_id)
        prro_fn = await prro.get_prro_fn()
        receipt.fiscal_check_url = _fiscal_check_url(receipt, prro_fn)
        return receipt
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.post("/sale", response_model=ReceiptResponse, status_code=201)
async def create_sale_receipt(
    data: CreateReceiptRequest,
    background_tasks: BackgroundTasks,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Створити чек продажу (зменшує залишки товарів).

    Авто-фіскалізація ПРРО виконується у фоні (BackgroundTasks):
    HTTP-відповідь повертається одразу зі статусом "pending";
    фактичний статус доступний через GET /receipts/{receipt_id}.
    """
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
        return await use_cases.create_sale_receipt(dto, background_tasks=background_tasks)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/return", response_model=ReceiptResponse, status_code=201)
async def create_return_receipt(
    data: CreateReceiptRequest,
    background_tasks: BackgroundTasks,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
):
    """Створити чек повернення (збільшує залишки товарів).

    Авто-фіскалізація ПРРО виконується у фоні (BackgroundTasks):
    HTTP-відповідь повертається одразу зі статусом "pending";
    фактичний статус доступний через GET /receipts/{receipt_id}.
    """
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
        return await use_cases.create_return_receipt(dto, background_tasks=background_tasks)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


def _fiscal_check_url(receipt, prro_fn: str | None) -> str | None:
    """Формує URL перевірки фіскального чеку (QR) з даних ReceiptDTO."""
    if (
        not getattr(receipt, "fiscal_number", None)
        or not prro_fn
        or not getattr(receipt, "fiscal_sent_at", None)
    ):
        return None
    return build_fiscal_check_url(
        fiscal_number=receipt.fiscal_number,
        amount=receipt.total or 0,
        prro_fn=prro_fn,
        sent_at=receipt.fiscal_sent_at,
        mac=getattr(receipt, "fiscal_serial", None),
    )
