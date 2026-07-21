"""
API роутер для роботи з журналом взаєморозрахунків (SupplierLedger).

Ендпоінти:
  - GET    /ledger/{supplier_id}        — історія операцій з постачальником
  - GET    /ledger/{supplier_id}/balance — поточний баланс постачальника
  - POST   /ledger                      — створити запис (оплата, коригування)
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.schemas.ledger import (
    SupplierLedgerCreate,
    SupplierLedgerResponse,
    SupplierLedgerBalanceResponse,
)
from app.services.auth_service import AuthService
from app.services.ledger_service import LedgerService

router = APIRouter(
    prefix="/ledger",
    tags=["Взаєморозрахунки"],
)


@router.get("/{supplier_id}", response_model=dict)
async def get_supplier_ledger(
    supplier_id: UUID,
    page: int = Query(1, ge=1, description="Сторінка"),
    size: int = Query(20, ge=1, le=100, description="Елементів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує історію взаєморозрахунків з постачальником.

    Повертає всі операції (накладні, оплати, повернення) з пагінацією.
    """
    ledger_service = LedgerService(session)
    entries, total = await ledger_service.get_ledger_history(
        supplier_id=supplier_id,
        page=page,
        size=size,
    )
    return {
        "items": [SupplierLedgerResponse.model_validate(e) for e in entries],
        "total": total,
        "page": page,
        "size": size,
    }


@router.get("/{supplier_id}/balance", response_model=SupplierLedgerBalanceResponse)
async def get_supplier_balance(
    supplier_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Отримує поточний баланс постачальника.

    Додатне значення — борг перед постачальником.
    """
    ledger_service = LedgerService(session)
    balance, supplier_name, last_date = (
        await ledger_service.get_supplier_balance_with_name(supplier_id)
    )
    return SupplierLedgerBalanceResponse(
        supplier_id=supplier_id,
        supplier_name=supplier_name,
        current_balance=balance,
        last_updated=last_date,
    )


@router.post("", response_model=SupplierLedgerResponse, status_code=201)
async def create_ledger_entry(
    data: SupplierLedgerCreate,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.require_admin),
):
    """
    Створює новий запис у журналі взаєморозрахунків.

    Використовується для:
    - Фіксації оплати постачальнику (operation_type = payment)
    - Коригування боргу (operation_type = correction)
    - Інших операцій

    Для накладних та повернень записи створюються автоматично
    при підтвердженні документа.
    """
    ledger_service = LedgerService(session)
    entry = await ledger_service.create_ledger_entry(
        supplier_id=data.supplier_id,
        operation_type=data.operation_type.value,
        amount=data.amount,
        operation_date=data.operation_date,
        document_id=data.document_id,
        document_number=data.document_number,
        notes=data.notes,
    )
    return SupplierLedgerResponse.model_validate(entry)
