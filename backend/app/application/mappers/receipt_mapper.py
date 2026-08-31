"""
Mapper для Receipt entity.

Конвертує між Receipt (domain entity) та ReceiptDTO / ReceiptCreateDTO.
"""

from decimal import Decimal

from app.application.dto.receipt_dto import ReceiptCreateDTO, ReceiptDTO, ReceiptItemDTO
from app.domain.entities.receipt import (
    FiscalStatus,
    PaymentMethod,
    Receipt,
    ReceiptItem,
)
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate


class ReceiptMapper:
    """Статичний mapper для конвертації Receipt entity <-> DTO.

    Підтримує як domain entity (quantity=Quantity, price=Money,
    payment_method=PaymentMethod), так і ORM-модель (quantity=float,
    price=float, receipt_number замість number, debtor_id замість customer_id).
    """

    @staticmethod
    def _amount(value):
        """Повертає float з Money або float/Decimal."""
        if value is None:
            return None
        if hasattr(value, "amount"):
            return float(value.amount)
        return float(value)

    @staticmethod
    def _quantity(value):
        """Повертає float з Quantity або float/Decimal."""
        if value is None:
            return 0.0
        if hasattr(value, "value"):
            return float(value.value)
        return float(value)

    @staticmethod
    def _tax_percent(value) -> int:
        """Повертає відсоток з TaxRate або float/int; ORM не зберігає tax_rate."""
        if value is None:
            return 20
        if hasattr(value, "percent"):
            return int(value.percent)
        return int(value)

    @staticmethod
    def entity_to_dto(entity: Receipt) -> ReceiptDTO:
        """
        Конвертує Receipt entity (або ORM Receipt) в ReceiptDTO.

        Args:
            entity: Receipt entity або ORM-модель Receipt.

        Returns:
            ReceiptDTO.
        """
        items = [
            ReceiptItemDTO(
                product_id=item.product_id,
                name=getattr(item, "name", "") or "",
                quantity=ReceiptMapper._quantity(item.quantity),
                price=ReceiptMapper._amount(item.price),
                tax_rate=ReceiptMapper._tax_percent(getattr(item, "tax_rate", None)),
            )
            for item in entity.items
        ]
        return ReceiptDTO(
            id=entity.id,
            number=getattr(entity, "number", None) or getattr(entity, "receipt_number", "") or "",
            items=items,
            total=ReceiptMapper._amount(
                getattr(entity, "total", None) or getattr(entity, "total_amount", None)
            ),
            payment_method=entity.payment_method.value
            if hasattr(entity.payment_method, "value")
            else str(entity.payment_method),
            created_at=getattr(entity, "created_at", None),
            cash_amount=ReceiptMapper._amount(getattr(entity, "cash_amount", None)),
            card_amount=ReceiptMapper._amount(getattr(entity, "card_amount", None)),
            change_amount=ReceiptMapper._amount(getattr(entity, "change_amount", None)),
            customer_id=getattr(entity, "customer_id", None) or getattr(entity, "debtor_id", None),
            notes=getattr(entity, "notes", "") or "",
            is_fiscal=getattr(entity, "is_fiscal", False),
            fiscal_status=entity.fiscal_status.value
            if hasattr(entity.fiscal_status, "value")
            else str(entity.fiscal_status),
            fiscal_number=getattr(entity, "fiscal_number", None),
            fiscal_serial=getattr(entity, "fiscal_serial", None),
            fiscal_sent_at=getattr(entity, "fiscal_sent_at", None),
            fiscal_error=getattr(entity, "fiscal_error", None),
            split_group_id=getattr(entity, "split_group_id", None),
            # ── Дані банківської транзакції терміналу (ПриватБанк) ──
            terminal_rrn=getattr(entity, "terminal_rrn", None),
            terminal_approval_code=getattr(entity, "terminal_approval_code", None),
            terminal_invoice_number=getattr(entity, "terminal_invoice_number", None),
            terminal_transaction_id=getattr(entity, "terminal_transaction_id", None),
            terminal_response_code=getattr(entity, "terminal_response_code", None),
            terminal_status=getattr(entity, "terminal_status", None),
            terminal_receipt=getattr(entity, "terminal_receipt", None),
            terminal_card_pan=getattr(entity, "terminal_card_pan", None),
            terminal_payment_system=getattr(entity, "terminal_payment_system", None),
            terminal_merchant=getattr(entity, "terminal_merchant", None),
            terminal_created_at=getattr(entity, "terminal_created_at", None),
        )

    @staticmethod
    def dto_to_entity(dto: ReceiptDTO) -> Receipt:
        """
        Конвертує ReceiptDTO назад в Receipt entity.

        Args:
            dto: ReceiptDTO.

        Returns:
            Receipt entity.
        """
        receipt = Receipt(
            id=dto.id,
            number=dto.number,
            payment_method=PaymentMethod(dto.payment_method),
            created_at=dto.created_at,
            cash_amount=Money(Decimal(str(dto.cash_amount))) if dto.cash_amount is not None else None,
            card_amount=Money(Decimal(str(dto.card_amount))) if dto.card_amount is not None else None,
            change_amount=Money(Decimal(str(dto.change_amount))) if dto.change_amount is not None else None,
            customer_id=dto.customer_id,
            notes=dto.notes,
            is_fiscal=dto.is_fiscal,
            fiscal_status=FiscalStatus(dto.fiscal_status),
            fiscal_number=dto.fiscal_number,
            fiscal_serial=dto.fiscal_serial,
            fiscal_sent_at=dto.fiscal_sent_at,
            fiscal_error=dto.fiscal_error,
            split_group_id=dto.split_group_id,
        )
        for item_dto in dto.items:
            receipt.add_item(ReceiptItem(
                product_id=item_dto.product_id,
                name=item_dto.name,
                quantity=Quantity(Decimal(str(item_dto.quantity))),
                price=Money(Decimal(str(item_dto.price))),
                tax_rate=TaxRate(Decimal(str(item_dto.tax_rate))),
            ))
        return receipt

    @staticmethod
    def create_dto_to_entity(dto: ReceiptCreateDTO) -> Receipt:
        """
        Конвертує ReceiptCreateDTO в нову Receipt entity.

        Args:
            dto: ReceiptCreateDTO.

        Returns:
            Нова Receipt entity.
        """
        receipt = Receipt(
            payment_method=PaymentMethod(dto.payment_method),
            cash_amount=Money(Decimal(str(dto.cash_amount))) if dto.cash_amount is not None else None,
            card_amount=Money(Decimal(str(dto.card_amount))) if dto.card_amount is not None else None,
            customer_id=dto.customer_id,
            notes=dto.notes,
            is_fiscal=dto.is_fiscal,
            fiscal_status=FiscalStatus.PENDING if dto.is_fiscal else FiscalStatus.NONE,
            split_group_id=dto.split_group_id,
            # ── Дані банківської транзакції терміналу (ПриватБанк) ──
            terminal_rrn=dto.terminal_rrn,
            terminal_approval_code=dto.terminal_approval_code,
            terminal_invoice_number=dto.terminal_invoice_number,
            terminal_transaction_id=dto.terminal_transaction_id,
            terminal_response_code=dto.terminal_response_code,
            terminal_status=dto.terminal_status,
            terminal_receipt=dto.terminal_receipt,
            terminal_card_pan=dto.terminal_card_pan,
            terminal_payment_system=dto.terminal_payment_system,
            terminal_merchant=dto.terminal_merchant,
            terminal_created_at=dto.terminal_created_at,
        )
        for item_dto in dto.items:
            receipt.add_item(ReceiptItem(
                product_id=item_dto.product_id,
                name=item_dto.name,
                quantity=Quantity(Decimal(str(item_dto.quantity))),
                price=Money(Decimal(str(item_dto.price))),
                tax_rate=TaxRate(Decimal(str(item_dto.tax_rate))),
            ))
        return receipt
