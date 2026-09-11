"""
Модель User (Користувач системи).

Підтримує авторизацію за логіном + паролем (для admin)
та за PIN-кодом (для cashier на касі).
"""

import uuid
from datetime import datetime
from enum import StrEnum
from typing import TYPE_CHECKING

from sqlalchemy import JSON, Boolean, Enum, Numeric, String
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.receipt import Receipt
    from app.infrastructure.persistence.models.work_session import WorkSession


class UserRole(StrEnum):
    """Ролі користувачів системи."""
    ADMIN = "admin"
    CASHIER = "cashier"


class User(Base):
    """Користувач системи Torgashka."""

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
        JSON,
        nullable=True,
        default=None,
        comment="Список прав доступу (масив рядків-пермішенів). "
                "Якщо None — використовуються права за замовчуванням для ролі.",
    )
    hourly_rate: Mapped[float | None] = mapped_column(
        Numeric(10, 2),
        nullable=True,
        default=None,
        comment="Ставка за годину роботи (грн). Використовується для розрахунку зарплати",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )
    last_login_at: Mapped[datetime | None] = mapped_column(
        nullable=True,
        default=None,
        comment="Дата/час останнього входу (оновлюється при логіні)",
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
    work_sessions: Mapped[list["WorkSession"]] = relationship(
        "WorkSession",
        back_populates="user",
    )

    # ── Domain-сумісні методи (дублюють API доменної сутності User) ──────
    # Репозиторій повертає ORM-модель, а use cases очікують domain entity.
    # Щоб не мапити ORM→domain у кожному виклику, ORM-модель надає ті самі методи.

    def record_login(self) -> None:
        """Фіксує час останнього входу (UTC, naive — як інші timestamps)."""
        self.last_login_at = datetime.utcnow()

    def deactivate(self) -> None:
        """Деактивує користувача."""
        self.is_active = False

    def activate(self) -> None:
        """Активує користувача."""
        self.is_active = True

    def change_role(self, new_role) -> None:
        """Змінює роль користувача."""
        self.role = new_role

    @property
    def is_admin(self) -> bool:
        """Чи є користувач адміністратором."""
        return self.role == UserRole.ADMIN or self.role == "admin"

    @property
    def is_manager(self) -> bool:
        """Чи є користувач менеджером."""
        return self.is_admin or self.role in (UserRole.CASHIER, "cashier")

    @property
    def is_cashier(self) -> bool:
        """Чи є користувач касиром."""
        return self.role == UserRole.CASHIER or self.role == "cashier"

    def can(self, permission: str) -> bool:
        """Перевіряє, чи має користувач певний дозвіл."""
        if not self.is_active:
            return False
        role = getattr(self.role, "value", self.role)
        if role == "admin":
            return True
        allowed = {"cashier": {"read", "sell", "return"}, "admin": {"all"}}
        return permission in allowed.get(role, set())

    def __repr__(self) -> str:
        return f"<User {self.login} ({self.role})>"
