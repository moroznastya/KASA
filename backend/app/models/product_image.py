"""
Модель ProductImage (Зображення товару).

Один товар може мати декілька зображень.
Одне з них може бути головним (is_main = True).
"""

import uuid
from datetime import datetime

from sqlalchemy import ForeignKey, String, Boolean
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


class ProductImage(Base):
    """Зображення товару."""

    __tablename__ = "product_images"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор зображення",
    )
    product_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("products.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор товару",
    )
    url: Mapped[str] = mapped_column(
        String(1024),
        nullable=False,
        comment="URL або шлях до файлу зображення",
    )
    is_main: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        comment="Чи є це зображення головним (показується в картці товару)",
    )
    sort_order: Mapped[int] = mapped_column(
        default=0,
        comment="Порядок сортування (чим менше — тим вище)",
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
        back_populates="images",
    )

    def __repr__(self) -> str:
        return f"<ProductImage {self.id}>"
