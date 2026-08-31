"""
Контракт модуля Auth (Авторизація).

Визначає інтерфейс для аутентифікації та авторизації користувачів.
"""

from datetime import timedelta
from typing import TYPE_CHECKING, Optional, Protocol
from uuid import UUID

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.user import User
    from app.schemas.user import UserCreate, UserUpdate


class AuthModuleInterface(Protocol):
    """
    Інтерфейс модуля авторизації.

    Відповідає за:
    - Аутентифікацію (логін за паролем та PIN-кодом)
    - Генерацію та верифікацію JWT токенів
    - Управління користувачами
    - Перевірку ролей та прав доступу

    Це єдиний модуль, який може працювати з паролями та токенами.
    Інші модулі отримують інформацію про користувача через ContextProvider.
    """

    # ─── Події, які публікує ─────────────────────────────────────────────
    # publishes:
    #   - "user.logged_in"      — коли користувач увійшов в систему
    #   - "user.logged_out"     — коли користувач вийшов
    #   - "user.created"        — коли створено нового користувача
    #
    # ─── Події, на які підписується ───────────────────────────────────────
    # subscribes:
    #   - (не підписується на події інших модулів)

    # ─── Аутентифікація ──────────────────────────────────────────────────

    async def login_by_password(self, login: str, password: str) -> tuple["User", str]:
        """
        Аутентифікація користувача за логіном та паролем.

        Після успішного входу публікує подію "user.logged_in".

        Args:
            login: Логін користувача.
            password: Пароль у відкритому вигляді.

        Returns:
            Кортеж (User, JWT токен).

        Raises:
            InvalidCredentials: Якщо логін або пароль невірні.
            UserNotActive: Якщо користувач деактивований.
        """
        ...

    async def login_by_pin(self, login: str, pin_code: str) -> tuple["User", str]:
        """
        Аутентифікація користувача за логіном та PIN-кодом.

        Використовується для швидкого входу на касі.

        Args:
            login: Логін користувача.
            pin_code: PIN-код у відкритому вигляді.

        Returns:
            Кортеж (User, JWT токен).

        Raises:
            InvalidCredentials: Якщо логін або PIN невірні.
            PinNotSet: Якщо PIN-код не встановлений.
        """
        ...

    # ─── JWT ─────────────────────────────────────────────────────────────

    def create_access_token(
        self,
        user_id: UUID,
        role: str,
        expires_delta: Optional[timedelta] = None,
    ) -> str:
        """
        Створює JWT токен доступу.

        Args:
            user_id: ID користувача.
            role: Роль користувача.
            expires_delta: Час дії токена (за замовчуванням з конфігу).

        Returns:
            JWT токен у вигляді рядка.
        """
        ...

    def decode_access_token(self, token: str) -> dict:
        """
        Декодує та верифікує JWT токен.

        Args:
            token: JWT токен.

        Returns:
            Словник з даними токена (sub, role, exp, iat).

        Raises:
            InvalidToken: Якщо токен недійсний або прострочений.
        """
        ...

    # ─── Отримання поточного користувача ─────────────────────────────────

    async def get_current_user(self, token: str) -> "User":
        """
        Отримує користувача за JWT токеном.

        Args:
            token: JWT токен.

        Returns:
            Об'єкт User.

        Raises:
            InvalidToken: Якщо токен недійсний.
            UserNotFound: Якщо користувача не знайдено.
            UserNotActive: Якщо користувач деактивований.
        """
        ...

    # ─── Перевірка ролей ────────────────────────────────────────────────

    def check_role(self, user: "User", required_role: str) -> bool:
        """
        Перевіряє, чи має користувач необхідну роль.

        Args:
            user: Об'єкт User.
            required_role: Необхідна роль.

        Returns:
            True — якщо роль відповідає, False — якщо ні.
        """
        ...

    # ─── Управління користувачами ────────────────────────────────────────

    async def create_user(self, data: "UserCreate") -> "User":
        """
        Створює нового користувача.

        Після створення публікує подію "user.created".
        Пароль автоматично хешується.

        Args:
            data: Дані для створення користувача.

        Returns:
            Створений об'єкт User.
        """
        ...

    async def update_user(self, user_id: UUID, data: "UserUpdate") -> "User":
        """
        Оновлює дані користувача.

        Args:
            user_id: UUID користувача.
            data: Дані для оновлення.

        Returns:
            Оновлений об'єкт User.
        """
        ...

    async def set_pin_code(self, user_id: UUID, pin_code: str) -> None:
        """
        Встановлює або змінює PIN-код користувача.

        Args:
            user_id: UUID користувача.
            pin_code: Новий PIN-код.
        """
        ...
