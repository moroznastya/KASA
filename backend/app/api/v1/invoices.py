"""
API роутер для роботи з прибутковими накладними (Invoices).

Ендпоінти:
  - GET    /invoices                     — список накладних (з пагінацією)
  - GET    /invoices/{id}                — отримати накладну за ID
  - POST   /invoices                     — створити накладну
  - PUT    /invoices/{id}                — оновити накладну
  - DELETE /invoices/{id}                — видалити накладну
  - GET    /invoices/{id}/payment-info   — інформація про оплату
  - POST   /invoices/{id}/confirm        — підтвердити накладну
  - POST   /invoices/{id}/cancel         — скасувати накладну
  - POST   /invoices/{id}/print-items    — друк цінників/етикеток з накладної
  - GET    /invoices/{id}/price-changes  — зміна цін в накладній

⚠️ DEPRECATED: цей v1-роутер залишено для зворотної сумісності — використовуйте /api/v2/invoices/*.
"""

import logging
import math
from datetime import UTC, datetime
from decimal import Decimal
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import desc, func, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.api.v1.print import _get_fields_from_settings
from app.database import get_session
from app.domain.services.auth_service import AuthService
from app.domain.services.document_service import DocumentService, generate_invoice_number
from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.infrastructure.persistence.models.print_template import PrintTemplate
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.supplier_ledger import LedgerOperationType, SupplierLedger
from app.infrastructure.services.price_tag_print_service import PriceTagPrintService
from app.schemas.invoice import (
    InvoiceConfirmRequest,
    InvoiceCreate,
    InvoicePaymentInfo,
    InvoiceResponse,
    InvoiceUpdate,
)
from app.schemas.print import (
    InvoicePrintRequest,
    InvoicePrintResponse,
    PriceChangeItem,
)

logger = logging.getLogger(__name__)

router = APIRouter(
    prefix="/invoices",
    tags=["Прибуткові накладні"],
)


@router.get("", response_model=dict, deprecated=True)
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


@router.get("/{invoice_id}", response_model=InvoiceResponse, deprecated=True)
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


@router.post("", response_model=InvoiceResponse, status_code=201, deprecated=True)
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
        # Отримуємо поточну ціну товару для previous_price
        prod_result = await session.execute(
            select(Product.price).where(Product.id == item_data.product_id)
        )
        current_product_price = prod_result.scalar_one_or_none()

        item = InvoiceItem(
            invoice_id=invoice.id,
            product_id=item_data.product_id,
            quantity=item_data.quantity,
            price=item_data.price,
            total=item_data.total,
            cost_price=item_data.cost_price or item_data.price,
            markup_percent=item_data.markup_percent or 0,
            previous_price=current_product_price,
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


@router.put("/{invoice_id}", response_model=InvoiceResponse, deprecated=True)
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
            # Отримуємо поточну ціну товару для previous_price
            prod_result = await session.execute(
                select(Product.price).where(Product.id == item_data.product_id)
            )
            current_product_price = prod_result.scalar_one_or_none()

            item = InvoiceItem(
                invoice_id=invoice.id,
                product_id=item_data.product_id,
                quantity=item_data.quantity,
                price=item_data.price,
                total=item_data.total,
                cost_price=item_data.cost_price or item_data.price,
                markup_percent=item_data.markup_percent or 0,
                previous_price=current_product_price,
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


@router.delete("/{invoice_id}", status_code=204, deprecated=True)
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


@router.get("/{invoice_id}/payment-info", response_model=InvoicePaymentInfo, deprecated=True)
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


@router.post("/{invoice_id}/confirm", response_model=InvoiceResponse, deprecated=True)
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


# ─── ЕНДПОІНТ: Друк цінників/етикеток з накладної ────────────────────────────


@router.post("/{invoice_id}/print-items", response_model=InvoicePrintResponse, deprecated=True)
async def render_invoice_print_items(
    invoice_id: UUID,
    data: InvoicePrintRequest,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Рендерить цінники або етикетки для товарів з прибуткової накладної.

    Тіло запиту:
    ```json
    {
      "print_type": "price_tag" | "label",
      "only_changed": false,
      "template_id": "uuid",
      "width_mm": 40,
      "height_mm": 25,
      "gap_mm": 3,
      "margin_mm": 10,
      "barcode_type": "code128",
      "barcode_height_mm": 12
    }
    ```

    Логіка:
    1. Завантажує накладну з товарами (тільки підтверджені)
    2. Для кожного товару порівнює ціну в накладній з поточною роздрібною ціною
    3. Якщо only_changed=True — фільтрує тільки товари зі змінною ціною
    4. Отримує шаблон друку за template_id
    5. Викликає PriceTagPrintService для генерації HTML
    6. Повертає HTML + мета-інформацію + статистику змін цін
    """
    # ─── 1. Завантажуємо накладну з товарами ──────────────────────────────
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

    # Друкувати можна тільки з підтверджених накладних
    if invoice.status != InvoiceStatus.CONFIRMED:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Друк цінників/етикеток можливий тільки для підтверджених накладних. "
                   f"Поточний статус: {invoice.status.value}",
        )

    if not invoice.items:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Накладна не містить товарів",
        )

    # ─── 2. Отримуємо шаблон з БД ──────────────────────────────────────────
    tmpl_result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == data.template_id)
    )
    template = tmpl_result.scalar_one_or_none()

    if not template or not template.is_active:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{data.template_id}' не знайдено або він неактивний",
        )

    # ─── 3. Отримуємо налаштування полів ────────────────────────────────────
    fields_key = "price_tag_fields" if data.print_type == "price_tag" else "label_fields"
    fields = await _get_fields_from_settings(
        session,
        fields_key,
        ["title", "price", "barcode"],
    )

    # ─── 4. Формуємо список товарів для друку та рахуємо зміни цін ──────────
    products_dicts: list[dict] = []
    price_changes: list[dict] = []
    changed_count = 0
    now_str = datetime.now(UTC).strftime("%d.%m.%Y")

    for item in invoice.items:
        product = item.product
        if not product:
            logger.warning("Товар з ID '%s' не знайдено в накладній '%s'", item.product_id, invoice_id)
            continue

        # Ціна в накладній
        invoice_price = item.price or 0
        # Ціна товару до накладної (previous_price) або поточна роздрібна
        prev_price = item.previous_price or product.price or 0
        # Поточна роздрібна ціна товару (для друку)
        current_price = product.price or 0

        # Порівнюємо ціну в накладній з ціною до накладної (для визначення змін)
        prev_price_dec = Decimal(str(prev_price)).quantize(Decimal("0.01"))
        invoice_price_dec = Decimal(str(invoice_price)).quantize(Decimal("0.01"))
        current_price_dec = Decimal(str(current_price)).quantize(Decimal("0.01"))
        difference = (prev_price_dec - invoice_price_dec).quantize(Decimal("0.01"))
        changed = difference != Decimal("0.00")

        # Статистика змін цін
        price_changes.append({
            "product_id": str(product.id),
            "title": product.title,
            "barcode": product.barcode or "",
            "article": product.sku or "",
            "invoice_price": str(invoice_price_dec),
            "current_price": str(current_price_dec),
            "changed": changed,
            "difference": str(difference),
        })

        if changed:
            changed_count += 1

        # Друкована ціна: беремо поточну роздрібну ціну (те, що на ціннику)
        print_price = str(current_price_dec)

        # Додаємо товар у список для друку
        products_dicts.append({
            "id": str(product.id),
            "title": product.title,
            "price": print_price,
            "barcode": product.barcode or "",
            "article": product.sku or "",
            "category": "",
            "copies": 1,
            "created_date": now_str,
        })

    if not products_dicts:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Не знайдено товарів для друку",
        )

    # ─── 5. Фільтруємо тільки зі змінною ціною (якщо потрібно) ─────────────
    if data.only_changed:
        filtered_products = []
        for i, p in enumerate(products_dicts):
            if price_changes[i]["changed"]:
                filtered_products.append(p)
        products_dicts = filtered_products

        if not products_dicts:
            return InvoicePrintResponse(
                html="",
                total_labels=0,
                total_pages=0,
                changed_count=0,
                total_count=len(invoice.items),
            )

    # ─── 6. Формуємо налаштування для сервісу друку ─────────────────────────
    total_items = len(invoice.items)

    settings = {
        "width_mm": data.width_mm,
        "height_mm": data.height_mm,
        "gap_mm": data.gap_mm,
        "fields": fields,
        "barcode_type": data.barcode_type,
        "barcode_height_mm": data.barcode_height_mm,
        "print_mode": data.print_mode,
    }

    if data.print_type == "price_tag":
        settings["margin_mm"] = data.margin_mm
        settings["page_width_mm"] = 210   # A4
        settings["page_height_mm"] = 297  # A4

    # ─── 7. Рендеримо HTML ──────────────────────────────────────────────────
    if data.print_type == "price_tag":
        html = PriceTagPrintService.render_price_tags_grid(
            template.content,
            products_dicts,
            settings,
        )
        # Обчислюємо кількість сторінок
        _cols, _rows, per_page = PriceTagPrintService._calc_grid(
            data.width_mm,
            data.height_mm,
            data.gap_mm,
            210,  # A4 ширина
            297,  # A4 висота
            data.margin_mm,
        )
        total_labels = len(products_dicts)
        total_pages = max(1, math.ceil(total_labels / per_page)) if per_page > 0 else 1
    else:
        html = PriceTagPrintService.render_labels_sequential(
            template.content,
            products_dicts,
            settings,
        )
        total_labels = len(products_dicts)
        total_pages = None

    logger.info(
        "Згенеровано друк для накладної '%s': %s, %d товарів, %d змін цін",
        invoice.number, data.print_type, total_labels, changed_count,
    )

    return InvoicePrintResponse(
        html=html,
        total_labels=total_labels,
        total_pages=total_pages,
        changed_count=changed_count,
        total_count=total_items,
    )


# ─── ЕНДПОІНТ: Інформація про зміну цін в накладній ─────────────────────────


@router.get("/{invoice_id}/price-changes", response_model=list[PriceChangeItem], deprecated=True)
async def get_invoice_price_changes(
    invoice_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    """
    Повертає список товарів з накладної з інформацією про зміну цін.

    Для кожного товару показує:
    - product_id — ID товару
    - title — назва товару
    - barcode — штрих-код
    - article — артикул
    - invoice_price — ціна в накладній
    - current_price — поточна роздрібна ціна
    - changed — чи змінилась ціна
    - difference — різниця між цінами

    Повертає JSON масив:
    ```json
    [
      {
        "product_id": "uuid",
        "title": "Хліб білий",
        "barcode": "4820012345678",
        "article": "ХЛ-001",
        "invoice_price": "25.00",
        "current_price": "28.00",
        "changed": true,
        "difference": "3.00"
      }
    ]
    ```
    """
    # ─── 1. Завантажуємо накладну з товарами ──────────────────────────────
    result = await session.execute(
        select(Invoice)
        .options(
            selectinload(Invoice.items).selectinload(InvoiceItem.product),
        )
        .where(Invoice.id == invoice_id)
    )
    invoice = result.scalar_one_or_none()

    if not invoice:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Накладну з ID '{invoice_id}' не знайдено",
        )

    if not invoice.items:
        return []

    # ─── 2. Формуємо список змін цін ──────────────────────────────────────
    changes: list[PriceChangeItem] = []

    for item in invoice.items:
        product = item.product
        if not product:
            continue

        invoice_price = item.price or 0
        # Використовуємо previous_price для порівняння (ціна ДО накладної)
        prev_price = item.previous_price or product.price or 0
        current_price = product.price or 0

        prev_price_dec = Decimal(str(prev_price)).quantize(Decimal("0.01"))
        invoice_price_dec = Decimal(str(invoice_price)).quantize(Decimal("0.01"))
        current_price_dec = Decimal(str(current_price)).quantize(Decimal("0.01"))
        difference = (prev_price_dec - invoice_price_dec).quantize(Decimal("0.01"))
        changed = difference != Decimal("0.00")

        changes.append(PriceChangeItem(
            product_id=product.id,
            title=product.title,
            barcode=product.barcode,
            article=product.sku,
            invoice_price=str(invoice_price_dec),
            current_price=str(current_price_dec),
            changed=changed,
            difference=str(difference),
        ))

    return changes
