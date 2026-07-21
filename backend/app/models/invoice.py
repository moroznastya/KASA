"""
Моделі Invoice та InvoiceItem (Прибуткова накладна).

Документ, який фіксує надходження товару від постачальника.
Підтримує часткові оплати через SupplierLedger.
"""

import uuid
from datetime import datetime
from enum import Enum as PyEnum

from sqlalchemy import (
    ForeignKey, String, Text, Numeric, Enum, DateTime, Boolean,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


class InvoiceStatus(str, PyEnum):
    """Статус прибуткової накладної."""
    DRAFT = "draft"           # Чернетка
    CONFIRMED = "confirmed"   # Підтверджено (товар оприбутковано)
    CANCELLED = "cancelled"   # Скасовано


class PaymentMethod(str, PyEnum):
    """Спосіб оплати постачальнику."""
    CREDIT = "credit"               # В борг постачальнику
    BANK_TRANSFER = "bank_transfer" # По перерахунку
    CASH = "cash"                   # Готівкою з каси
    OTHER = "other"                 # Інший спосіб


class Invoice(Base):
    """Прибуткова накладна (надходження товару від постачальника)."""

    __tablename__ = "invoices"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор накладної",
    )
    number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер накладної (внутрішній або від постачальника)",
    )
    supplier_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("suppliers.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор постачальника",
    )
    invoice_date: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата накладної (від постачальника)",
    )
    status: Mapped[InvoiceStatus] = mapped_column(
        Enum(InvoiceStatus, name="invoice_status", create_constraint=True, values_callable=lambda x: [e.value for e in x]),
        default=InvoiceStatus.DRAFT,
        nullable=False,
        comment="Статус накладної",
    )
    payment_method: Mapped[PaymentMethod | None] = mapped_column(
        Enum(PaymentMethod, name="payment_method", create_constraint=True, values_callable=lambda x: [e.value for e in x]),
        nullable=True,
        comment="Спосіб оплати з постачальником",
    )
    is_fiscal: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        nullable=False,
        comment="Фіскальна накладна (проведена через РРО)",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Додаткові нотатки до накладної",
    )
    total_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        default=0.00,
        comment="Загальна сума накладної (грн)",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення запису",
    )
    updated_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        onupdate=datetime.utcnow,
        comment="Дата останнього оновлення",
    )

    # ── Зв'язки ─────────────────────────────────
    supplier: Mapped["Supplier"] = relationship(
        "Supplier",
        back_populates="invoices",
    )
    items: Mapped[list["InvoiceItem"]] = relationship(
        "InvoiceItem",
        back_populates="invoice",
        cascade="all, delete-orphan",
    )

    def __repr__(self) -> str:
        return f"<Invoice {self.number}>"


class InvoiceItem(Base):
    """Позиція прибуткової накладної (один товар)."""

    __tablename__ = "invoice_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор позиції",
    )
    invoice_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("invoices.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор накладної",
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
    invoice: Mapped["Invoice"] = relationship(
        "Invoice",
        back_populates="items",
    )
    product: Mapped["Product"] = relationship(
        "Product",
        back_populates="invoice_items",
    )

    def __repr__(self) -> str:
        return f"<InvoiceItem {self.product_id} x{self.quantity}>"
