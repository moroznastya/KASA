"""
Domain Entity: User (Користувач).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from typing import Optional
from uuid import UUID, uuid4


class UserRole(Enum):
    """Роль користувача в системі."""
    ADMIN = "admin"
    MANAGER = "manager"
    CASHIER = "cashier"
    WAREHOUSE = "warehouse"
    VIEWER = "viewer"


@dataclass
class User:
    """
    Користувач системи.

    Відповідає за:
    - Ідентифікацію користувача
    - Аутентифікацію (логін, пароль, PIN)
    - Авторизацію (роль, активність)
    """

    id: UUID = field(default_factory=uuid4)
    name: str = ""
    login: str = ""
    role: UserRole = UserRole.CASHIER
    is_active: bool = True
    email: str = ""
    phone: str = ""
    created_at: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    last_login_at: Optional[datetime] = None

    def deactivate(self) -> None:
        """Деактивує користувача."""
        self.is_active = False

    def activate(self) -> None:
        """Активує користувача."""
        self.is_active = True

    def change_role(self, new_role: UserRole) -> None:
        """
        Змінює роль користувача.

        Args:
            new_role: Нова роль.
        """
        self.role = new_role

    def record_login(self) -> None:
        """Фіксує час останнього входу."""
        self.last_login_at = datetime.now(timezone.utc)

    @property
    def is_admin(self) -> bool:
        """Чи є користувач адміністратором."""
        return self.role == UserRole.ADMIN

    @property
    def is_manager(self) -> bool:
        """Чи є користувач менеджером."""
        return self.role == UserRole.MANAGER

    @property
    def is_cashier(self) -> bool:
        """Чи є користувач касиром."""
        return self.role == UserRole.CASHIER

    def can(self, permission: str) -> bool:
        """
        Перевіряє, чи має користувач певний дозвіл.

        Args:
            permission: Назва дозволу.

        Returns:
            True якщо має дозвіл.
        """
        # Базова перевірка ролей
        role_permissions = {
            UserRole.ADMIN: {"all"},
            UserRole.MANAGER: {"read", "write", "confirm", "reports"},
            UserRole.CASHIER: {"read", "sell", "return"},
            UserRole.WAREHOUSE: {"read", "write", "stock"},
            UserRole.VIEWER: {"read", "reports"},
        }
        if not self.is_active:
            return False
        permissions = role_permissions.get(self.role, set())
        return "all" in permissions or permission in permissions

    def __str__(self) -> str:
        return f"User(id={self.id}, login='{self.login}', role={self.role.value})"

    def __repr__(self) -> str:
        return (
            f"User(id={self.id}, name='{self.name}', "
            f"login='{self.login}', role={self.role.value}, "
            f"active={self.is_active})"
        )
