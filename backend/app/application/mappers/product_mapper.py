"""
Mapper для Product entity.

Конвертує між Product (domain entity) та ProductDTO / ProductCreateDTO / ProductUpdateDTO.
"""

from decimal import Decimal
from typing import Optional

from app.domain.entities.product import Product
from app.domain.value_objects.barcode import Barcode
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.domain.value_objects.tax_rate import TaxRate
from app.application.dto.product_dto import ProductDTO, ProductCreateDTO, ProductUpdateDTO


class ProductMapper:
    """Статичний mapper для конвертації Product entity <-> DTO."""

    @staticmethod
    def entity_to_dto(entity: Product) -> ProductDTO:
        """
        Конвертує Product entity в ProductDTO.

        Args:
            entity: Product entity.

        Returns:
            ProductDTO.
        """
        return ProductDTO(
            id=entity.id,
            name=entity.name,
            barcode=str(entity.barcode) if entity.barcode else None,
            price=float(entity.price.amount) if entity.price else None,
            cost_price=float(entity.cost_price.amount) if entity.cost_price else None,
            stock=float(entity.stock.value) if entity.stock else None,
            category_id=entity.category_id,
            supplier_id=entity.supplier_id,
            tax_rate=entity.tax_rate.percent if entity.tax_rate else 20,
            sku=entity.sku,
            unit=entity.unit,
            is_active=entity.is_active,
            description=entity.description,
        )

    @staticmethod
    def dto_to_entity(dto: ProductDTO) -> Product:
        """
        Конвертує ProductDTO назад в Product entity.

        Args:
            dto: ProductDTO.

        Returns:
            Product entity.
        """
        return Product(
            id=dto.id,
            name=dto.name,
            barcode=Barcode(dto.barcode) if dto.barcode else None,
            price=Money(Decimal(str(dto.price))) if dto.price is not None else None,
            cost_price=Money(Decimal(str(dto.cost_price))) if dto.cost_price is not None else None,
            stock=Quantity(Decimal(str(dto.stock)), dto.unit) if dto.stock is not None else None,
            category_id=dto.category_id,
            supplier_id=dto.supplier_id,
            tax_rate=TaxRate(Decimal(str(dto.tax_rate))),
            sku=dto.sku,
            unit=dto.unit,
            is_active=dto.is_active,
            description=dto.description,
        )

    @staticmethod
    def create_dto_to_entity(dto: ProductCreateDTO) -> Product:
        """
        Конвертує ProductCreateDTO в нову Product entity.

        Args:
            dto: ProductCreateDTO.

        Returns:
            Нова Product entity.
        """
        return Product(
            name=dto.name,
            barcode=Barcode(dto.barcode) if dto.barcode else None,
            price=Money(Decimal(str(dto.price))) if dto.price is not None else None,
            cost_price=Money(Decimal(str(dto.cost_price))) if dto.cost_price is not None else None,
            stock=Quantity(Decimal(str(dto.stock)), dto.unit) if dto.stock is not None else None,
            category_id=dto.category_id,
            supplier_id=dto.supplier_id,
            tax_rate=TaxRate(Decimal(str(dto.tax_rate))),
            sku=dto.sku,
            unit=dto.unit,
            is_active=dto.is_active,
            description=dto.description,
        )

    @staticmethod
    def apply_update(entity: Product, dto: ProductUpdateDTO) -> Product:
        """
        Застосовує оновлення з ProductUpdateDTO до існуючої Product entity.

        Args:
            entity: Існуюча Product entity.
            dto: ProductUpdateDTO з полями для оновлення.

        Returns:
            Оновлена Product entity.
        """
        if dto.name is not None:
            entity.name = dto.name
        if dto.barcode is not None:
            entity.barcode = Barcode(dto.barcode) if dto.barcode else None
        if dto.price is not None:
            entity.price = Money(Decimal(str(dto.price)))
        if dto.cost_price is not None:
            entity.cost_price = Money(Decimal(str(dto.cost_price)))
        if dto.stock is not None:
            entity.stock = Quantity(Decimal(str(dto.stock)), dto.unit or entity.unit)
        if dto.category_id is not None:
            entity.category_id = dto.category_id
        if dto.supplier_id is not None:
            entity.supplier_id = dto.supplier_id
        if dto.tax_rate is not None:
            entity.tax_rate = TaxRate(Decimal(str(dto.tax_rate)))
        if dto.sku is not None:
            entity.sku = dto.sku
        if dto.unit is not None:
            entity.unit = dto.unit
        if dto.is_active is not None:
            entity.is_active = dto.is_active
        if dto.description is not None:
            entity.description = dto.description
        return entity
