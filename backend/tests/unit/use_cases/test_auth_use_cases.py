"""Unit tests: AuthUseCases (app/application/use_cases/auth_use_cases.py).

Покриває:
- login (пароль) — успіх, невідомий користувач, неактивний користувач
- login_by_pin — успіх, невідомий користувач, неактивний користувач
- refresh_token — успіх, не знайдено, неактивний
- get_current_user — успіх, не знайдено
- create_user — успіх, дублікат логіна
- list_users — фільтри та пагінація
"""

from __future__ import annotations

from unittest.mock import AsyncMock
from uuid import uuid4

import pytest

from app.application.dto.user_dto import UserCreateDTO
from app.application.use_cases.auth_use_cases import AuthUseCases
from app.domain.entities.user import User, UserRole


def _make_user(
    *,
    login: str = "admin",
    role: UserRole = UserRole.ADMIN,
    is_active: bool = True,
    password: str = "admin123",
    pin: str = "1234",
) -> User:
    from app.domain.services.auth_service import AuthService

    return User(
        id=uuid4(),
        name="Тестовий користувач",
        login=login,
        role=role,
        is_active=is_active,
        password_hash=AuthService.hash_password(password),
        pin_code=AuthService.hash_password(pin),
    )


def _build_use_cases(
    *,
    user_repo: AsyncMock | None = None,
    uow: AsyncMock | None = None,
    event_bus: AsyncMock | None = None,
) -> AuthUseCases:
    return AuthUseCases(
        user_repo=user_repo or AsyncMock(),
        unit_of_work=uow or AsyncMock(),
        event_bus=event_bus or AsyncMock(),
    )


class TestLogin:
    """Тести входу за паролем."""

    @pytest.mark.asyncio
    async def test_login_success(self):
        """Успішний вхід за паролем."""
        user = _make_user()
        repo = AsyncMock()
        repo.find_by_login.return_value = user
        uow = AsyncMock()
        event_bus = AsyncMock()

        uc = _build_use_cases(user_repo=repo, uow=uow, event_bus=event_bus)
        dto, token = await uc.login(login="admin", password="admin123")

        assert dto.login == "admin"
        assert isinstance(token, str) and len(token) > 20
        assert user.last_login_at is not None
        repo.update.assert_awaited_once_with(user)
        uow.commit.assert_awaited_once()
        event_bus.publish.assert_awaited_once()
        published = event_bus.publish.call_args.args[0]
        assert published.login_method == "password"
        assert published.user_id == user.id

    @pytest.mark.asyncio
    async def test_login_unknown_user_raises(self):
        """Помилка при невідомому логіні."""
        repo = AsyncMock()
        repo.find_by_login.return_value = None

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="пароль"):
            await uc.login(login="nobody", password="x")
        repo.update.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_login_inactive_user_raises(self):
        """Помилка при деактивованому користувачі."""
        repo = AsyncMock()
        repo.find_by_login.return_value = _make_user(is_active=False)

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="деактивований"):
            await uc.login(login="admin", password="admin123")


class TestLoginByPin:
    """Тести входу за PIN-кодом."""

    @pytest.mark.asyncio
    async def test_login_by_pin_success(self):
        """Успішний вхід за PIN-кодом."""
        user = _make_user(role=UserRole.CASHIER)
        repo = AsyncMock()
        repo.find_by_login.return_value = user
        uow = AsyncMock()
        event_bus = AsyncMock()

        uc = _build_use_cases(user_repo=repo, uow=uow, event_bus=event_bus)
        dto, token = await uc.login_by_pin(login="cashier", pin_code="1234")

        assert dto.role == "cashier"
        assert isinstance(token, str) and len(token) > 20
        uow.commit.assert_awaited_once()
        published = event_bus.publish.call_args.args[0]
        assert published.login_method == "pin"

    @pytest.mark.asyncio
    async def test_login_by_pin_unknown_user_raises(self):
        """Помилка при невідомому логіні."""
        repo = AsyncMock()
        repo.find_by_login.return_value = None

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="PIN"):
            await uc.login_by_pin(login="nobody", pin_code="1234")

    @pytest.mark.asyncio
    async def test_login_by_pin_inactive_user_raises(self):
        """Помилка при деактивованому користувачі."""
        repo = AsyncMock()
        repo.find_by_login.return_value = _make_user(is_active=False)

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="деактивований"):
            await uc.login_by_pin(login="admin", pin_code="1234")


class TestRefreshToken:
    """Тести оновлення токена."""

    @pytest.mark.asyncio
    async def test_refresh_token_success(self):
        """Успішне оновлення токена."""
        user = _make_user()
        repo = AsyncMock()
        repo.find_by_id.return_value = user

        uc = _build_use_cases(user_repo=repo)
        dto, token = await uc.refresh_token(user.id)

        assert dto.id == user.id
        assert isinstance(token, str) and len(token) > 20
        repo.find_by_id.assert_awaited_once_with(user.id)

    @pytest.mark.asyncio
    async def test_refresh_token_user_not_found_raises(self):
        """Помилка якщо користувача не знайдено."""
        repo = AsyncMock()
        repo.find_by_id.return_value = None

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="Користувача не знайдено"):
            await uc.refresh_token(uuid4())

    @pytest.mark.asyncio
    async def test_refresh_token_inactive_user_raises(self):
        """Помилка для деактивованого користувача."""
        repo = AsyncMock()
        repo.find_by_id.return_value = _make_user(is_active=False)

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="деактивований"):
            await uc.refresh_token(uuid4())


class TestGetCurrentUser:
    """Тести отримання поточного користувача."""

    @pytest.mark.asyncio
    async def test_get_current_user_success(self):
        """Успішне отримання користувача за ID."""
        user = _make_user()
        repo = AsyncMock()
        repo.find_by_id.return_value = user

        uc = _build_use_cases(user_repo=repo)
        dto = await uc.get_current_user(user.id)

        assert dto.id == user.id
        assert dto.name == "Тестовий користувач"

    @pytest.mark.asyncio
    async def test_get_current_user_not_found_raises(self):
        """Помилка якщо користувача не знайдено."""
        repo = AsyncMock()
        repo.find_by_id.return_value = None

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="не знайдено"):
            await uc.get_current_user(uuid4())


class TestCreateUser:
    """Тести створення користувача."""

    @pytest.mark.asyncio
    async def test_create_user_success(self):
        """Успішне створення користувача."""
        saved_user = _make_user(login="newuser", role=UserRole.CASHIER)
        repo = AsyncMock()
        repo.exists_by_login.return_value = False
        repo.save.return_value = saved_user
        uow = AsyncMock()
        event_bus = AsyncMock()

        uc = _build_use_cases(user_repo=repo, uow=uow, event_bus=event_bus)
        dto = await uc.create_user(
            UserCreateDTO(
                name="Новий",
                login="newuser",
                password="pass123",
                role="cashier",
            )
        )

        assert dto.login == "newuser"
        assert dto.role == "cashier"
        repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()
        published = event_bus.publish.call_args.args[0]
        assert published.login == "newuser"
        assert published.role == "cashier"

    @pytest.mark.asyncio
    async def test_create_user_duplicate_login_raises(self):
        """Помилка при дублюванні логіна."""
        repo = AsyncMock()
        repo.exists_by_login.return_value = True

        uc = _build_use_cases(user_repo=repo)
        with pytest.raises(ValueError, match="вже існує"):
            await uc.create_user(
                UserCreateDTO(name="Новий", login="admin", password="pass")
            )
        repo.save.assert_not_awaited()


class TestListUsers:
    """Тести списку користувачів."""

    @pytest.mark.asyncio
    async def test_list_users_with_filters(self):
        """Список з фільтрами та пагінацією."""
        users = [_make_user(), _make_user(login="cashier", role=UserRole.CASHIER)]
        repo = AsyncMock()
        repo.search.return_value = (users, 2)

        uc = _build_use_cases(user_repo=repo)
        dtos, total = await uc.list_users(
            query="тест",
            role="admin",
            is_active=True,
            page=2,
            size=10,
        )

        assert total == 2
        assert len(dtos) == 2
        assert all(d.role in ("admin", "cashier") for d in dtos)
        repo.search.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_list_users_no_filters(self):
        """Список без фільтрів (role=None)."""
        repo = AsyncMock()
        repo.search.return_value = ([], 0)

        uc = _build_use_cases(user_repo=repo)
        dtos, total = await uc.list_users()

        assert total == 0
        assert dtos == []
        # роль не передається — отже search викликається з role=None
        assert repo.search.call_args.kwargs["role"] is None
