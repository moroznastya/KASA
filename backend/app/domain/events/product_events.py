"""
Доменні події модуля Product (Товари).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from typing import Optional
from uuid import UUID

from .base_event import DomainEvent


@dataclass(frozen=True)
class ProductCreated(DomainEvent):
    """
    Подія: товар створено.

    Публікується після успішного створення нового товару.
    """

    product_id: UUID = field(default_factory=UUID)
    name: str = ""
    barcode: Optional[str] = None
    sku: str = ""
    category_id: Optional[UUID] = None
    supplier_id: Optional[UUID] = None


@dataclass(frozen=True)
class ProductUpdated(DomainEvent):
    """
    Подія: товар оновлено.

    Публікується після успішного оновлення даних товару.
    """

    product_id: UUID = field(default_factory=UUID)
    name: str = ""
    changed_fields: tuple[str, ...] = field(default_factory=tuple)


@dataclass(frozen=True)
class StockChanged(DomainEvent):
    """
    Подія: змінено залишок товару.

    Публікується після зміни кількості товару на складі.
    """

    product_id: UUID = field(default_factory=UUID)
    old_quantity: Decimal = Decimal("0")
    new_quantity: Decimal = Decimal("0")
    change_amount: Decimal = Decimal("0")
    reason: str = ""
    document_id: Optional[UUID] = None
    document_type: str = ""
