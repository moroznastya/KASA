"""
Модель PrintTemplate (Шаблон чека друку).

Зберігає HTML-шаблони чеків з {{змінними}} для різних типів принтерів.
Обмеження: лише один шаблон може бути is_default=True для кожного type.
"""

from __future__ import annotations

import uuid
from datetime import datetime, timezone

from sqlalchemy import String, Text, Boolean, DateTime, Index, text
from sqlalchemy.orm import Mapped, mapped_column
from sqlalchemy.dialects.postgresql import UUID, JSONB

from app.database import Base


class PrintTemplate(Base):
    """Шаблон чека друку."""

    __tablename__ = "print_templates"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор шаблону",
    )
    name: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        comment="Назва шаблону (наприклад 'Стандартний 58мм')",
    )
    type: Mapped[str] = mapped_column(
        String(20),
        nullable=False,
        index=True,
        comment="Тип шаблону: receipt_58mm, receipt_80mm, fiscal, custom",
    )
    content: Mapped[str] = mapped_column(
        Text,
        nullable=False,
        comment="HTML-вміст шаблону з {{змінними}} для підстановки",
    )
    variables: Mapped[dict | None] = mapped_column(
        JSONB,
        nullable=True,
        default=None,
        comment="JSON з описом змінних: { 'name': 'shop_name', 'label': 'Назва магазину', 'type': 'string' }",
    )
    is_default: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        comment="Чи є шаблоном за замовчуванням для свого type",
    )
    is_active: Mapped[bool] = mapped_column(
        Boolean,
        default=True,
        comment="Чи активний шаблон (може використовуватись для друку)",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        default=lambda: datetime.now(timezone.utc),
        comment="Дата створення",
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        default=lambda: datetime.now(timezone.utc),
        onupdate=lambda: datetime.now(timezone.utc),
        comment="Дата останнього оновлення",
    )

    # ── Індекси та обмеження ────────────────────
    __table_args__ = (
        # Частковий унікальний індекс: тільки один шаблон може бути
        # is_default=True для кожного type
        Index(
            "uq_print_templates_default_per_type",
            "type",
            unique=True,
            postgresql_where=text("is_default = TRUE"),
        ),
    )

    def __repr__(self) -> str:
        return f"<PrintTemplate {self.name!r} ({self.type}) default={self.is_default}>"
