"""
Модель Barcode (Штрих-код товару).

Окрема таблиця для підтримки декількох штрих-кодів на один товар.
Це необхідно для:
  - Різних упаковок (штука / коробка)
  - Змінених штрих-кодів виробником
  - Дублювання старих штрих-кодів
"""

import uuid
from datetime import datetime

from sqlalchemy import ForeignKey, String, Boolean
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


class Barcode(Base):
    """Штрих-код товару (один товар може мати багато штрих-кодів)."""

    __tablename__ = "barcodes"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор запису штрих-коду",
    )
    product_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("products.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор товару",
    )
    barcode: Mapped[str] = mapped_column(
        String(50),
        unique=True,
        nullable=False,
        index=True,
        comment="Штрих-код (EAN-13, UPC, Code128 тощо)",
    )
    is_primary: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        comment="Чи є цей штрих-код основним (для пошуку за замовчуванням)",
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
    product: Mapped["Product"] = relationship(
        "Product",
        back_populates="barcodes",
    )

    def __repr__(self) -> str:
        return f"<Barcode {self.barcode}>"
