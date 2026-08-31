"""
Моделі PurchaseOrder та PurchaseOrderItem (Замовлення постачальнику).

Документ, який фіксує замовлення товарів у постачальника.
При підтвердженні автоматично створюється прибуткова накладна (Invoice),
яка оприбутковує товари на склад.

Статуси:
  - draft      — чернетка (замовлення створено, але ще не відправлено)
  - confirmed  — підтверджено (товари замовлено, створено прибуткову накладну)
  - cancelled  — скасовано (замовлення не актуальне)
"""

import uuid
from datetime import datetime
from enum import StrEnum
from typing import TYPE_CHECKING

from sqlalchemy import (
    Boolean,
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
    from app.infrastructure.persistence.models.invoice import Invoice
    from app.infrastructure.persistence.models.product import Product
    from app.infrastructure.persistence.models.supplier import Supplier
    from app.infrastructure.persistence.models.user import User


class PurchaseOrderStatus(StrEnum):
    """Статус замовлення постачальнику."""
    DRAFT = "draft"           # Чернетка
    CONFIRMED = "confirmed"   # Підтверджено (створено прибуткову накладну)
    CANCELLED = "cancelled"   # Скасовано


class PurchaseOrder(Base):
    """Замовлення постачальнику."""

    __tablename__ = "purchase_orders"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор замовлення",
    )
    number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер замовлення",
    )
    supplier_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("suppliers.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор постачальника",
    )
    order_date: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата замовлення",
    )
    expected_date: Mapped[datetime | None] = mapped_column(
        DateTime,
        nullable=True,
        comment="Очікувана дата поставки",
    )
    status: Mapped[PurchaseOrderStatus] = mapped_column(
        Enum(PurchaseOrderStatus, name="purchase_order_status", create_constraint=True, values_callable=lambda x: [e.value for e in x]),
        default=PurchaseOrderStatus.DRAFT,
        nullable=False,
        comment="Статус замовлення",
    )
    is_fiscal: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        nullable=False,
        comment="Фіскальний документ",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Додаткові нотатки до замовлення",
    )
    total_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        default=0.00,
        comment="Загальна сума замовлення (грн)",
    )

    # ── Користувач, який створив ─────────────────
    created_by_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("users.id", ondelete="RESTRICT"),
        nullable=False,
        comment="Ідентифікатор користувача, який створив замовлення",
    )
    creator: Mapped["User"] = relationship(
        "User",
        foreign_keys=[created_by_id],
    )

    # ── Зв'язок з прибутковою накладною ──
    invoice_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("invoices.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="ID прибуткової накладної, створеної при підтвердженні замовлення",
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
    items: Mapped[list["PurchaseOrderItem"]] = relationship(
        "PurchaseOrderItem",
        back_populates="purchase_order",
        cascade="all, delete-orphan",
    )
    invoice: Mapped["Invoice | None"] = relationship(
        "Invoice",
        foreign_keys=[invoice_id],
        post_update=True,
    )

    def __repr__(self) -> str:
        return f"<PurchaseOrder {self.number}>"


class PurchaseOrderItem(Base):
    """Позиція замовлення постачальнику (один товар)."""

    __tablename__ = "purchase_order_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор позиції",
    )
    purchase_order_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("purchase_orders.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор замовлення",
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
        comment="Замовлена кількість",
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
    purchase_order: Mapped["PurchaseOrder"] = relationship(
        "PurchaseOrder",
        back_populates="items",
    )
    product: Mapped["Product"] = relationship(
        "Product",
    )

    def __repr__(self) -> str:
        return f"<PurchaseOrderItem {self.product_id} x{self.quantity}>"
