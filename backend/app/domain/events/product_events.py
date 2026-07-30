"""Domain Events: Product."""

from __future__ import annotations

from dataclasses import dataclass
from uuid import UUID

from .base_event import BaseDomainEvent


@dataclass(kw_only=True)
class ProductCreated(BaseDomainEvent):
    """Створено новий товар."""
    product_id: UUID
    name: str
    barcode: str
    category_id: UUID | None = None
    supplier_id: UUID | None = None


@dataclass(kw_only=True)
class ProductUpdated(BaseDomainEvent):
    """Оновлено товар."""
    product_id: UUID
    changes: dict  # {field_name: (old_value, new_value)}


@dataclass(kw_only=True)
class ProductDeleted(BaseDomainEvent):
    """Видалено товар."""
    product_id: UUID


@dataclass(kw_only=True)
class StockChanged(BaseDomainEvent):
    """Змінено залишок товару."""
    product_id: UUID
    old_quantity: float
    new_quantity: float
    reason: str  # "purchase", "sale", "write_off", "adjustment"
    reference_type: str | None = None  # "invoice", "receipt", "write_off"
    reference_id: UUID | None = None
