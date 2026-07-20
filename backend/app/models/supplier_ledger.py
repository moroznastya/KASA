"""
Модель SupplierLedger (Журнал взаєморозрахунків з постачальниками).

Фіксує кожну операцію, що змінює баланс постачальника:
  - Надходження товару (прибуткова накладна) — збільшує борг
  - Оплата постачальнику — зменшує борг
  - Повернення товару — зменшує борг

Підтримує часткові оплати (одна накладна може бути оплачена частинами).
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


class LedgerOperationType(str, PyEnum):
    """Тип операції у взаєморозрахунках."""
    INVOICE = "invoice"               # Надходження товару (борг +)
    PAYMENT = "payment"               # Оплата постачальнику (борг -)
    RETURN = "return"                 # Повернення товару (борг -)
    CORRECTION = "correction"         # Коригування боргу


class SupplierLedger(Base):
    """Запис у журналі взаєморозрахунків з постачальником."""

    __tablename__ = "supplier_ledger"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор запису",
    )
    supplier_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("suppliers.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор постачальника",
    )
    operation_type: Mapped[LedgerOperationType] = mapped_column(
        Enum(LedgerOperationType, name="ledger_operation_type", create_constraint=True),
        nullable=False,
        comment="Тип операції",
    )
    document_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        nullable=True,
        comment="Ідентифікатор документа (накладної, платежу тощо)",
    )
    document_number: Mapped[str | None] = mapped_column(
        String(50),
        nullable=True,
        comment="Номер документа",
    )
    amount: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        comment="Сума операції (грн)",
    )
    balance_after: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        comment="Баланс після операції (грн)",
    )
    operation_date: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата операції",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Додаткові нотатки",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення запису",
    )

    # ── Зв'язки ─────────────────────────────────
    supplier: Mapped["Supplier"] = relationship(
        "Supplier",
        back_populates="ledger_entries",
    )

    def __repr__(self) -> str:
        return (
            f"<SupplierLedger {self.supplier_id} "
            f"{self.operation_type.value} {self.amount}>"
        )
