"""Unit tests: User Repository."""

from __future__ import annotations

from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.user import User, UserRole


class TestUserRepository:

    @pytest.mark.asyncio
    async def test_save_user(self, user_repo, session):
        user = User(
            id=uuid4(), login="test_user", password_hash="hash",
            name="Тест", role=UserRole.CASHIER, pin_code="1234"
        )
        created = await user_repo.save(user)
        await session.commit()
        assert created.login == "test_user"

    @pytest.mark.asyncio
    async def test_find_by_login(self, user_repo, session):
        user = User(
            id=uuid4(), login="find_me", password_hash="hash",
            name="Знайти", role=UserRole.CASHIER, pin_code="5678"
        )
        await user_repo.save(user)
        await session.commit()
        found = await user_repo.find_by_login("find_me")
        assert found is not None
        assert found.name == "Знайти"
