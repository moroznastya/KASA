"""
API роутер для отримання списку всіх документів (об'єднаний список).

Ендпоінти:
  - GET /documents — список всіх документів з пагінацією
"""

from datetime import datetime
from uuid import UUID

from fastapi import APIRouter, Depends, Query
from sqlalchemy import select, desc, func, union_all, literal_column
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.models.invoice import Invoice
from app.models.transfer import Transfer
from app.models.write_off import WriteOff
from app.models.return_invoice import ReturnInvoice
from app.models.receipt import Receipt
from app.schemas.invoice import InvoiceResponse
from app.schemas.transfer import TransferResponse
from app.schemas.write_off import WriteOffResponse
from app.schemas.return_invoice import ReturnInvoiceResponse
from app.schemas.receipt import ReceiptResponse
from app.services.auth_service import AuthService

router = APIRouter(
    prefix="/documents",
    tags=["Документи"],
)


@router.get("")
async def list_documents(
    page: int = Query(1, ge=1, description="Сторінка"),
    size: int = Query(20, ge=1, le=100, description="Елементів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Повертає об'єднаний список всіх документів:
    - Прибуткові накладні (Invoice)
    - Переміщення (Transfer)
    - Списання (WriteOff)
    - Повернення постачальнику (ReturnInvoice)
    - Чеки продажу (Receipt)

    Відсортовані за датою створення (від нових до старих).
    """
    offset = (page - 1) * size

    # Отримуємо всі документи окремо
    invoices_result = await session.execute(
        select(Invoice)
        .options(selectinload(Invoice.items))
        .order_by(desc(Invoice.created_at))
    )
    invoices = invoices_result.scalars().all()

    transfers_result = await session.execute(
        select(Transfer)
        .options(selectinload(Transfer.items))
        .order_by(desc(Transfer.created_at))
    )
    transfers = transfers_result.scalars().all()

    writeoffs_result = await session.execute(
        select(WriteOff)
        .options(selectinload(WriteOff.items))
        .order_by(desc(WriteOff.created_at))
    )
    writeoffs = writeoffs_result.scalars().all()

    returns_result = await session.execute(
        select(ReturnInvoice)
        .options(selectinload(ReturnInvoice.items))
        .order_by(desc(ReturnInvoice.created_at))
    )
    returns = returns_result.scalars().all()

    receipts_result = await session.execute(
        select(Receipt)
        .options(selectinload(Receipt.items))
        .order_by(desc(Receipt.created_at))
    )
    receipts = receipts_result.scalars().all()

    # Формуємо уніфікований список
    all_documents = []

    for inv in invoices:
        all_documents.append({
            "id": str(inv.id),
            "type": "invoice",
            "type_name": "Прибуткова накладна",
            "number": inv.invoice_number,
            "status": inv.status.value if hasattr(inv.status, 'value') else str(inv.status),
            "total_amount": float(inv.total_amount) if inv.total_amount else 0,
            "supplier_name": inv.supplier.name if inv.supplier else "",
            "created_at": inv.created_at.isoformat() if inv.created_at else None,
        })

    for tr in transfers:
        all_documents.append({
            "id": str(tr.id),
            "type": "transfer",
            "type_name": "Переміщення",
            "number": tr.transfer_number,
            "status": tr.status.value if hasattr(tr.status, 'value') else str(tr.status),
            "total_amount": 0,
            "supplier_name": "",
            "created_at": tr.created_at.isoformat() if tr.created_at else None,
        })

    for wo in writeoffs:
        all_documents.append({
            "id": str(wo.id),
            "type": "write_off",
            "type_name": "Списання",
            "number": wo.write_off_number,
            "status": wo.status.value if hasattr(wo.status, 'value') else str(wo.status),
            "total_amount": float(wo.total_amount) if wo.total_amount else 0,
            "supplier_name": "",
            "created_at": wo.created_at.isoformat() if wo.created_at else None,
        })

    for ri in returns:
        all_documents.append({
            "id": str(ri.id),
            "type": "return_invoice",
            "type_name": "Повернення постачальнику",
            "number": ri.return_number,
            "status": ri.status.value if hasattr(ri.status, 'value') else str(ri.status),
            "total_amount": float(ri.total_amount) if ri.total_amount else 0,
            "supplier_name": ri.supplier.name if ri.supplier else "",
            "created_at": ri.created_at.isoformat() if ri.created_at else None,
        })

    for rc in receipts:
        all_documents.append({
            "id": str(rc.id),
            "type": "receipt",
            "type_name": "Чек продажу",
            "number": rc.receipt_number,
            "status": "completed",
            "total_amount": float(rc.total_amount) if rc.total_amount else 0,
            "supplier_name": "",
            "created_at": rc.created_at.isoformat() if rc.created_at else None,
        })

    # Сортуємо за датою (від нових до старих)
    all_documents.sort(key=lambda d: d["created_at"] or "", reverse=True)

    total = len(all_documents)

    # Пагінація
    paginated = all_documents[offset:offset + size]

    return {
        "items": paginated,
        "total": total,
        "page": page,
        "size": size,
    }
