"""
Модель системних налаштувань Torgashka POS.
Key-Value зберігання згрупованих за модулями налаштувань.
"""
from __future__ import annotations

import uuid
from datetime import UTC, datetime

from sqlalchemy import Boolean, DateTime, String, Text
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column

from app.database import Base


class SystemSetting(Base):
    """Системне налаштування (key-value)."""

    __tablename__ = "system_settings"

    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор налаштування",
    )
    module: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Модуль: general, pos, printing, pricing, notifications, integrations, security, backup",
    )
    key: Mapped[str] = mapped_column(
        String(100),
        unique=True,
        nullable=False,
        index=True,
        comment="Унікальний ключ налаштування, наприклад 'company_name'",
    )
    value: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Значення налаштування (зберігається як текст)",
    )
    value_type: Mapped[str] = mapped_column(
        String(20),
        nullable=False,
        default="string",
        comment="Тип значення: string, boolean, number, select",
    )
    label: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        comment="Людино-зрозуміла назва налаштування (укр.)",
    )
    description: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Опис налаштування",
    )
    options: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="JSON-список варіантів для select, напр. '[\"1\",\"10\",\"50\"]'",
    )
    is_active: Mapped[bool] = mapped_column(
        Boolean,
        default=True,
        comment="Чи активне налаштування",
    )
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        default=lambda: datetime.now(UTC),
        comment="Дата створення",
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        default=lambda: datetime.now(UTC),
        onupdate=lambda: datetime.now(UTC),
        comment="Дата останнього оновлення",
    )

    def __repr__(self) -> str:
        return f"<SystemSetting {self.module}.{self.key}={self.value}>"
