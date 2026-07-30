"""
Repository Implementation: SQLAlchemyUserRepository.

Реалізація IUserRepository з використанням SQLAlchemy.
"""

from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.repositories import IUserRepository
from app.infrastructure.persistence.models.user import User, UserRole


class SQLAlchemyUserRepository(IUserRepository):
    """
    SQLAlchemy реалізація репозиторію користувачів.

    Працює з моделлю User.
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, user: User) -> User:
        """Зберігає нового користувача."""
        self._session.add(user)
        await self._session.flush()
        return user

    async def update(self, user: User) -> User:
        """Оновлює існуючого користувача."""
        merged = await self._session.merge(user)
        await self._session.flush()
        return merged

    async def find_by_id(self, user_id: UUID) -> Optional[User]:
        """Знаходить користувача за ID."""
        stmt = select(User).where(User.id == user_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_login(self, login: str) -> Optional[User]:
        """Знаходить користувача за логіном."""
        stmt = select(User).where(User.login == login)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_role(self, role: UserRole) -> list[User]:
        """Знаходить всіх користувачів з роллю."""
        stmt = select(User).where(User.role == role).order_by(User.name)
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def search(
        self,
        query: Optional[str] = None,
        role: Optional[UserRole] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[User], int]:
        """Пошук користувачів з фільтрацією та пагінацією."""
        base_stmt = select(User)

        if query:
            like_pattern = f"%{query}%"
            base_stmt = base_stmt.where(
                or_(
                    User.name.ilike(like_pattern),
                    User.login.ilike(like_pattern),
                )
            )
        if role is not None:
            base_stmt = base_stmt.where(User.role == role)
        if is_active is not None:
            base_stmt = base_stmt.where(User.is_active == is_active)

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = base_stmt.offset(offset).limit(size).order_by(User.name)
        result = await self._session.execute(stmt)
        users = list(result.scalars().all())

        return users, total

    async def delete(self, user_id: UUID) -> None:
        """Видаляє користувача за ID."""
        user = await self.find_by_id(user_id)
        if user is not None:
            await self._session.delete(user)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість користувачів."""
        stmt = select(func.count()).select_from(User)
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def exists_by_login(
        self, login: str, exclude_id: Optional[UUID] = None
    ) -> bool:
        """Перевіряє, чи існує користувач з таким логіном."""
        stmt = select(User).where(User.login == login)
        if exclude_id is not None:
            stmt = stmt.where(User.id != exclude_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none() is not None
