"""
Моделі Inventory та InventoryItem (Інвентаризація).

Фіксує фактичні залишки товарів на складі/магазині та порівнює
з обліковими даними для виявлення розбіжностей.
"""

import uuid
from datetime import datetime
from enum import Enum as PyEnum
from typing import Optional

from sqlalchemy import (
    ForeignKey, String, Text, Numeric, Enum, DateTime,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


class InventoryStatus(str, PyEnum):
    """Статус інвентаризації."""
    DRAFT = "draft"
    CONFIRMED = "confirmed"   # Інвентаризацію проведено
    CANCELLED = "cancelled"


class Inventory(Base):
    """Інвентаризація (звіряння фактичних та облікових залишків)."""

    __tablename__ = "inventories"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор інвентаризації",
    )
    number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер документа інвентаризації (префікс ІН-)",
    )
    location: Mapped[Optional[str]] = mapped_column(
        String(255),
        nullable=True,
        default=None,
        comment="Локація проведення (магазин/склад)",
    )
    inventory_date: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата проведення інвентаризації",
    )
    status: Mapped[InventoryStatus] = mapped_column(
        Enum(InventoryStatus, name="inventory_status", create_constraint=True, values_callable=lambda x: [e.value for e in x]),
        default=InventoryStatus.DRAFT,
        nullable=False,
        comment="Статус інвентаризації",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Додаткові нотатки",
    )

    # ── Користувач, який створив ─────────────────
    created_by_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("users.id", ondelete="RESTRICT"),
        nullable=False,
        comment="Ідентифікатор користувача, який створив інвентаризацію",
    )
    creator: Mapped["User"] = relationship(
        "User",
        foreign_keys=[created_by_id],
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )
    updated_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        onupdate=datetime.utcnow,
        comment="Дата останнього оновлення",
    )

    # ── Зв'язки ─────────────────────────────────
    items: Mapped[list["InventoryItem"]] = relationship(
        "InventoryItem",
        back_populates="inventory",
        cascade="all, delete-orphan",
    )

    def __repr__(self) -> str:
        return f"<Inventory {self.number} ({self.location})>"


class InventoryItem(Base):
    """Позиція інвентаризації — фіксує фактичні залишки та ціни на момент проведення."""

    __tablename__ = "inventory_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор позиції",
    )
    inventory_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("inventories.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор інвентаризації",
    )
    product_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("products.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор товару",
    )

    # ── Кількість ───────────────────────────────
    actual_quantity: Mapped[float] = mapped_column(
        Numeric(10, 3),
        nullable=False,
        default=0,
        comment="Фактична кількість (введена користувачем)",
    )
    accounting_quantity: Mapped[float] = mapped_column(
        Numeric(10, 3),
        nullable=False,
        default=0,
        comment="Облікова кількість (поточний залишок)",
    )
    difference: Mapped[float] = mapped_column(
        Numeric(10, 3),
        nullable=False,
        default=0,
        comment="Різниця = actual - accounting",
    )

    # ── Ціни на момент інвентаризації ───────────
    cost_price: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        default=0,
        comment="Собівартість одиниці товару на момент інвентаризації",
    )
    price: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        default=0,
        comment="Ціна продажу одиниці товару на момент інвентаризації",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )

    # ── Зв'язки ─────────────────────────────────
    inventory: Mapped["Inventory"] = relationship(
        "Inventory",
        back_populates="items",
    )
    product: Mapped["Product"] = relationship(
        "Product",
        back_populates="inventory_items",
    )

    def __repr__(self) -> str:
        return f"<InventoryItem {self.product_id} actual={self.actual_quantity} accounting={self.accounting_quantity}>"
