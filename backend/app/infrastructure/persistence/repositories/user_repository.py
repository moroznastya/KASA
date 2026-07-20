"""
Infrastructure Layer: UserRepository — реалізація IUserRepository.

Використовує SQLAlchemy ORM модель UserModel для роботи з БД.
"""

from __future__ import annotations

import logging
from typing import Optional
from uuid import UUID

from sqlalchemy import select, func, or_, and_
from sqlalchemy.ext.asyncio import AsyncSession

from app.domain.entities.user import User, UserRole
from app.domain.repositories.i_user_repository import IUserRepository
from app.infrastructure.persistence.models import UserModel

logger = logging.getLogger(__name__)


class UserRepository(IUserRepository):
    """
    Репозиторій користувачів.

    Реалізує IUserRepository використовуючи SQLAlchemy ORM.
    """

    def __init__(self) -> None:
        self._session: AsyncSession | None = None

    @property
    def session(self) -> AsyncSession:
        if self._session is None:
            raise RuntimeError("Session not set.")
        return self._session

    def set_session(self, session: AsyncSession) -> None:
        self._session = session

    async def save(self, user: User) -> User:
        model = self._to_model(user)
        self.session.add(model)
        await self.session.flush()
        return self._to_domain(model)

    async def update(self, user: User) -> User:
        model = await self._get_model(user.id)
        if model is None:
            raise ValueError(f"User with id {user.id} not found")
        self._update_model(model, user)
        await self.session.flush()
        return self._to_domain(model)

    async def find_by_id(self, user_id: UUID) -> Optional[User]:
        model = await self._get_model(user_id)
        return self._to_domain(model) if model else None

    async def find_by_login(self, login: str) -> Optional[User]:
        result = await self.session.execute(
            select(UserModel).where(UserModel.login == login)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    async def find_by_role(self, role: UserRole) -> list[User]:
        result = await self.session.execute(
            select(UserModel).where(UserModel.role == role.value)
        )
        return [self._to_domain(m) for m in result.scalars().all()]

    async def search(
        self,
        query: Optional[str] = None,
        role: Optional[UserRole] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[User], int]:
        stmt = select(UserModel)
        count_stmt = select(func.count(UserModel.id))

        conditions = []
        if query:
            like_pattern = f"%{query}%"
            conditions.append(
                or_(
                    UserModel.full_name.ilike(like_pattern),
                    UserModel.login.ilike(like_pattern),
                )
            )
        if role:
            conditions.append(UserModel.role == role.value)
        if is_active is not None:
            conditions.append(UserModel.is_active == is_active)

        if conditions:
            stmt = stmt.where(and_(*conditions))
            count_stmt = count_stmt.where(and_(*conditions))

        total_result = await self.session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = stmt.offset(offset).limit(size).order_by(UserModel.full_name)

        result = await self.session.execute(stmt)
        models = result.scalars().all()

        return [self._to_domain(m) for m in models], total

    async def delete(self, user_id: UUID) -> None:
        model = await self._get_model(user_id)
        if model:
            await self.session.delete(model)
            await self.session.flush()

    async def count(self) -> int:
        result = await self.session.execute(
            select(func.count(UserModel.id))
        )
        return result.scalar() or 0

    async def exists_by_login(
        self,
        login: str,
        exclude_id: Optional[UUID] = None,
    ) -> bool:
        stmt = select(UserModel).where(UserModel.login == login)
        if exclude_id:
            stmt = stmt.where(UserModel.id != exclude_id)
        result = await self.session.execute(stmt)
        return result.scalar_one_or_none() is not None

    # ─── Маппінг ────────────────────────────────────────────────────────────

    def _to_domain(self, model: UserModel | None) -> User | None:
        if model is None:
            return None
        return User(
            id=model.id,
            login=model.login,
            full_name=model.full_name or "",
            role=UserRole(model.role) if model.role else UserRole.CASHIER,
            is_active=model.is_active,
        )

    def _to_model(self, domain: User) -> UserModel:
        return UserModel(
            id=domain.id,
            login=domain.login,
            full_name=domain.full_name,
            role=domain.role.value if domain.role else "cashier",
            is_active=domain.is_active,
        )

    def _update_model(self, model: UserModel, domain: User) -> None:
        model.login = domain.login
        model.full_name = domain.full_name
        model.role = domain.role.value if domain.role else "cashier"
        model.is_active = domain.is_active

    async def _get_model(self, user_id: UUID) -> Optional[UserModel]:
        result = await self.session.execute(
            select(UserModel).where(UserModel.id == user_id)
        )
        return result.scalar_one_or_none()
