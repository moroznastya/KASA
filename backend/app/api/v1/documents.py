"""
API роутер для отримання списку всіх документів (об'єднаний список).

Ендпоінти:
  - GET /documents — список всіх документів з пагінацією та фільтрацією
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from fastapi import APIRouter, Depends, Query
from sqlalchemy import select, desc, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.database import get_session
from app.models.invoice import Invoice
from app.models.transfer import Transfer
from app.models.write_off import WriteOff, WriteOffItem
from app.models.return_invoice import ReturnInvoice
from app.services.auth_service import AuthService

router = APIRouter(
    prefix="/documents",
    tags=["Документи"],
)


@router.get("")
async def list_documents(
    page: int = Query(1, ge=1, description="Сторінка"),
    size: int = Query(20, ge=1, le=100, description="Елементів на сторінці"),
    status: Optional[str] = Query(None, description="Фільтр за статусом (draft, confirmed, cancelled)"),
    document_type: Optional[str] = Query(None, description="Фільтр за типом (invoice, transfer, write_off, return_invoice)"),
    search: Optional[str] = Query(None, description="Пошук за номером документа"),
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Повертає об'єднаний список всіх документів (БЕЗ чеків продажу):
    - Прибуткові накладні (Invoice)
    - Переміщення (Transfer)
    - Списання (WriteOff)
    - Повернення постачальнику (ReturnInvoice)

    Відсортовані за датою створення (від нових до старих).
    Підтримує фільтрацію за типом, статусом та пошук за номером.
    """
    offset = (page - 1) * size
    all_documents = []

    # ─── Прибуткові накладні ────────────────────────────────────────────────
    if not document_type or document_type == 'invoice':
        invoice_query = (
            select(Invoice)
            .options(
                selectinload(Invoice.items),
                selectinload(Invoice.supplier),
            )
            .order_by(desc(Invoice.created_at))
        )
        invoices_result = await session.execute(invoice_query)
        invoices = invoices_result.scalars().all()

        for inv in invoices:
            inv_status = inv.status.value if hasattr(inv.status, 'value') else str(inv.status)
            if status and inv_status != status:
                continue
            if search and search.lower() not in (inv.number or '').lower():
                continue

            supplier_name = inv.supplier.name if inv.supplier else ""
            all_documents.append({
                "id": str(inv.id),
                "document_type": "invoice",
                "document_number": inv.number,
                "status": inv_status,
                "total_amount": float(inv.total_amount) if inv.total_amount else 0,
                "supplier_name": supplier_name,
                "supplier_id": str(inv.supplier_id) if inv.supplier_id else None,
                "created_at": inv.created_at.isoformat() if inv.created_at else None,
            })

    # ─── Переміщення ─────────────────────────────────────────────────────────
    if not document_type or document_type == 'transfer':
        transfers_result = await session.execute(
            select(Transfer)
            .options(selectinload(Transfer.items))
            .order_by(desc(Transfer.created_at))
        )
        transfers = transfers_result.scalars().all()

        for tr in transfers:
            tr_status = tr.status.value if hasattr(tr.status, 'value') else str(tr.status)
            if status and tr_status != status:
                continue
            if search and search.lower() not in (tr.number or '').lower():
                continue

            all_documents.append({
                "id": str(tr.id),
                "document_type": "transfer",
                "document_number": tr.number,
                "status": tr_status,
                "total_amount": 0,
                "supplier_name": "",
                "supplier_id": None,
                "created_at": tr.created_at.isoformat() if tr.created_at else None,
            })

    # ─── Списання ────────────────────────────────────────────────────────────
    if not document_type or document_type == 'write_off':
        writeoffs_result = await session.execute(
            select(WriteOff)
            .options(
                selectinload(WriteOff.items).selectinload(WriteOffItem.product),
            )
            .order_by(desc(WriteOff.created_at))
        )
        writeoffs = writeoffs_result.scalars().all()

        for wo in writeoffs:
            if status and status != "confirmed":
                continue
            if search and search.lower() not in (wo.number or '').lower():
                continue

            total = 0.0
            if wo.items:
                for item in wo.items:
                    price = float(item.product.price) if item.product and item.product.price else 0
                    qty = float(item.quantity) if item.quantity else 0
                    total += price * qty

            all_documents.append({
                "id": str(wo.id),
                "document_type": "write_off",
                "document_number": wo.number,
                "status": "confirmed",
                "total_amount": float(wo.total_amount) if wo.total_amount else total,
                "supplier_name": "",
                "supplier_id": None,
                "created_at": wo.created_at.isoformat() if wo.created_at else None,
            })

    # ─── Повернення постачальнику ────────────────────────────────────────────
    if not document_type or document_type == 'return_invoice':
        returns_result = await session.execute(
            select(ReturnInvoice)
            .options(
                selectinload(ReturnInvoice.items),
                selectinload(ReturnInvoice.supplier),
            )
            .order_by(desc(ReturnInvoice.created_at))
        )
        returns = returns_result.scalars().all()

        for ri in returns:
            ri_status = ri.status.value if hasattr(ri.status, 'value') else str(ri.status)
            if status and ri_status != status:
                continue
            if search and search.lower() not in (ri.number or '').lower():
                continue

            supplier_name = ri.supplier.name if ri.supplier else ""
            all_documents.append({
                "id": str(ri.id),
                "document_type": "return_invoice",
                "document_number": ri.number,
                "status": ri_status,
                "total_amount": float(ri.total_amount) if ri.total_amount else 0,
                "supplier_name": supplier_name,
                "supplier_id": str(ri.supplier_id) if ri.supplier_id else None,
                "created_at": ri.created_at.isoformat() if ri.created_at else None,
            })

    # Сортуємо за датою (від нових до старих)
    all_documents.sort(key=lambda d: d["created_at"] or "", reverse=True)

    total = len(all_documents)
    paginated = all_documents[offset:offset + size]

    return {
        "items": paginated,
        "total": total,
        "page": page,
        "size": size,
        "pages": max(1, (total + size - 1) // size),
    }
