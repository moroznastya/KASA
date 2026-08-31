"""
Модель Supplier (Постачальник).

Зберігає інформацію про постачальників товарів.
"""

import uuid
from datetime import datetime
from typing import TYPE_CHECKING

from sqlalchemy import String, Text
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.invoice import Invoice
    from app.infrastructure.persistence.models.product import Product
    from app.infrastructure.persistence.models.supplier_ledger import SupplierLedger


class Supplier(Base):
    """Постачальник товарів."""

    __tablename__ = "suppliers"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор постачальника",
    )
    name: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        index=True,
        comment="Назва постачальника (юр. особа або ФОП)",
    )
    edrpou: Mapped[str | None] = mapped_column(
        String(10),
        nullable=True,
        comment="Код ЄДРПОУ / ІПН",
    )
    phone: Mapped[str | None] = mapped_column(
        String(20),
        nullable=True,
        comment="Номер телефону",
    )
    email: Mapped[str | None] = mapped_column(
        String(255),
        nullable=True,
        comment="Електронна пошта",
    )
    address: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Юридична / фактична адреса",
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
    products: Mapped[list["Product"]] = relationship(
        "Product",
        back_populates="supplier",
    )
    invoices: Mapped[list["Invoice"]] = relationship(
        "Invoice",
        back_populates="supplier",
    )
    ledger_entries: Mapped[list["SupplierLedger"]] = relationship(
        "SupplierLedger",
        back_populates="supplier",
    )

    def __repr__(self) -> str:
        return f"<Supplier {self.name}>"
