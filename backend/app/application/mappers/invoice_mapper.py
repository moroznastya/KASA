"""
Mapper для Invoice entity.

Конвертує між Invoice (domain entity) та InvoiceDTO / InvoiceCreateDTO.
"""

from decimal import Decimal

from app.domain.entities.invoice import Invoice, InvoiceItem, InvoiceStatus
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate
from app.application.dto.invoice_dto import InvoiceDTO, InvoiceItemDTO, InvoiceCreateDTO, InvoiceConfirmDTO


class InvoiceMapper:
    """Статичний mapper для конвертації Invoice entity <-> DTO."""

    @staticmethod
    def entity_to_dto(entity: Invoice) -> InvoiceDTO:
        """
        Конвертує Invoice entity в InvoiceDTO.

        Args:
            entity: Invoice entity.

        Returns:
            InvoiceDTO.
        """
        items = [
            InvoiceItemDTO(
                product_id=item.product_id,
                quantity=float(item.quantity.value),
                price=float(item.price.amount),
                tax_rate=item.tax_rate.percent,
                name=item.name,
            )
            for item in entity.items
        ]
        return InvoiceDTO(
            id=entity.id,
            number=entity.number,
            supplier_id=entity.supplier_id,
            items=items,
            total=float(entity.total.amount) if entity.total else None,
            status=entity.status.value,
            created_at=entity.created_at,
            confirmed_at=entity.confirmed_at,
            notes=entity.notes,
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
