"""
Repository Interface: IUserRepository.

Визначає контракт для роботи з користувачами.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from typing import Optional, Protocol
from uuid import UUID

from ..entities.user import User, UserRole


class IUserRepository(Protocol):
    """
    Інтерфейс репозиторію користувачів.
    """

    async def save(self, user: User) -> User:
        """
        Зберігає нового користувача.

        Args:
            user: Entity користувача.

        Returns:
            Збережений користувач.
        """
        ...

    async def update(self, user: User) -> User:
        """
        Оновлює існуючого користувача.

        Args:
            user: Entity користувача з оновленими даними.

        Returns:
            Оновлений користувач.
        """
        ...

    async def find_by_id(self, user_id: UUID) -> Optional[User]:
        """
        Знаходить користувача за ID.

        Args:
            user_id: UUID користувача.

        Returns:
            User або None.
        """
        ...

    async def find_by_login(self, login: str) -> Optional[User]:
        """
        Знаходить користувача за логіном.

        Args:
            login: Логін користувача.

        Returns:
            User або None.
        """
        ...

    async def find_by_role(self, role: UserRole) -> list[User]:
        """
        Знаходить всіх користувачів з роллю.

        Args:
            role: Роль користувача.

        Returns:
            Список користувачів.
        """
        ...

    async def search(
        self,
        query: Optional[str] = None,
        role: Optional[UserRole] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[User], int]:
        """
        Пошук користувачів.

        Args:
            query: Текстовий пошук (ім'я, логін).
            role: Фільтр за роллю.
            is_active: Фільтр за активністю.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список користувачів, загальна кількість).
        """
        ...

    async def delete(self, user_id: UUID) -> None:
        """
        Видаляє користувача за ID.

        Args:
            user_id: UUID користувача.
        """
        ...

    async def count(self) -> int:
        """
        Повертає загальну кількість користувачів.

        Returns:
            Кількість користувачів.
        """
        ...

    async def exists_by_login(self, login: str, exclude_id: Optional[UUID] = None) -> bool:
        """
        Перевіряє, чи існує користувач з таким логіном.

        Args:
            login: Логін для перевірки.
            exclude_id: ID для виключення.

        Returns:
            True якщо існує.
        """
        ...
