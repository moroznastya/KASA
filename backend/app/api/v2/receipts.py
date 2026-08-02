"""Receipt API v2 — використовує ReceiptUseCases."""

from __future__ import annotations

from datetime import datetime
from uuid import UUID

from fastapi import APIRouter, BackgroundTasks, Depends, HTTPException, Query, Request, status
from pydantic import BaseModel, Field

from app.application.use_cases import ReceiptUseCases
from app.application.use_cases.prro.prro_use_cases import PrroUseCases
from app.infrastructure.services.prro.qr_url import build_fiscal_check_url
from .deps import get_prro_use_cases, get_receipt_use_cases, get_cache_service
from app.domain.services.cache_service import ICacheService
from app.config import settings
from .cache_utils import cached, invalidate_receipt_cache, invalidate_product_cache

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
    cache: ICacheService = Depends(get_cache_service),
):
    """Отримати список чеків з пагінацією та кешуванням (TTL: 15s)."""

    @cached(
        cache,
        prefix="receipts:list",
        ttl=settings.CACHE_TTL_RECEIPTS,
    )
    async def _fetch(
        page: int, size: int,
        search: str | None,
        date_from: datetime | None,
        date_to: datetime | None,
        payment_method: str | None,
    ):
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

    return await _fetch(page, size, search, date_from, date_to, payment_method)


@router.get("/stats/today", response_model=ReceiptTodayStatsResponse)
async def get_today_stats(
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Статистика чеків за сьогодні з кешуванням (TTL: 10s)."""

    @cached(
        cache,
        prefix="receipts:stats",
        ttl=settings.CACHE_TTL_RECEIPT_STATS,
    )
    async def _fetch():
        return await use_cases.get_today_stats()

    return await _fetch()


@router.get("/search", response_model=ReceiptSearchResponse)
async def search_receipts(
    q: str = Query("", max_length=100, description="Пошук за номером чеку або назвою товару"),
    date_from: datetime | None = Query(None, description="Початкова дата"),
    date_to: datetime | None = Query(None, description="Кінцева дата"),
    receipt_type: str | None = Query("sale", description="Тип чеку: sale/return"),
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Пошук чеків для повернень з кешуванням (TTL: 60s)."""

    @cached(
        cache,
        prefix="receipts:search",
        ttl=settings.CACHE_TTL_RECEIPT_DETAIL,
    )
    async def _fetch(
        q: str,
        date_from: datetime | None,
        date_to: datetime | None,
        receipt_type: str | None,
        page: int,
        size: int,
    ):
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

    return await _fetch(q, date_from, date_to, receipt_type, page, size)


@router.get("/by-product/{query}/recent-sales", response_model=ProductRecentSalesListResponse)
async def get_recent_sales_by_product(
    query: str,
    limit: int = Query(5, ge=1, le=20, description="Кількість останніх продажів"),
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Останні продажі товару з кешуванням (TTL: 60s)."""

    @cached(
        cache,
        prefix="receipts:by-product",
        ttl=settings.CACHE_TTL_RECEIPT_DETAIL,
    )
    async def _fetch(query: str, limit: int):
        try:
            items = await use_cases.get_recent_sales_by_product(query, limit)
            return {"items": items, "total": len(items)}
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(query, limit)


@router.get("/products/{product_id}/returnable-quantity", response_model=ReturnableQuantityResponse)
async def get_returnable_quantity(
    product_id: UUID,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Скільки одиниць товару можна повернути з кешуванням (TTL: 30s)."""

    @cached(
        cache,
        prefix="receipts:returnable",
        ttl=settings.CACHE_TTL_RECEIPTS,
    )
    async def _fetch(product_id: UUID):
        try:
            return await use_cases.get_returnable_quantity(product_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(product_id)


@router.get("/{receipt_id}/items", response_model=list[ReceiptItemsResponse])
async def get_receipt_items(
    receipt_id: UUID,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Позиції чеку з кешуванням (TTL: 60s)."""

    @cached(
        cache,
        prefix="receipts:items",
        ttl=settings.CACHE_TTL_RECEIPT_DETAIL,
    )
    async def _fetch(receipt_id: UUID):
        try:
            return await use_cases.get_receipt_items(receipt_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(receipt_id)


@router.get("/{receipt_id}", response_model=ReceiptResponse)
async def get_receipt(
    receipt_id: UUID,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    prro: PrroUseCases = Depends(get_prro_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Отримати чек за ID з кешуванням (TTL: 60s)."""

    @cached(
        cache,
        prefix="receipts:detail",
        ttl=settings.CACHE_TTL_RECEIPT_DETAIL,
    )
    async def _fetch(receipt_id: UUID):
        try:
            receipt = await use_cases.get_receipt(receipt_id)
            prro_fn = await prro.get_prro_fn()
            receipt.fiscal_check_url = _fiscal_check_url(receipt, prro_fn)
            return receipt
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return await _fetch(receipt_id)


@router.post("/sale", response_model=ReceiptResponse, status_code=201)
async def create_sale_receipt(
    data: CreateReceiptRequest,
    request: Request,
    background_tasks: BackgroundTasks,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    cache: ICacheService = Depends(get_cache_service),
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
            cashier_id=_uuid_or_none(request.scope.get("user_id")),
            notes=data.notes,
        )
        result = await use_cases.create_sale_receipt(dto, background_tasks=background_tasks)
        await invalidate_receipt_cache(cache)
        await invalidate_product_cache(cache)
        return result
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/return", response_model=ReceiptResponse, status_code=201)
async def create_return_receipt(
    data: CreateReceiptRequest,
    request: Request,
    background_tasks: BackgroundTasks,
    use_cases: ReceiptUseCases = Depends(get_receipt_use_cases),
    cache: ICacheService = Depends(get_cache_service),
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
            cashier_id=_uuid_or_none(request.scope.get("user_id")),
            notes=data.notes,
        )
        result = await use_cases.create_return_receipt(dto, background_tasks=background_tasks)
        await invalidate_receipt_cache(cache)
        await invalidate_product_cache(cache)
        return result
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


def _uuid_or_none(value):
    """Конвертує значення з JWT scope (str/UUID/None) у UUID | None."""
    if value is None:
        return None
    if isinstance(value, UUID):
        return value
    try:
        return UUID(str(value))
    except (ValueError, AttributeError, TypeError):
        return None
