"""
Mapper для Supplier entity.

Конвертує між Supplier (domain entity) та SupplierDTO / SupplierCreateDTO.
"""

from decimal import Decimal

from app.application.dto.supplier_dto import SupplierCreateDTO, SupplierDTO
from app.domain.entities.supplier import Supplier
from app.domain.value_objects.money import Money


class SupplierMapper:
    """Статичний mapper для конвертації Supplier entity <-> DTO."""

    @staticmethod
    def entity_to_dto(entity: Supplier) -> SupplierDTO:
        """
        Конвертує Supplier entity в SupplierDTO.

        Args:
            entity: Supplier entity.

        Returns:
            SupplierDTO.
        """
        return SupplierDTO(
            id=entity.id,
            name=entity.name,
            balance=float(entity.balance.amount) if entity.balance else None,
            contact_person=entity.contact_person,
            phone=entity.phone,
            email=entity.email,
            address=entity.address,
            edrpou=entity.edrpou,
            is_active=entity.is_active,
            created_at=entity.created_at,
            notes=entity.notes,
        )

    @staticmethod
    def dto_to_entity(dto: SupplierDTO) -> Supplier:
        """
        Конвертує SupplierDTO назад в Supplier entity.

        Args:
            dto: SupplierDTO.

        Returns:
            Supplier entity.
        """
        return Supplier(
            id=dto.id,
            name=dto.name,
            balance=Money(Decimal(str(dto.balance))) if dto.balance is not None else Money.zero(),
            contact_person=dto.contact_person,
            phone=dto.phone,
            email=dto.email,
            address=dto.address,
            edrpou=dto.edrpou,
            is_active=dto.is_active,
            created_at=dto.created_at,
            notes=dto.notes,
        )

    @staticmethod
    def create_dto_to_entity(dto: SupplierCreateDTO) -> Supplier:
        """
        Конвертує SupplierCreateDTO в нову Supplier entity.

        Args:
            dto: SupplierCreateDTO.

        Returns:
            Нова Supplier entity.
        """
        return Supplier(
            name=dto.name,
            contact_person=dto.contact_person,
            phone=dto.phone,
            email=dto.email,
            address=dto.address,
            edrpou=dto.edrpou,
            is_active=dto.is_active,
            notes=dto.notes,
        )
