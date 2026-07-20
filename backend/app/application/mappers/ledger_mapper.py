"""
Mapper для LedgerEntry entity.

Конвертує між LedgerEntry (domain entity) та LedgerEntryDTO / LedgerCreateDTO.
"""

from decimal import Decimal

from app.domain.entities.ledger_entry import LedgerEntry, OperationType
from app.domain.value_objects.money import Money
from app.application.dto.ledger_dto import LedgerEntryDTO, LedgerCreateDTO


class LedgerMapper:
    """Статичний mapper для конвертації LedgerEntry entity <-> DTO."""

    @staticmethod
    def entity_to_dto(entity: LedgerEntry) -> LedgerEntryDTO:
        """
        Конвертує LedgerEntry entity в LedgerEntryDTO.

        Args:
            entity: LedgerEntry entity.

        Returns:
            LedgerEntryDTO.
        """
        return LedgerEntryDTO(
            id=entity.id,
            supplier_id=entity.supplier_id,
            amount=float(entity.amount.amount),
            operation_type=entity.operation_type.value,
            balance_after=float(entity.balance_after.amount) if entity.balance_after else None,
            created_at=entity.created_at,
            document_id=entity.document_id,
            document_number=entity.document_number,
            notes=entity.notes,
        )

    @staticmethod
    def dto_to_entity(dto: LedgerEntryDTO) -> LedgerEntry:
        """
        Конвертує LedgerEntryDTO назад в LedgerEntry entity.

        Args:
            dto: LedgerEntryDTO.

        Returns:
            LedgerEntry entity.
        """
        return LedgerEntry(
            id=dto.id,
            supplier_id=dto.supplier_id,
            amount=Money(Decimal(str(dto.amount))),
            operation_type=OperationType(dto.operation_type),
            balance_after=Money(Decimal(str(dto.balance_after))) if dto.balance_after is not None else None,
            created_at=dto.created_at,
            document_id=dto.document_id,
            document_number=dto.document_number,
            notes=dto.notes,
        )

    @staticmethod
    def create_dto_to_entity(dto: LedgerCreateDTO) -> LedgerEntry:
        """
        Конвертує LedgerCreateDTO в нову LedgerEntry entity.

        Args:
            dto: LedgerCreateDTO.

        Returns:
            Нова LedgerEntry entity.
        """
        return LedgerEntry(
            supplier_id=dto.supplier_id,
            amount=Money(Decimal(str(dto.amount))),
            operation_type=OperationType(dto.operation_type),
            document_id=dto.document_id,
            document_number=dto.document_number,
            notes=dto.notes,
        )
