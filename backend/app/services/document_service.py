"""
Сервіс для роботи з документами (накладні, переміщення, списання, повернення).

Забезпечує:
  - Підтвердження документів (зміна статусу)
  - Оновлення залишків товарів при підтвердженні
  - Відміну підтвердження (повернення залишків)
"""

from decimal import Decimal
from typing import Union
from uuid import UUID

from fastapi import HTTPException, status
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.models.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.models.transfer import Transfer, TransferItem, TransferStatus
from app.models.write_off import WriteOff, WriteOffItem
from app.models.return_invoice import ReturnInvoice, ReturnInvoiceItem, ReturnInvoiceStatus
from app.models.product import Product
from app.services.product_service import ProductService
from app.services.ledger_service import LedgerService


# Тип документа для узагальненої роботи
DocumentType = Union[Invoice, Transfer, WriteOff, ReturnInvoice]


class DocumentService:
    """
    Сервіс для управління документами.

    Відповідає за:
    - Підтвердження документів (зміна статусу з DRAFT на CONFIRMED)
    - Оновлення залишків товарів при підтвердженні
    - Скасування документів (зміна статусу на CANCELLED)
    - Відкат залишків при скасуванні
    """

    def __init__(self, session: AsyncSession):
        """Ініціалізація сервісу з асинхронною сесією БД."""
        self.session = session
        self.product_service = ProductService(session)
        self.ledger_service = LedgerService(session)

    # ─── Прибуткова накладна (Invoice) ───────────────────────────────────────

    async def confirm_invoice(self, invoice_id: UUID) -> Invoice:
        """
        Підтверджує прибуткову накладну.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Збільшує залишки товарів на складі
        3. Створює запис у SupplierLedger (збільшення боргу)

        Args:
            invoice_id: UUID накладної.

        Returns:
            Оновлений об'єкт Invoice.
        """
        # Отримуємо накладну з позиціями (обов'язково selectinload для async)
        result = await self.session.execute(
            select(Invoice)
            .options(selectinload(Invoice.items))
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
                detail=f"Накладна вже має статус '{invoice.status.value}'",
            )

        # Оновлюємо залишки товарів
        for item in invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=item.quantity,
            )

        # Створюємо запис у SupplierLedger
        if invoice.total_amount and invoice.total_amount > 0:
            await self.ledger_service.create_ledger_entry(
                supplier_id=invoice.supplier_id,
                operation_type="invoice",
                document_id=invoice.id,
                document_number=invoice.number,
                amount=invoice.total_amount,
                operation_date=invoice.invoice_date,
                notes=f"Прибуткова накладна №{invoice.number}",
            )

        # Змінюємо статус
        invoice.status = InvoiceStatus.CONFIRMED
        await self.session.flush()
        return invoice

    async def cancel_invoice(self, invoice_id: UUID) -> Invoice:
        """
        Скасовує прибуткову накладну.

        При скасуванні:
        1. Змінює статус на CANCELLED
        2. Зменшує залишки товарів (відкат)

        Args:
            invoice_id: UUID накладної.
        """
        result = await self.session.execute(
            select(Invoice)
            .options(selectinload(Invoice.items))
            .where(Invoice.id == invoice_id)
        )
        invoice = result.scalar_one_or_none()

        if not invoice:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Накладну з ID '{invoice_id}' не знайдено",
            )

        if invoice.status != InvoiceStatus.CONFIRMED:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Скасувати можна лише підтверджену накладну",
            )

        # Відкатуємо залишки
        for item in invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=-item.quantity,
            )

        invoice.status = InvoiceStatus.CANCELLED
        await self.session.flush()
        return invoice

    # ─── Переміщення (Transfer) ──────────────────────────────────────────────

    async def confirm_transfer(self, transfer_id: UUID) -> Transfer:
        """
        Підтверджує переміщення товару.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Зменшує залишок на складі відправника
        3. Збільшує залишок на складі отримувача

        Args:
            transfer_id: UUID переміщення.
        """
        result = await self.session.execute(
            select(Transfer)
            .options(selectinload(Transfer.items))
            .where(Transfer.id == transfer_id)
        )
        transfer = result.scalar_one_or_none()

        if not transfer:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Переміщення з ID '{transfer_id}' не знайдено",
            )

        if transfer.status != TransferStatus.DRAFT:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Переміщення вже має статус '{transfer.status.value}'",
            )

        # Оновлюємо залишки: зменшуємо на from_location, збільшуємо на to_location
        for item in transfer.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=-item.quantity,
            )

        transfer.status = TransferStatus.CONFIRMED
        await self.session.flush()
        return transfer

    async def cancel_transfer(self, transfer_id: UUID) -> Transfer:
        """
        Скасовує переміщення.

        Args:
            transfer_id: UUID переміщення.
        """
        result = await self.session.execute(
            select(Transfer)
            .options(selectinload(Transfer.items))
            .where(Transfer.id == transfer_id)
        )
        transfer = result.scalar_one_or_none()

        if not transfer:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Переміщення з ID '{transfer_id}' не знайдено",
            )

        if transfer.status != TransferStatus.CONFIRMED:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Скасувати можна лише підтверджене переміщення",
            )

        # Відкатуємо залишки
        for item in transfer.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=item.quantity,
            )

        transfer.status = TransferStatus.CANCELLED
        await self.session.flush()
        return transfer

    # ─── Списання (WriteOff) ─────────────────────────────────────────────────

    async def confirm_write_off(self, write_off_id: UUID) -> WriteOff:
        """
        Підтверджує списання товару.

        При підтвердженні зменшує залишки товарів на складі.

        Args:
            write_off_id: UUID списання.
        """
        result = await self.session.execute(
            select(WriteOff)
            .options(selectinload(WriteOff.items))
            .where(WriteOff.id == write_off_id)
        )
        write_off = result.scalar_one_or_none()

        if not write_off:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Списання з ID '{write_off_id}' не знайдено",
            )

        for item in write_off.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=-item.quantity,
            )

        await self.session.flush()
        return write_off

    # ─── Повернення постачальнику (ReturnInvoice) ────────────────────────────

    async def confirm_return_invoice(self, return_id: UUID) -> ReturnInvoice:
        """
        Підтверджує повернення товару постачальнику.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Зменшує залишки товарів на складі
        3. Створює запис у SupplierLedger (зменшення боргу)

        Args:
            return_id: UUID повернення.
        """
        result = await self.session.execute(
            select(ReturnInvoice)
            .options(selectinload(ReturnInvoice.items))
            .where(ReturnInvoice.id == return_id)
        )
        return_invoice = result.scalar_one_or_none()

        if not return_invoice:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Повернення з ID '{return_id}' не знайдено",
            )

        if return_invoice.status != ReturnInvoiceStatus.DRAFT:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Повернення вже має статус '{return_invoice.status.value}'",
            )

        # Зменшуємо залишки
        for item in return_invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=-item.quantity,
            )

        # Створюємо запис у SupplierLedger (зменшення боргу)
        if return_invoice.total_amount and return_invoice.total_amount > 0:
            await self.ledger_service.create_ledger_entry(
                supplier_id=return_invoice.supplier_id,
                operation_type="return",
                document_id=return_invoice.id,
                document_number=return_invoice.number,
                amount=-return_invoice.total_amount,
                operation_date=return_invoice.return_date,
                notes=f"Повернення постачальнику №{return_invoice.number}",
            )

        return_invoice.status = ReturnInvoiceStatus.CONFIRMED
        await self.session.flush()
        return return_invoice

    async def cancel_return_invoice(self, return_id: UUID) -> ReturnInvoice:
        """
        Скасовує повернення постачальнику.

        Args:
            return_id: UUID повернення.
        """
        result = await self.session.execute(
            select(ReturnInvoice)
            .options(selectinload(ReturnInvoice.items))
            .where(ReturnInvoice.id == return_id)
        )
        return_invoice = result.scalar_one_or_none()

        if not return_invoice:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Повернення з ID '{return_id}' не знайдено",
            )

        if return_invoice.status != ReturnInvoiceStatus.CONFIRMED:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Скасувати можна лише підтверджене повернення",
            )

        # Відкатуємо залишки
        for item in return_invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=item.quantity,
            )

        return_invoice.status = ReturnInvoiceStatus.CANCELLED
        await self.session.flush()
        return return_invoice
