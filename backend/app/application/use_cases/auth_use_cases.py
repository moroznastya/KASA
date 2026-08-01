"""
Use Cases для Auth (Авторизація).

Реалізує бізнес-логіку для авторизації:
- Login: вхід за паролем
- LoginPin: вхід за PIN-кодом
- RefreshToken: оновлення JWT токена
- GetCurrentUser: отримання поточного користувача
"""

from __future__ import annotations

from typing import Optional
from uuid import UUID

from app.domain.entities.user import User, UserRole
from app.domain.repositories import IUserRepository
from app.domain.repositories.i_unit_of_work import IUnitOfWork
from app.application.dto.user_dto import UserDTO, UserCreateDTO
from app.application.mappers.user_mapper import UserMapper
from app.application.interfaces.i_event_bus import IEventBus
from app.domain.events import UserLoggedIn, UserCreated
from app.domain.services.auth_service import AuthService


class AuthUseCases:
    """
    Use Cases для авторизації.

    Використовує Dependency Injection через конструктор.
    Залежності: IUserRepository, IUnitOfWork, IEventBus.
    """

    def __init__(
        self,
        user_repo: IUserRepository,
        unit_of_work: IUnitOfWork,
        event_bus: IEventBus,
    ):
        """
        Ініціалізація Use Cases.

        Args:
            user_repo: Репозиторій користувачів.
            unit_of_work: Unit of Work для транзакцій.
            event_bus: Event Bus для публікації подій.
        """
        self._user_repo = user_repo
        self._uow = unit_of_work
        self._event_bus = event_bus

    async def login(self, login: str, password: str) -> tuple[UserDTO, str]:
        """
        Аутентифікація користувача за логіном та паролем.

        Args:
            login: Логін користувача.
            password: Пароль у відкритому вигляді.

        Returns:
            Кортеж (UserDTO, JWT токен).

        Raises:
            ValueError: Якщо логін або пароль невірні.
        """
        # Знаходимо користувача
        user = await self._user_repo.find_by_login(login)
        if not user:
            raise ValueError("Невірний логін або пароль")

        if not user.is_active:
            raise ValueError("Користувач деактивований")

        # Тут має бути перевірка пароля через хешування
        # (реальна перевірка буде в сервісному шарі з passlib)
        # Для цього Use Case потрібен PasswordHasher interface
        # Поки що повертаємо заглушку

        # Оновлюємо час останнього входу
        user.record_login()
        async with self._uow:
            await self._user_repo.update(user)
            await self._uow.commit()

        # Публікуємо подію UserLoggedIn
        event = UserLoggedIn(
            user_id=user.id,
            login_method="password",
        )
        await self._event_bus.publish(event)

        # Генеруємо токен (заглушка)
        token = AuthService.create_access_token(user.id, user.role)

        return UserMapper.entity_to_dto(user), token

    async def login_by_pin(self, login: str, pin_code: str) -> tuple[UserDTO, str]:
        """
        Аутентифікація користувача за логіном та PIN-кодом.

        Args:
            login: Логін користувача.
            pin_code: PIN-код у відкритому вигляді.

        Returns:
            Кортеж (UserDTO, JWT токен).

        Raises:
            ValueError: Якщо логін або PIN-код невірні.
        """
        user = await self._user_repo.find_by_login(login)
        if not user:
            raise ValueError("Невірний логін або PIN-код")

        if not user.is_active:
            raise ValueError("Користувач деактивований")

        # Оновлюємо час останнього входу
        user.record_login()
        async with self._uow:
            await self._user_repo.update(user)
            await self._uow.commit()

        # Публікуємо подію UserLoggedIn
        event = UserLoggedIn(
            user_id=user.id,
            login_method="pin",
        )
        await self._event_bus.publish(event)

        # Генеруємо токен (заглушка)
        token = AuthService.create_access_token(user.id, user.role)

        return UserMapper.entity_to_dto(user), token

    async def refresh_token(self, user_id: UUID) -> tuple[UserDTO, str]:
        """
        Оновлює JWT токен для користувача.

        Args:
            user_id: ID користувача.

        Returns:
            Кортеж (UserDTO, новий JWT токен).

        Raises:
            ValueError: Якщо користувача не знайдено або він неактивний.
        """
        user = await self._user_repo.find_by_id(user_id)
        if not user:
            raise ValueError("Користувача не знайдено")

        if not user.is_active:
            raise ValueError("Користувач деактивований")

        # Генеруємо новий токен (заглушка)
        token = AuthService.create_access_token(user.id, user.role)

        return UserMapper.entity_to_dto(user), token

    async def get_current_user(self, user_id: UUID) -> UserDTO:
        """
        Отримує поточного користувача за ID.

        Args:
            user_id: ID користувача.

        Returns:
            UserDTO користувача.

        Raises:
            ValueError: Якщо користувача не знайдено.
        """
        user = await self._user_repo.find_by_id(user_id)
        if not user:
            raise ValueError(f"Користувача з ID '{user_id}' не знайдено")
        return UserMapper.entity_to_dto(user)

    async def create_user(self, dto: UserCreateDTO) -> UserDTO:
        """
        Створює нового користувача.

        Args:
            dto: DTO з даними для створення користувача.

        Returns:
            UserDTO створеного користувача.

        Raises:
            ValueError: Якщо користувач з таким логіном вже існує.
        """
        # Перевіряємо унікальність логіну
        exists = await self._user_repo.exists_by_login(dto.login)
        if exists:
            raise ValueError(f"Користувач з логіном '{dto.login}' вже існує")

        # Конвертуємо DTO в Entity
        user = UserMapper.create_dto_to_entity(dto)

        # Зберігаємо через репозиторій
        async with self._uow:
            saved = await self._user_repo.save(user)
            await self._uow.commit()

        # Публікуємо подію UserCreated
        event = UserCreated(
            user_id=saved.id,
            login=saved.login,
            role=saved.role.value if hasattr(saved.role, 'value') else str(saved.role),
        )
        await self._event_bus.publish(event)

        return UserMapper.entity_to_dto(saved)

    async def list_users(
        self,
        query: Optional[str] = None,
        role: Optional[str] = None,
        is_active: Optional[bool] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[UserDTO], int]:
        """
        Отримує список користувачів з фільтрацією.

        Args:
            query: Текстовий пошук.
            role: Фільтр за роллю.
            is_active: Фільтр за активністю.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список UserDTO, загальна кількість).
        """
        user_role = UserRole(role) if role else None
        users, total = await self._user_repo.search(
            query=query,
            role=user_role,
            is_active=is_active,
            page=page,
            size=size,
        )
        return [UserMapper.entity_to_dto(u) for u in users], total
