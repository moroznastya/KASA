"""
Mapper для Invoice entity.

Конвертує між Invoice (domain entity) та InvoiceDTO / InvoiceCreateDTO.
"""

from decimal import Decimal

from app.application.dto.invoice_dto import InvoiceCreateDTO, InvoiceDTO, InvoiceItemDTO
from app.domain.entities.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate


class InvoiceMapper:
    """Статичний mapper для конвертації Invoice entity <-> DTO.

    Підтримує як domain entity (quantity=Quantity, price=Money, tax_rate=TaxRate),
    так і ORM-модель (quantity=float, price=float, tax_rate відсутній).
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
    def entity_to_dto(entity: Invoice) -> InvoiceDTO:
        """
        Конвертує Invoice entity (або ORM Invoice) в InvoiceDTO.

        Args:
            entity: Invoice entity або ORM-модель Invoice.

        Returns:
            InvoiceDTO.
        """
        items = [
            InvoiceItemDTO(
                product_id=item.product_id,
                quantity=InvoiceMapper._quantity(item.quantity),
                price=InvoiceMapper._amount(item.price),
                tax_rate=InvoiceMapper._tax_percent(getattr(item, "tax_rate", None)),
                name=getattr(item, "name", "") or "",
            )
            for item in entity.items
        ]
        return InvoiceDTO(
            id=entity.id,
            number=entity.number,
            supplier_id=entity.supplier_id,
            items=items,
            total=InvoiceMapper._amount(
                getattr(entity, "total", None) or getattr(entity, "total_amount", None)
            ),
            status=entity.status.value
            if hasattr(entity.status, "value")
            else str(entity.status),
            created_at=getattr(entity, "created_at", None),
            confirmed_at=getattr(entity, "confirmed_at", None),
            notes=getattr(entity, "notes", "") or "",
            is_fiscal=getattr(entity, "is_fiscal", False),
        )

    @staticmethod
    def dto_to_entity(dto: InvoiceDTO) -> Invoice:
        """
        Конвертує InvoiceDTO назад в Invoice entity.

        Args:
            dto: InvoiceDTO.

        Returns:
            Invoice entity.
        """
        invoice = Invoice(
            id=dto.id,
            number=dto.number,
            supplier_id=dto.supplier_id,
            status=InvoiceStatus(dto.status),
            created_at=dto.created_at,
            confirmed_at=dto.confirmed_at,
            notes=dto.notes,
            is_fiscal=dto.is_fiscal,
        )
        for item_dto in dto.items:
            invoice.add_item(InvoiceItem(
                product_id=item_dto.product_id,
                quantity=Quantity(Decimal(str(item_dto.quantity))),
                price=Money(Decimal(str(item_dto.price))),
                tax_rate=TaxRate(Decimal(str(item_dto.tax_rate))),
                name=item_dto.name,
            ))
        return invoice

    @staticmethod
    def create_dto_to_entity(dto: InvoiceCreateDTO) -> Invoice:
        """
        Конвертує InvoiceCreateDTO в нову Invoice entity.

        Args:
            dto: InvoiceCreateDTO.

        Returns:
            Нова Invoice entity.
        """
        invoice = Invoice(
            number=dto.number,
            supplier_id=dto.supplier_id,
            notes=dto.notes,
            is_fiscal=dto.is_fiscal,
        )
        for item_dto in dto.items:
            invoice.add_item(InvoiceItem(
                product_id=item_dto.product_id,
                quantity=Quantity(Decimal(str(item_dto.quantity))),
                price=Money(Decimal(str(item_dto.price))),
                tax_rate=TaxRate(Decimal(str(item_dto.tax_rate))),
                name=item_dto.name,
            ))
        return invoice
