"""Unit tests: User Repository."""

from __future__ import annotations

from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.user import User, UserRole


class TestUserRepository:

    @pytest.mark.asyncio
    async def test_save_user(self, user_repo, session):
        """Створення нового користувача."""
        user = User(
            id=uuid4(),
            login="test_user",
            password_hash="hashed_password",
            name="Тестовий Користувач",
            role=UserRole.CASHIER,
            pin_code="1234",
        )
        created = await user_repo.save(user)
        await session.commit()
        assert created.login == "test_user"
        assert created.name == "Тестовий Користувач"

    @pytest.mark.asyncio
    async def test_find_by_login(self, user_repo, session):
        """Пошук користувача за логіном."""
        user = User(
            id=uuid4(),
            login="find_me",
            password_hash="hash",
            name="Знайти",
            role=UserRole.CASHIER,
            pin_code="5678",
        )
        await user_repo.save(user)
        await session.commit()
        found = await user_repo.find_by_login("find_me")
        assert found is not None
        assert found.name == "Знайти"

    @pytest.mark.asyncio
    async def test_find_by_id(self, user_repo, session):
        """Пошук користувача за ID."""
        user = User(
            id=uuid4(),
            login="by_id",
            password_hash="hash",
            name="За ID",
            role=UserRole.ADMIN,
            pin_code="1111",
        )
        await user_repo.save(user)
        await session.commit()
        found = await user_repo.find_by_id(user.id)
        assert found is not None
        assert found.login == "by_id"

    @pytest.mark.asyncio
    async def test_delete_user(self, user_repo, session):
        """Видалення користувача."""
        user = User(
            id=uuid4(),
            login="delete_me",
            password_hash="hash",
            name="Видалити",
            role=UserRole.CASHIER,
            pin_code="9999",
        )
        await user_repo.save(user)
        await session.commit()

        await user_repo.delete(user.id)
        await session.commit()

        found = await user_repo.find_by_id(user.id)
        assert found is None
