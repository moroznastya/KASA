"""
Модель Debtor (Боржник).

Зберігає інформацію про клієнтів/боржників, які купують товари в борг.
"""

import uuid
from datetime import datetime
from typing import TYPE_CHECKING

from sqlalchemy import ForeignKey, Numeric, String, Text
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.receipt import Receipt


class Debtor(Base):
    """Боржник (клієнт, який купує в борг)."""

    __tablename__ = "debtors"

    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор боржника",
    )
    name: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        index=True,
        comment="Повне ім'я боржника",
    )
    phone: Mapped[str | None] = mapped_column(
        String(50),
        nullable=True,
        comment="Номер телефону",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Додаткові нотатки",
    )
    total_debt: Mapped[float] = mapped_column(
        Numeric(12, 2),
        default=0,
        comment="Загальна сума боргу",
    )
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
    receipts: Mapped[list["Receipt"]] = relationship(
        "Receipt",
        back_populates="debtor",
    )
    payments: Mapped[list["DebtorPayment"]] = relationship(
        "DebtorPayment",
        back_populates="debtor",
        cascade="all, delete-orphan",
        order_by="DebtorPayment.created_at.desc()",
    )

    def __repr__(self) -> str:
        return f"<Debtor {self.name}>"


class DebtorPayment(Base):
    """Історія оплат боргу."""

    __tablename__ = "debtor_payments"

    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор оплати",
    )
    debtor_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("debtors.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="ID боржника",
    )
    amount: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        comment="Сума оплати",
    )
    payment_method: Mapped[str | None] = mapped_column(
        String(20),
        nullable=True,
        comment="Спосіб оплати: cash, card, transfer, mixed",
    )
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата оплати",
    )

    # ── Зв'язки ─────────────────────────────────
    debtor: Mapped["Debtor"] = relationship("Debtor", back_populates="payments")

    def __repr__(self) -> str:
        return f"<DebtorPayment {self.amount} for {self.debtor_id}>"
