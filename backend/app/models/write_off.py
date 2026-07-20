"""
Моделі WriteOff та WriteOffItem (Списання товару).

Фіксує списання товару зі складу (псування, термін придатності, крадіжка тощо).
"""

import uuid
from datetime import datetime
from enum import Enum as PyEnum

from sqlalchemy import (
    ForeignKey, String, Text, Numeric, Enum, DateTime,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


class WriteOffReason(str, PyEnum):
    """Причина списання товару."""
    EXPIRED = "expired"           # Закінчився термін придатності
    DAMAGED = "damaged"           # Пошкодження / бій
    DEFECT = "defect"             # Брак / дефект
    THEFT = "theft"               # Крадіжка
    INVENTORY = "inventory"       # Інвентаризація (нестача)
    OTHER = "other"               # Інше


class WriteOff(Base):
    """Списання товару зі складу."""

    __tablename__ = "write_offs"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор списання",
    )
    number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер документа списання",
    )
    reason: Mapped[WriteOffReason] = mapped_column(
        Enum(WriteOffReason, name="write_off_reason", create_constraint=True),
        nullable=False,
        comment="Причина списання",
    )
    write_off_date: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата списання",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Додаткові нотатки",
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
    items: Mapped[list["WriteOffItem"]] = relationship(
        "WriteOffItem",
        back_populates="write_off",
        cascade="all, delete-orphan",
    )

    def __repr__(self) -> str:
        return f"<WriteOff {self.number} ({self.reason.value})>"


class WriteOffItem(Base):
    """Позиція списання (один товар)."""

    __tablename__ = "write_off_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор позиції",
    )
    write_off_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("write_offs.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор списання",
    )
    product_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("products.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор товару",
    )
    quantity: Mapped[float] = mapped_column(
        Numeric(10, 3),
        nullable=False,
        comment="Кількість списаного товару",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )

    # ── Зв'язки ─────────────────────────────────
    write_off: Mapped["WriteOff"] = relationship(
        "WriteOff",
        back_populates="items",
    )
    product: Mapped["Product"] = relationship(
        "Product",
        back_populates="write_off_items",
    )

    def __repr__(self) -> str:
        return f"<WriteOffItem {self.product_id} x{self.quantity}>"
