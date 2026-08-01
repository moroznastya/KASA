"""
Mapper для Receipt entity.

Конвертує між Receipt (domain entity) та ReceiptDTO / ReceiptCreateDTO.
"""

from decimal import Decimal
from typing import Optional

from app.domain.entities.receipt import (
    Receipt,
    ReceiptItem,
    PaymentMethod,
    FiscalStatus,
)
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate
from app.application.dto.receipt_dto import ReceiptDTO, ReceiptItemDTO, ReceiptCreateDTO


class ReceiptMapper:
    """Статичний mapper для конвертації Receipt entity <-> DTO."""

    @staticmethod
    def entity_to_dto(entity: Receipt) -> ReceiptDTO:
        """
        Конвертує Receipt entity в ReceiptDTO.

        Args:
            entity: Receipt entity.

        Returns:
            ReceiptDTO.
        """
        items = [
            ReceiptItemDTO(
                product_id=item.product_id,
                name=item.name,
                quantity=float(item.quantity.value),
                price=float(item.price.amount),
                tax_rate=item.tax_rate.percent,
            )
            for item in entity.items
        ]
        return ReceiptDTO(
            id=entity.id,
            number=entity.number,
            items=items,
            total=float(entity.total.amount) if entity.total else None,
            payment_method=entity.payment_method.value,
            created_at=entity.created_at,
            cash_amount=float(entity.cash_amount.amount) if entity.cash_amount else None,
            card_amount=float(entity.card_amount.amount) if entity.card_amount else None,
            change_amount=float(entity.change_amount.amount) if entity.change_amount else None,
            customer_id=entity.customer_id,
            notes=entity.notes,
            is_fiscal=entity.is_fiscal,
            fiscal_status=entity.fiscal_status.value
            if hasattr(entity.fiscal_status, "value")
            else str(entity.fiscal_status),
            fiscal_number=entity.fiscal_number,
            fiscal_serial=entity.fiscal_serial,
            fiscal_sent_at=entity.fiscal_sent_at,
            fiscal_error=entity.fiscal_error,
            split_group_id=entity.split_group_id,
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
