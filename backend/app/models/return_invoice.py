"""
Моделі ReturnInvoice та ReturnInvoiceItem (Повернення постачальнику).

Фіксує повернення товару постачальнику (брак, пересортиця тощо).
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


class ReturnInvoiceStatus(str, PyEnum):
    """Статус повернення постачальнику."""
    DRAFT = "draft"
    CONFIRMED = "confirmed"   # Товар повернуто
    CANCELLED = "cancelled"


class ReturnInvoice(Base):
    """Повернення товару постачальнику."""

    __tablename__ = "return_invoices"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор повернення",
    )
    number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер документа повернення",
    )
    supplier_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("suppliers.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор постачальника",
    )
    return_date: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата повернення",
    )
    status: Mapped[ReturnInvoiceStatus] = mapped_column(
        Enum(ReturnInvoiceStatus, name="return_invoice_status", create_constraint=True, values_callable=lambda x: [e.value for e in x]),
        default=ReturnInvoiceStatus.DRAFT,
        nullable=False,
        comment="Статус повернення",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Причина повернення / додаткові нотатки",
    )
    total_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        default=0.00,
        comment="Загальна сума повернення (грн)",
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
    supplier: Mapped["Supplier"] = relationship(
        "Supplier",
    )
    items: Mapped[list["ReturnInvoiceItem"]] = relationship(
        "ReturnInvoiceItem",
        back_populates="return_invoice",
        cascade="all, delete-orphan",
    )

    def __repr__(self) -> str:
        return f"<ReturnInvoice {self.number}>"


class ReturnInvoiceItem(Base):
    """Позиція повернення постачальнику (один товар)."""

    __tablename__ = "return_invoice_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор позиції",
    )
    return_invoice_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("return_invoices.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор повернення",
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
    price: Mapped[float] = mapped_column(
        Numeric(10, 2),
        nullable=False,
        comment="Ціна за одиницю (грн)",
    )
    total: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        comment="Загальна сума позиції (грн)",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )

    # ── Зв'язки ─────────────────────────────────
    return_invoice: Mapped["ReturnInvoice"] = relationship(
        "ReturnInvoice",
        back_populates="items",
    )
    product: Mapped["Product"] = relationship(
        "Product",
        back_populates="return_items",
    )

    def __repr__(self) -> str:
        return f"<ReturnInvoiceItem {self.product_id} x{self.quantity}>"
