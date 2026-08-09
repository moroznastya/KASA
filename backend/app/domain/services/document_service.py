"""
Сервіс для роботи з документами (накладні, переміщення, списання, повернення, інвентаризація).

Забезпечує:
  - Підтвердження документів (зміна статусу)
  - Оновлення залишків товарів при підтвердженні
  - Оновлення собівартості товарів при підтвердженні накладної
  - Відміну підтвердження (повернення залишків)
  - Автоматичне створення прибуткової накладної при обміні товару
  - Оновлення залишків при підтвердженні інвентаризації
"""

from datetime import datetime
from decimal import Decimal
from typing import Union
from uuid import UUID

from fastapi import HTTPException, status
from sqlalchemy import select, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.infrastructure.persistence.models.invoice import Invoice, InvoiceItem, InvoiceStatus, PaymentMethod
from app.infrastructure.persistence.models.transfer import Transfer, TransferItem, TransferStatus
from app.infrastructure.persistence.models.write_off import WriteOff, WriteOffItem
from app.infrastructure.persistence.models.return_invoice import ReturnInvoice, ReturnInvoiceItem, ReturnInvoiceStatus, ReturnActionType
from app.infrastructure.persistence.models.purchase_order import PurchaseOrder
from app.infrastructure.persistence.models.inventory import Inventory, InventoryItem, InventoryStatus
from app.infrastructure.persistence.models.product import Product
from app.domain.services.product_service import ProductService
from app.domain.services.ledger_service import LedgerService
from app.infrastructure.persistence.models.supplier_ledger import (
    LedgerOperationType,
    SupplierLedger,
)


# Тип документа для узагальненої роботи
DocumentType = Union[Invoice, Transfer, WriteOff, ReturnInvoice, PurchaseOrder, Inventory]


# Мапа для відображення способу оплати в текст
PAYMENT_METHOD_LABELS: dict[PaymentMethod, str] = {
    PaymentMethod.CREDIT: "в борг",
    PaymentMethod.BANK_TRANSFER: "по перерахунку",
    PaymentMethod.CASH: "готівкою з каси",
    PaymentMethod.OTHER: "інший спосіб",
}

# Мапа для відображення типу дії повернення в текст
RETURN_ACTION_LABELS: dict[ReturnActionType, str] = {
    ReturnActionType.DEDUCT_FROM_DEBT: "списано з боргу постачальника",
    ReturnActionType.ADD_TO_CASH: "зачислено в касу",
    ReturnActionType.EXCHANGE: "обмін на інший товар",
}


async def generate_invoice_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для прибуткової накладної.
    Формат: ПН-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    from datetime import datetime
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"ПН-{today}-"

    result = await session.execute(
        select(func.max(Invoice.number))
        .where(Invoice.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()

    if max_number:
        last_seq = int(max_number[-3:])
        new_seq = last_seq + 1
    else:
        new_seq = 1

    return f"{prefix}{new_seq:03d}"


async def generate_inventory_number(session: AsyncSession) -> str:
    """
    Генерує автоматичний номер для інвентаризації.
    Формат: ІН-{YYYYMMDD}-{XXX}, де XXX — порядковий номер за день.
    """
    from datetime import datetime
    today = datetime.utcnow().strftime("%Y%m%d")
    prefix = f"ІН-{today}-"

    result = await session.execute(
        select(func.max(Inventory.number))
        .where(Inventory.number.like(f"{prefix}%"))
    )
    max_number = result.scalar()

    if max_number:
        last_seq = int(max_number[-3:])
        new_seq = last_seq + 1
    else:
        new_seq = 1

    return f"{prefix}{new_seq:03d}"


class DocumentService:
    """
    Сервіс для управління документами.

    Відповідає за:
    - Підтвердження документів (зміна статусу з DRAFT на CONFIRMED)
    - Оновлення залишків товарів при підтвердженні
    - Оновлення собівартості товарів при підтвердженні накладної
    - Скасування документів (зміна статусу на CANCELLED)
    - Відкат залишків при скасуванні
    - Автоматичне створення прибуткової накладної при обміні
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
        3. Оновлює собівартість товарів (середньозважену або останню ціну закупівлі)
        4. Якщо обрано спосіб оплати "в борг" (credit) — створює запис
           у SupplierLedger (збільшення боргу перед постачальником)

        Args:
            invoice_id: UUID накладної.

        Returns:
            Оновлений об'єкт Invoice.
        """
        # Отримуємо накладну з позиціями (обов'язково selectinload для async)
        result = await self.session.execute(
            select(Invoice)
            .options(selectinload(Invoice.items).selectinload(InvoiceItem.product))
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

        # Оновлюємо залишки та собівартість товарів
        for item in invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=item.quantity,
            )
            # Фіскальна накладна: товар надходить у фіскальний залишок
            if invoice.is_fiscal:
                await self._increase_fiscal_stock(item.product_id, item.quantity)
            # Оновлюємо собівартість товару (середньозважену або останню ціну закупівлі)
            if item.cost_price and item.cost_price > 0:
                result = await self.session.execute(
                    select(Product).where(Product.id == item.product_id)
                )
                product = result.scalar_one_or_none()
                if product:
                    product.cost_price = item.cost_price
                    # Оновлюємо роздрібну ціну товару згідно з ціною в накладній
                    # Зберігаємо попередню ціну, якщо ще не збережена
                    if item.previous_price is None:
                        item.previous_price = product.price
                    product.price = item.price
                    await self.session.flush()

        # Змінюємо статус
        invoice.status = InvoiceStatus.CONFIRMED
        await self.session.flush()

        # Створюємо INVOICE-запис у журналі взаєморозрахунків (борг +) ЗАВЖДИ,
        # у тій самій сесії/транзакції
        invoice_amount = (
            invoice.total_amount if invoice.total_amount is not None else 0
        )
        notes = f"Прибуткова накладна №{invoice.number}"
        if invoice.payment_method:
            method_label = PAYMENT_METHOD_LABELS.get(
                invoice.payment_method, invoice.payment_method.value
            )
            notes += f" ({method_label})"

        await self.ledger_service.create_ledger_entry(
            supplier_id=invoice.supplier_id,
            operation_type="invoice",
            amount=Decimal(str(invoice_amount)),
            operation_date=invoice.invoice_date or invoice.created_at,
            document_id=invoice.id,
            document_number=invoice.number,
            notes=notes,
        )
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
            .options(selectinload(Invoice.items).selectinload(InvoiceItem.product))
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
            # Відкат фіскального залишку (не нижче 0)
            if invoice.is_fiscal:
                await self._decrease_fiscal_stock(item.product_id, item.quantity)

        # Компенсуємо ledger-запис: якщо для накладної існує INVOICE-запис,
        # створюємо CORRECTION-запис (відкат боргу) — інакше нічого не робимо
        # (накладні, підтверджені до появи журналу, не дублюємо)
        ledger_result = await self.session.execute(
            select(SupplierLedger).where(
                SupplierLedger.document_id == invoice.id,
                SupplierLedger.operation_type == LedgerOperationType.INVOICE,
            )
        )
        invoice_ledger_entry = ledger_result.scalars().first()

        if invoice_ledger_entry is not None:
            invoice_amount = (
                invoice.total_amount if invoice.total_amount is not None else 0
            )
            await self.ledger_service.create_ledger_entry(
                supplier_id=invoice.supplier_id,
                operation_type="correction",
                amount=-Decimal(str(invoice_amount)),
                operation_date=datetime.utcnow(),
                document_id=invoice.id,
                document_number=invoice.number,
                notes=f"Скасування накладної №{invoice.number}",
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

        # Ідемпотентність: якщо документ вже проведено — не зменшуємо
        # залишки повторно. Це захищає від подвійного списання, коли
        # create_write_off вже підтвердив документ, а фронтенд/користувач
        # викликає POST /write-offs/{id}/confirm ще раз (кнопка
        # "Створити та підтвердити", batch-confirm тощо).
        if write_off.status == "confirmed":
            return write_off

        for item in write_off.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=-item.quantity,
            )

        write_off.status = "confirmed"
        await self.session.flush()
        return write_off

    # ─── Повернення постачальнику (ReturnInvoice) ────────────────────────────

    async def confirm_return_invoice(
        self,
        return_id: UUID,
        exchange_items: list[dict] | None = None,
    ) -> ReturnInvoice:
        """
        Підтверджує повернення товару постачальнику.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Зменшує залишки товарів на складі
        3. Виконує дію згідно з return_action:
           - deduct_from_debt: зменшує борг постачальника (SupplierLedger)
           - add_to_cash: зачислює суму в касу (створює запис у CashRegister)
           - exchange: створює прибуткову накладну на новий товар
                        та збільшує його залишок
        4. Якщо вказано source_invoice_id — прив'язує повернення до прибуткової
           накладної: ledger entry створюється з document_id = source_invoice_id,
           що дозволяє payment-info прибуткової накладної враховувати повернення

        Args:
            return_id: UUID повернення.
            exchange_items: Список товарів для обміну (якщо return_action = exchange).
                            Кожен елемент: {"product_id": UUID, "quantity": Decimal,
                                             "price": Decimal, "total": Decimal}

        Returns:
            Оновлений об'єкт ReturnInvoice з посиланням на прибуткову накладну.
        """
        result = await self.session.execute(
            select(ReturnInvoice)
            .options(
                selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
                selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            )
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

        # Зменшуємо залишки повернутих товарів
        for item in return_invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=-item.quantity,
            )
            # Повернення постачальнику з фіскального документа:
            # зменшуємо фіскальний залишок (не нижче 0)
            if return_invoice.is_fiscal:
                await self._decrease_fiscal_stock(item.product_id, item.quantity)

        # Виконуємо дію згідно з return_action
        action_label = RETURN_ACTION_LABELS.get(
            return_invoice.return_action,
            return_invoice.return_action.value
        )
        notes = f"Повернення постачальнику №{return_invoice.number} ({action_label})"

        # Визначаємо document_id та document_number для ledger entry:
        # Якщо повернення прив'язане до прибуткової накладної (source_invoice_id),
        # використовуємо ID та номер прибуткової накладної, щоб payment-info
        # для цієї накладної враховувало повернення
        if return_invoice.source_invoice_id:
            doc_id = return_invoice.source_invoice_id
            # Отримуємо номер прибуткової накладної
            src_result = await self.session.execute(
                select(Invoice.number).where(Invoice.id == return_invoice.source_invoice_id)
            )
            src_number = src_result.scalar_one_or_none()
            doc_number = src_number or return_invoice.number
            notes += f" (прив'язано до накладної №{doc_number})"
        else:
            doc_id = return_invoice.id
            doc_number = return_invoice.number

        if return_invoice.total_amount and return_invoice.total_amount > 0:
            if return_invoice.return_action == ReturnActionType.DEDUCT_FROM_DEBT:
                # Списуємо з боргу постачальника (від'ємна сума = зменшення боргу)
                await self.ledger_service.create_ledger_entry(
                    supplier_id=return_invoice.supplier_id,
                    operation_type="return",
                    document_id=doc_id,
                    document_number=doc_number,
                    amount=-return_invoice.total_amount,
                    operation_date=return_invoice.return_date,
                    notes=notes,
                )

            elif return_invoice.return_action == ReturnActionType.ADD_TO_CASH:
                # Зачислюємо в касу (тут має бути логіка CashRegister)
                # Поки що створюємо запис у ledger з позначкою "в касу"
                await self.ledger_service.create_ledger_entry(
                    supplier_id=return_invoice.supplier_id,
                    operation_type="return",
                    document_id=doc_id,
                    document_number=doc_number,
                    amount=Decimal("0.00"),  # Не змінюємо борг
                    operation_date=return_invoice.return_date,
                    notes=notes + " (сума зачислена в касу)",
                )

            elif return_invoice.return_action == ReturnActionType.EXCHANGE:
                # Обмін на інший товар — створюємо прибуткову накладну
                if not exchange_items:
                    raise HTTPException(
                        status_code=status.HTTP_400_BAD_REQUEST,
                        detail="Для обміну (exchange) необхідно вказати exchange_items — "
                               "список товарів, на які відбувається обмін",
                    )

                # Генеруємо номер для нової прибуткової накладної
                invoice_number = await generate_invoice_number(self.session)

                # Розраховуємо загальну суму нової накладної
                exchange_total = sum(
                    Decimal(str(item["total"])) for item in exchange_items
                )

                # Створюємо прибуткову накладну
                new_invoice = Invoice(
                    number=invoice_number,
                    supplier_id=return_invoice.supplier_id,
                    invoice_date=return_invoice.return_date,
                    payment_method=PaymentMethod.CREDIT,  # Обмін — це фактично кредит
                    is_fiscal=return_invoice.is_fiscal,
                    notes=f"Автоматично створено при обміні з повернення №{return_invoice.number}",
                    total_amount=exchange_total,
                    status=InvoiceStatus.DRAFT,
                )
                self.session.add(new_invoice)
                await self.session.flush()

                # Додаємо позиції нової накладної
                for exch_item in exchange_items:
                    item = InvoiceItem(
                        invoice_id=new_invoice.id,
                        product_id=exch_item["product_id"],
                        quantity=exch_item["quantity"],
                        price=exch_item["price"],
                        total=exch_item["total"],
                    )
                    self.session.add(item)

                # Збільшуємо залишки нових товарів
                for exch_item in exchange_items:
                    await self.product_service.update_stock(
                        product_id=exch_item["product_id"],
                        quantity_change=exch_item["quantity"],
                    )

                # Підтверджуємо нову накладну (щоб товар одразу оприбуткувався)
                new_invoice.status = InvoiceStatus.CONFIRMED

                # Зв'язуємо повернення з новою накладною
                return_invoice.exchange_invoice_id = new_invoice.id

                # Створюємо запис у ledger для обміну
                await self.ledger_service.create_ledger_entry(
                    supplier_id=return_invoice.supplier_id,
                    operation_type="return",
                    document_id=doc_id,
                    document_number=doc_number,
                    amount=Decimal("0.00"),  # Не змінюємо борг — це обмін
                    operation_date=return_invoice.return_date,
                    notes=notes + f" (створено прибуткову накладну №{invoice_number})",
                )

        return_invoice.status = ReturnInvoiceStatus.CONFIRMED
        await self.session.flush()
        return return_invoice

    async def cancel_return_invoice(self, return_id: UUID) -> ReturnInvoice:
        """
        Скасовує повернення постачальнику.

        При скасуванні:
        1. Змінює статус на CANCELLED
        2. Відкатує залишки повернутих товарів
        3. Якщо був обмін (exchange_invoice_id) — скасовує прибуткову накладну
           та відкатує залишки нових товарів

        Args:
            return_id: UUID повернення.
        """
        result = await self.session.execute(
            select(ReturnInvoice)
            .options(
                selectinload(ReturnInvoice.items).selectinload(ReturnInvoiceItem.product),
                selectinload(ReturnInvoice.exchange_invoice).selectinload(Invoice.items).selectinload(InvoiceItem.product),
            )
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

        # Якщо був обмін — скасовуємо прибуткову накладну
        if return_invoice.exchange_invoice_id and return_invoice.exchange_invoice:
            exchange_inv = return_invoice.exchange_invoice
            if exchange_inv.status == InvoiceStatus.CONFIRMED:
                # Відкатуємо залишки нових товарів
                for item in exchange_inv.items:
                    await self.product_service.update_stock(
                        product_id=item.product_id,
                        quantity_change=-item.quantity,
                    )
                exchange_inv.status = InvoiceStatus.CANCELLED

        # Відкатуємо залишки повернутих товарів
        for item in return_invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=item.quantity,
            )
            # Відкат фіскального залишку (повертаємо товар у fiscal_stock)
            if return_invoice.is_fiscal:
                await self._increase_fiscal_stock(item.product_id, item.quantity)

        return_invoice.status = ReturnInvoiceStatus.CANCELLED
        await self.session.flush()
        return return_invoice

    # ─── Фіскальні залишки (fiscal_stock) ───────────────────────────────────

    async def _increase_fiscal_stock(
        self, product_id: UUID, quantity: Decimal
    ) -> None:
        """Збільшує fiscal_stock товару (позначає як фіскальний)."""
        product = await self._get_product(product_id)
        if product is None:
            return
        product.is_fiscal = True
        current = Decimal(str(product.fiscal_stock or 0))
        product.fiscal_stock = current + Decimal(str(quantity))
        await self.session.flush()

    async def _decrease_fiscal_stock(
        self, product_id: UUID, quantity: Decimal
    ) -> None:
        """Зменшує fiscal_stock товару (не нижче 0)."""
        product = await self._get_product(product_id)
        if product is None:
            return
        current = Decimal(str(product.fiscal_stock or 0))
        product.fiscal_stock = max(
            Decimal("0"), current - Decimal(str(quantity))
        )
        await self.session.flush()

    async def _get_product(self, product_id: UUID) -> Product | None:
        """Завантажує товар за ID (або None)."""
        result = await self.session.execute(
            select(Product).where(Product.id == product_id)
        )
        return result.scalar_one_or_none()

    # ─── Інвентаризація (Inventory) ──────────────────────────────────────────

    async def confirm_inventory(self, inventory_id: UUID) -> Inventory:
        """
        Підтверджує інвентаризацію.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Для кожної позиції:
           - Якщо difference > 0 (надлишок) → збільшуємо stock
           - Якщо difference < 0 (нестача) → зменшуємо stock (на |difference|)
           - Якщо difference == 0 → нічого не робимо

        Args:
            inventory_id: UUID інвентаризації.
        """
        # Отримуємо інвентаризацію з позиціями
        result = await self.session.execute(
            select(Inventory)
            .options(selectinload(Inventory.items))
            .where(Inventory.id == inventory_id)
        )
        inventory = result.scalar_one_or_none()

        if not inventory:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Інвентаризацію з ID '{inventory_id}' не знайдено",
            )

        if inventory.status != InventoryStatus.DRAFT:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Інвентаризація вже має статус '{inventory.status.value}'",
            )

        # Оновлюємо залишки товарів згідно з різницею
        for item in inventory.items:
            if item.difference != 0:
                await self.product_service.update_stock(
                    product_id=item.product_id,
                    quantity_change=item.difference,
                )

        inventory.status = InventoryStatus.CONFIRMED
        await self.session.flush()
        return inventory

    async def cancel_inventory(self, inventory_id: UUID) -> Inventory:
        """
        Скасовує інвентаризацію.

        При скасуванні:
        1. Змінює статус на CANCELLED
        2. Відкатує залишки (зворотна операція до confirm)

        Args:
            inventory_id: UUID інвентаризації.
        """
        result = await self.session.execute(
            select(Inventory)
            .options(selectinload(Inventory.items))
            .where(Inventory.id == inventory_id)
        )
        inventory = result.scalar_one_or_none()

        if not inventory:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Інвентаризацію з ID '{inventory_id}' не знайдено",
            )

        if inventory.status != InventoryStatus.CONFIRMED:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Скасувати можна лише підтверджену інвентаризацію",
            )

        # Відкатуємо залишки (зворотна операція)
        for item in inventory.items:
            if item.difference != 0:
                await self.product_service.update_stock(
                    product_id=item.product_id,
                    quantity_change=-item.difference,
                )

        inventory.status = InventoryStatus.CANCELLED
        await self.session.flush()
        return inventory
