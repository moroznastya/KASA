"""
Моделі WriteOff та WriteOffItem (Списання товару).

Фіксує списання товару зі складу (псування, термін придатності, крадіжка тощо).
Причина списання (reason) — РЯДОК: назва з персистентного довідника
write_off_reasons (див. models/reasons.py). Користувач може додати нову
причину, яка зберігається в довіднику і доступна в наступних накладних.
"""

import uuid
from datetime import datetime
from typing import TYPE_CHECKING

import sqlalchemy as sa
from sqlalchemy import (
    DateTime,
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

    # ── Ідемпотентність push (offline-first sync, дизайн 8.2) ────────────
    client_uuid: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        nullable=True,
        comment="UUID транзакції з каси — ключ ідемпотентного прийому push",
    )

    __table_args__ = (
        sa.Index(
            "uq_write_offs_client_uuid",
            "client_uuid",
            unique=True,
            postgresql_where=sa.text("client_uuid IS NOT NULL"),
        ),
    )
    number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер документа списання",
    )
    reason: Mapped[str] = mapped_column(
        String(100),
        nullable=False,
        comment="Причина списання (назва з довідника write_off_reasons)",
    )
    # Deprecated: раніше використовувалось для довільної причини (reason='other').
    # Тепер причина — завжди назва рядком з персистентного списку.
    # Колонку залишено для зворотної сумісності зі старими даними.
    custom_reason: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Deprecated: довільна причина списання (більше не використовується)",
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
    status: Mapped[str] = mapped_column(
        String(20),
        default="confirmed",
        nullable=False,
        comment="Статус списання (confirmed за замовчуванням)",
    )
    total_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        default=0.00,
        comment="Загальна сума списання (грн)",
    )

    # ── Користувач, який створив ─────────────────
    created_by_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("users.id", ondelete="RESTRICT"),
        nullable=False,
        comment="Ідентифікатор користувача, який створив списання",
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
    items: Mapped[list["WriteOffItem"]] = relationship(
        "WriteOffItem",
        back_populates="write_off",
        cascade="all, delete-orphan",
    )

    def __repr__(self) -> str:
        return f"<WriteOff {self.number} ({self.reason})>"


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

    # ── Ціни на момент списання ────────────────
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
