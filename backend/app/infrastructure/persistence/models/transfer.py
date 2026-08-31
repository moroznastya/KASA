"""
Моделі Transfer та TransferItem (Переміщення товару).

Фіксує переміщення товару між складами або між магазином і складом.
"""

import uuid
from datetime import datetime
from enum import StrEnum
from typing import TYPE_CHECKING

from sqlalchemy import (
    DateTime,
    Enum,
    ForeignKey,
    Numeric,
    String,
    Text,
)
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.product import Product
    from app.infrastructure.persistence.models.user import User


class TransferStatus(StrEnum):
    """Статус переміщення."""
    DRAFT = "draft"
    CONFIRMED = "confirmed"   # Товар переміщено
    CANCELLED = "cancelled"


class Transfer(Base):
    """Переміщення товару між складами / магазинами."""

    __tablename__ = "transfers"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор переміщення",
    )
    number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер документа переміщення",
    )
    from_location: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        comment="Звідки переміщуємо (склад/магазин)",
    )
    to_location: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        comment="Куди переміщуємо (склад/магазин)",
    )

    # ── Користувач, який створив ─────────────────
    created_by_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("users.id", ondelete="RESTRICT"),
        nullable=False,
        comment="Ідентифікатор користувача, який створив переміщення",
    )
    creator: Mapped["User"] = relationship(
        "User",
        foreign_keys=[created_by_id],
    )

    transfer_date: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата переміщення",
    )
    status: Mapped[TransferStatus] = mapped_column(
        Enum(TransferStatus, name="transfer_status", create_constraint=True, values_callable=lambda x: [e.value for e in x]),
        default=TransferStatus.DRAFT,
        nullable=False,
        comment="Статус переміщення",
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
    items: Mapped[list["TransferItem"]] = relationship(
        "TransferItem",
        back_populates="transfer",
        cascade="all, delete-orphan",
    )

    def __repr__(self) -> str:
        return f"<Transfer {self.number}: {self.from_location} → {self.to_location}>"


class TransferItem(Base):
    """Позиція переміщення (один товар)."""

    __tablename__ = "transfer_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор позиції",
    )
    transfer_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("transfers.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор переміщення",
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
        comment="Кількість товару",
    )

    # ── Ціни на момент переміщення ─────────────
    cost_price: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        default=0,
        comment="Собівартість одиниці товару",
    )
    price: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        default=0,
        comment="Ціна продажу одиниці товару",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )

    # ── Зв'язки ─────────────────────────────────
    transfer: Mapped["Transfer"] = relationship(
        "Transfer",
        back_populates="items",
    )
    product: Mapped["Product"] = relationship(
        "Product",
        back_populates="transfer_items",
    )

    def __repr__(self) -> str:
        return f"<TransferItem {self.product_id} x{self.quantity}>"
