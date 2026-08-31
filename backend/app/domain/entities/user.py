"""
Domain Entity: User (Користувач).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import UTC, datetime
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
    password_hash: str = ""
    """bcrypt-хеш пароля користувача."""
    pin_code: Optional[str] = None
    """bcrypt-хеш PIN-коду (для швидкого входу на касі)."""
    role: UserRole = UserRole.CASHIER
    is_active: bool = True
    email: str = ""
    phone: str = ""
    created_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    last_login_at: Optional[datetime] = None

    @staticmethod
    def generate_login_from_name(name: str) -> str:
        """
        Генерує логін з імені користувача (транслітерація + lower case).

        Args:
            name: Повне ім'я користувача (може містити кирилицю).

        Returns:
            Згенерований логін (транслітерований, lowercase, без спецсимволів).
        """
        translit_map = {
            'а': 'a', 'б': 'b', 'в': 'v', 'г': 'h', 'ґ': 'g',
            'д': 'd', 'е': 'e', 'є': 'ie', 'ж': 'zh', 'з': 'z',
            'и': 'y', 'і': 'i', 'ї': 'i', 'й': 'i', 'к': 'k',
            'л': 'l', 'м': 'm', 'н': 'n', 'о': 'o', 'п': 'p',
            'р': 'r', 'с': 's', 'т': 't', 'у': 'u', 'ф': 'f',
            'х': 'kh', 'ц': 'ts', 'ч': 'ch', 'ш': 'sh', 'щ': 'shch',
            'ю': 'iu', 'я': 'ia',
            'А': 'a', 'Б': 'b', 'В': 'v', 'Г': 'h', 'Ґ': 'g',
            'Д': 'd', 'Е': 'e', 'Є': 'ie', 'Ж': 'zh', 'З': 'z',
            'И': 'y', 'І': 'i', 'Ї': 'i', 'Й': 'i', 'К': 'k',
            'Л': 'l', 'М': 'm', 'Н': 'n', 'О': 'o', 'П': 'p',
            'Р': 'r', 'С': 's', 'Т': 't', 'У': 'u', 'Ф': 'f',
            'Х': 'kh', 'Ц': 'ts', 'Ч': 'ch', 'Ш': 'sh', 'Щ': 'shch',
            'Ю': 'iu', 'Я': 'ia',
        }
        # Transliterate
        result = ''
        for char in name:
            result += translit_map.get(char, char)
        # Replace non-alphanumeric with underscore, lowercase, strip
        result = re.sub(r'[^a-zA-Z0-9]', '_', result).lower().strip('_')
        # Remove consecutive underscores
        result = re.sub(r'_+', '_', result)
        return result

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
        self.last_login_at = datetime.now(UTC)

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
