"""
Сервіс для отримання товарів постачальника, їх залишків та руху.

Збирає дані з усіх типів документів, що належать постачальнику:
  - Прибуткові накладні (Invoice) — прихід
  - Повернення постачальнику (ReturnInvoice) — витрата
  - Переміщення (Transfer) — витрата
  - Списання (WriteOff) — витрата
  - Чеки (Receipt) — продаж (витрата)

Товари визначаються як ті, що:
  1. Мають supplier_id = постачальник, АБО
  2. Зустрічаються в документах цього постачальника
"""

from decimal import Decimal
from typing import Optional
from uuid import UUID

from sqlalchemy import select, union
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import joinedload

from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem
from app.infrastructure.persistence.models.return_invoice import ReturnInvoice, ReturnInvoiceItem, ReturnInvoiceStatus
from app.infrastructure.persistence.models.supplier import Supplier
from app.infrastructure.persistence.models.transfer import Transfer, TransferItem, TransferStatus
from app.infrastructure.persistence.models.write_off import WriteOff, WriteOffItem
from app.schemas.supplier_products import (
    SupplierProductItem,
    SupplierProductMovement,
    SupplierProductMovementsResponse,
    SupplierProductsResponse,
)


class SupplierProductService:
    """Сервіс для роботи з товарами постачальника."""

    def __init__(self, session: AsyncSession):
        self.session = session

    async def get_supplier_products(
        self,
        supplier_id: UUID,
        search: Optional[str] = None,
    ) -> SupplierProductsResponse:
        """Отримує всі товари, пов'язані з постачальником через документи або direct supplier_id."""
        # Отримуємо постачальника
        result = await self.session.execute(
            select(Supplier).where(Supplier.id == supplier_id)
        )
        supplier = result.scalar_one_or_none()
        if not supplier:
            raise ValueError(f"Постачальника з ID '{supplier_id}' не знайдено")

        # Збираємо ID товарів, які є в документах цього постачальника
        # 1. Товари з прибуткових накладних
        invoice_products = (
            select(InvoiceItem.product_id)
            .join(Invoice, InvoiceItem.invoice_id == Invoice.id)
            .where(Invoice.supplier_id == supplier_id)
            .where(Invoice.status == InvoiceStatus.CONFIRMED)
        )
        # 2. Товари з повернень
        return_products = (
            select(ReturnInvoiceItem.product_id)
            .join(ReturnInvoice, ReturnInvoiceItem.return_invoice_id == ReturnInvoice.id)
            .where(ReturnInvoice.supplier_id == supplier_id)
            .where(ReturnInvoice.status == ReturnInvoiceStatus.CONFIRMED)
        )
        # 3. Товари, які мають supplier_id = постачальник
        direct_products = select(Product.id).where(Product.supplier_id == supplier_id)

        # Об'єднуємо всі ID
        union_query = union(invoice_products, return_products, direct_products)
        result = await self.session.execute(union_query)
        product_ids = set(row[0] for row in result)

        if not product_ids:
            return SupplierProductsResponse(
                supplier_id=supplier.id,
                supplier_name=supplier.name,
                total_products=0,
                total_stock_value=Decimal("0.00"),
                products=[],
            )

        # Отримуємо товари
        query = select(Product).options(joinedload(Product.category)).where(Product.id.in_(product_ids))

        if search:
            search_pattern = f"%{search}%"
            query = query.where(
                Product.title.ilike(search_pattern) |
                Product.barcode.ilike(search_pattern) |
                Product.sku.ilike(search_pattern)
            )

        query = query.order_by(Product.title)
        result = await self.session.execute(query)
        products = result.scalars().all()

        # Рахуємо загальну вартість залишків
        total_stock_value = Decimal("0.00")
        product_items = []
        for p in products:
            stock = Decimal(str(p.stock or 0))
            cost_price = Decimal(str(p.cost_price or 0))
            total_stock_value += stock * cost_price

            product_items.append(SupplierProductItem(
                id=p.id,
                barcode=p.barcode,
                sku=p.sku,
                title=p.title,
                price=Decimal(str(p.price or 0)),
                cost_price=Decimal(str(p.cost_price or 0)),
                stock=stock,
                unit=p.unit,
                category_name=p.category.name if p.category else None,
            ))

        return SupplierProductsResponse(
            supplier_id=supplier.id,
            supplier_name=supplier.name,
            total_products=len(product_items),
            total_stock_value=total_stock_value,
            products=product_items,
        )

    async def get_product_movements(
        self,
        supplier_id: UUID,
        product_id: UUID,
        limit: int = 100,
    ) -> SupplierProductMovementsResponse:
        """Отримує рух конкретного товару по документах постачальника."""
        # Перевіряємо постачальника
        result = await self.session.execute(
            select(Supplier).where(Supplier.id == supplier_id)
        )
        supplier = result.scalar_one_or_none()
        if not supplier:
            raise ValueError(f"Постачальника з ID '{supplier_id}' не знайдено")

        # Отримуємо товар (він може не мати supplier_id, але бути в документах)
        result = await self.session.execute(
            select(Product).options(joinedload(Product.category)).where(Product.id == product_id)
        )
        product = result.scalar_one_or_none()
        if not product:
            raise ValueError(f"Товар з ID '{product_id}' не знайдено")

        movements = []

        # 1. Прибуткові накладні (прихід) — тільки від цього постачальника
        result = await self.session.execute(
            select(InvoiceItem, Invoice)
            .join(Invoice, InvoiceItem.invoice_id == Invoice.id)
            .where(
                InvoiceItem.product_id == product_id,
                Invoice.supplier_id == supplier_id,
                Invoice.status == InvoiceStatus.CONFIRMED,
            )
            .order_by(Invoice.invoice_date.desc())
            .limit(limit)
        )
        for item, invoice in result:
            movements.append(SupplierProductMovement(
                id=item.id,
                date=invoice.invoice_date,
                document_type="invoice",
                document_number=invoice.number,
                document_id=invoice.id,
                quantity=Decimal(str(item.quantity)),
                price=Decimal(str(item.price)),
                total=Decimal(str(item.total)),
                notes=f"Прибуткова накладна: {invoice.number}",
            ))

        # 2. Повернення постачальнику (витрата) — тільки цьому постачальнику
        result = await self.session.execute(
            select(ReturnInvoiceItem, ReturnInvoice)
            .join(ReturnInvoice, ReturnInvoiceItem.return_invoice_id == ReturnInvoice.id)
            .where(
                ReturnInvoiceItem.product_id == product_id,
                ReturnInvoice.supplier_id == supplier_id,
                ReturnInvoice.status == ReturnInvoiceStatus.CONFIRMED,
            )
            .order_by(ReturnInvoice.return_date.desc())
            .limit(limit)
        )
        for item, ret_inv in result:
            movements.append(SupplierProductMovement(
                id=item.id,
                date=ret_inv.return_date,
                document_type="return_invoice",
                document_number=ret_inv.number,
                document_id=ret_inv.id,
                quantity=Decimal(str(-item.quantity)),
                price=Decimal(str(item.price)),
                total=Decimal(str(-item.total)),
                notes=f"Повернення постачальнику: {ret_inv.number}",
            ))

        # 3. Чеки (продаж — витрата)
        result = await self.session.execute(
            select(ReceiptItem, Receipt)
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(
                ReceiptItem.product_id == product_id,
                not Receipt.is_return,
            )
            .order_by(Receipt.created_at.desc())
            .limit(limit)
        )
        for item, receipt in result:
            movements.append(SupplierProductMovement(
                id=item.id,
                date=receipt.created_at,
                document_type="receipt",
                document_number=receipt.receipt_number,
                document_id=receipt.id,
                quantity=Decimal(str(-item.quantity)),
                price=Decimal(str(item.price)),
                total=Decimal(str(-item.total)),
                notes=f"Чек: {receipt.receipt_number}",
            ))

        # 4. Списання (витрата)
        result = await self.session.execute(
            select(WriteOffItem, WriteOff)
            .join(WriteOff, WriteOffItem.write_off_id == WriteOff.id)
            .where(
                WriteOffItem.product_id == product_id,
            )
            .order_by(WriteOff.created_at.desc())
            .limit(limit)
        )
        for item, wo in result:
            movements.append(SupplierProductMovement(
                id=item.id,
                date=wo.created_at,
                document_type="write_off",
                document_number=wo.number,
                document_id=wo.id,
                quantity=Decimal(str(-item.quantity)),
                price=Decimal(str(item.price or 0)),
                total=Decimal(str(-(item.quantity * (item.price or 0)))),
                notes=f"Списання: {wo.number}",
            ))

        # 5. Переміщення (витрата зі складу)
        result = await self.session.execute(
            select(TransferItem, Transfer)
            .join(Transfer, TransferItem.transfer_id == Transfer.id)
            .where(
                TransferItem.product_id == product_id,
                Transfer.status == TransferStatus.CONFIRMED,
            )
            .order_by(Transfer.created_at.desc())
            .limit(limit)
        )
        for item, tr in result:
            movements.append(SupplierProductMovement(
                id=item.id,
                date=tr.created_at,
                document_type="transfer",
                document_number=tr.number,
                document_id=tr.id,
                quantity=Decimal(str(-item.quantity)),
                price=Decimal(str(item.price or 0)),
                total=Decimal(str(-(item.quantity * (item.price or 0)))),
                notes=f"Переміщення: {tr.number}",
            ))

        # Сортуємо всі рухи за датою (від найновіших)
        movements.sort(key=lambda m: m.date, reverse=True)

        product_item = SupplierProductItem(
            id=product.id,
            barcode=product.barcode,
            sku=product.sku,
            title=product.title,
            price=Decimal(str(product.price or 0)),
            cost_price=Decimal(str(product.cost_price or 0)),
            stock=Decimal(str(product.stock or 0)),
            unit=product.unit,
            category_name=product.category.name if product.category else None,
        )

        return SupplierProductMovementsResponse(
            product=product_item,
            movements=movements[:limit],
            total_movements=len(movements),
        )
