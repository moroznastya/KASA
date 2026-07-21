"""
Модель Debtor (Боржник).

Зберігає інформацію про клієнтів/боржників, які купують товари в борг.
"""

import uuid
from datetime import datetime

from sqlalchemy import String, Text, Numeric, DateTime
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


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

    def __repr__(self) -> str:
        return f"<Debtor {self.name}>"
