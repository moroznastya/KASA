"""
Модель User (Користувач системи).

Підтримує авторизацію за логіном + паролем (для admin)
та за PIN-кодом (для cashier на касі).
"""

import uuid
from datetime import datetime
from enum import Enum as PyEnum

from sqlalchemy import String, Boolean, Enum, Text
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID, JSONB

from app.database import Base


class UserRole(str, PyEnum):
    """Ролі користувачів системи."""
    ADMIN = "admin"
    CASHIER = "cashier"


class User(Base):
    """Користувач системи Kasa."""

    __tablename__ = "users"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор користувача",
    )
    name: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        comment="Повне ім'я користувача",
    )
    login: Mapped[str] = mapped_column(
        String(100),
        unique=True,
        nullable=False,
        index=True,
        comment="Логін для входу в систему",
    )
    password_hash: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        comment="Хеш пароля (bcrypt)",
    )
    pin_code: Mapped[str | None] = mapped_column(
        String(255),
        nullable=True,
        comment="Хеш PIN-коду (bcrypt) для швидкої авторизації на касі",
    )
    role: Mapped[UserRole] = mapped_column(
        Enum("admin", "cashier", name="user_role", create_constraint=True),
        default="cashier",
        nullable=False,
        comment="Роль користувача: admin або cashier",
    )
    is_active: Mapped[bool] = mapped_column(
        Boolean,
        default=True,
        comment="Чи активний користувач (може входити в систему)",
    )
    permissions: Mapped[list | None] = mapped_column(
        JSONB,
        nullable=True,
        default=None,
        comment="Список прав доступу (масив рядків-пермішенів). "
                "Якщо None — використовуються права за замовчуванням для ролі.",
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
    receipts: Mapped[list["Receipt"]] = relationship(
        "Receipt",
        back_populates="cashier",
    )

    def __repr__(self) -> str:
        return f"<User {self.login} ({self.role})>"
