"""
Фабрики для створення тестових користувачів (User).

Використовує factory_boy для генерації даних.
"""

from datetime import datetime
from uuid import uuid4

import factory

from app.domain.services.auth_service import AuthService
from app.infrastructure.persistence.models.user import User, UserRole


class UserFactory(factory.Factory):
    """Фабрика для створення тестових користувачів."""

    class Meta:
        model = User

    id = factory.LazyFunction(uuid4)
    name = factory.Faker("name", locale="uk_UA")
    login = factory.Sequence(lambda n: f"user_{n:03d}")
    password_hash = factory.LazyFunction(
        lambda: AuthService.hash_password("test123")
    )
    pin_code = factory.LazyFunction(
        lambda: AuthService.hash_password("1111")
    )
    role = UserRole.CASHIER
    is_active = True
    created_at = factory.LazyFunction(datetime.utcnow)
    updated_at = factory.LazyFunction(datetime.utcnow)

    @classmethod
    def admin(cls, **kwargs):
        """Створює адміністратора."""
        return cls(role=UserRole.ADMIN, **kwargs)

    @classmethod
    def cashier(cls, **kwargs):
        """Створює касира."""
        return cls(role=UserRole.CASHIER, **kwargs)

    @classmethod
    def inactive(cls, **kwargs):
        """Створює неактивного користувача."""
        return cls(is_active=False, **kwargs)
