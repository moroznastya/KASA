"""Ledger API v2 — використовує LedgerUseCases."""

from __future__ import annotations

from datetime import datetime
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field

from app.application.use_cases import LedgerUseCases
from .deps import get_ledger_use_cases

router = APIRouter(prefix="/ledger", tags=["ledger_v2"])


# ─── Pydantic схеми ──────────────────────────────────────────────────────────

class LedgerEntryResponse(BaseModel):
    id: UUID
    supplier_id: UUID
    amount: float
    operation_type: str = "invoice"
    balance_after: float | None = None
    created_at: datetime | None = None
    document_id: UUID | None = None
    document_number: str = ""
    notes: str = ""

    model_config = {"from_attributes": True}


class CreateLedgerEntryRequest(BaseModel):
    supplier_id: UUID
    amount: float = Field(..., description="Сума операції (додатна/від'ємна)")
    operation_type: str = "invoice"
    document_id: UUID | None = None
    document_number: str = ""
    notes: str = ""


class LedgerListResponse(BaseModel):
    items: list[LedgerEntryResponse]
    total: int
    page: int
    size: int


class SupplierBalanceResponse(BaseModel):
    supplier_id: UUID
    supplier_name: str | None = None
    balance: float
    last_operation_date: datetime | None = None


# ─── Ендпоінти ───────────────────────────────────────────────────────────────

@router.get("/entries", response_model=LedgerListResponse)
async def list_ledger_entries(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    supplier_id: UUID | None = None,
    operation_type: str | None = None,
    date_from: datetime | None = None,
    date_to: datetime | None = None,
    use_cases: LedgerUseCases = Depends(get_ledger_use_cases),
):
    """Отримати журнал взаєморозрахунків з пагінацією та фільтрацією."""
    entries, total = await use_cases.get_ledger_history(
        supplier_id=supplier_id,
        operation_type=operation_type,
        date_from=date_from,
        date_to=date_to,
        page=page,
        size=size,
    )
    return {
        "items": entries,
        "total": total,
        "page": page,
        "size": size,
    }


@router.post("/entries", response_model=LedgerEntryResponse, status_code=201)
async def create_ledger_entry(
    data: CreateLedgerEntryRequest,
    use_cases: LedgerUseCases = Depends(get_ledger_use_cases),
):
    """Створити новий запис у журналі взаєморозрахунків."""
    try:
        from app.application.dto.ledger_dto import LedgerCreateDTO
        dto = LedgerCreateDTO(
            supplier_id=data.supplier_id,
            amount=__import__('decimal').Decimal(str(data.amount)),
            operation_type=data.operation_type,
            document_id=data.document_id,
            document_number=data.document_number,
            notes=data.notes,
        )
        return await use_cases.create_entry(dto)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.get("/balance/{supplier_id}")
async def get_supplier_balance(
    supplier_id: UUID,
    use_cases: LedgerUseCases = Depends(get_ledger_use_cases),
):
    """Отримати поточний баланс постачальника."""
    try:
        balance = await use_cases.get_supplier_balance(supplier_id)
        return {
            "supplier_id": str(supplier_id),
            "balance": balance,
        }
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.get("/balances", response_model=list[SupplierBalanceResponse])
async def get_all_balances(
    use_cases: LedgerUseCases = Depends(get_ledger_use_cases),
):
    """Отримати баланси всіх постачальників."""
    balances = await use_cases.get_all_balances()
    return balances
