"""
Сервіс авторизації користувачів.

Забезпечує:
  - Логін за паролем (bcrypt через passlib)
  - Логін за PIN-кодом (bcrypt через passlib)
  - Генерацію JWT токенів (через python-jose)
  - Верифікацію токенів
  - Оновлення токенів (refresh)
  - Перевірку прав доступу (permissions)
"""

from datetime import datetime, timedelta
from typing import Optional
from uuid import UUID

from fastapi import Depends, HTTPException, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from jose import JWTError, jwt
from passlib.context import CryptContext
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.config import settings
from app.database import get_session
from app.infrastructure.persistence.models.user import User, UserRole
from app.infrastructure.persistence.models.permission import Permission, ADMIN_PERMISSIONS, CASHIER_PERMISSIONS

# Схема Bearer токена для Swagger
# auto_error=False — дозволяє передати None в get_current_user_optional
security_scheme = HTTPBearer(auto_error=False)

# Контекст хешування паролів (bcrypt)
pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")


def get_default_permissions(role: UserRole) -> list[str]:
    """
    Повертає набір прав за замовчуванням для вказаної ролі.

    Args:
        role: Роль користувача.

    Returns:
        Список рядків-пермішенів.
    """
    if role == UserRole.ADMIN:
        return ADMIN_PERMISSIONS
    return CASHIER_PERMISSIONS


class AuthService:
    """
    Сервіс авторизації та аутентифікації.

    Використовує passlib (bcrypt) для хешування паролів та PIN-кодів,
    та JWT (HS256 через python-jose) для генерації токенів доступу.
    """

    def __init__(self, session: AsyncSession):
        """Ініціалізація сервісу з асинхронною сесією БД."""
        self.session = session

    # ─── Хешування ───────────────────────────────────────────────────────────

    @staticmethod
    def hash_password(password: str) -> str:
        """
        Хешує пароль за допомогою bcrypt (через passlib).

        Args:
            password: Пароль у відкритому вигляді.

        Returns:
            Закодований bcrypt хеш (str).
        """
        return pwd_context.hash(password)

    @staticmethod
    def verify_password(password: str, hashed: str) -> bool:
        """
        Перевіряє пароль проти хешу.

        Args:
            password: Пароль у відкритому вигляді.
            hashed: Збережений bcrypt хеш.

        Returns:
            True якщо пароль співпадає, інакше False.
        """
        return pwd_context.verify(password, hashed)

    # ─── JWT ─────────────────────────────────────────────────────────────────

    @staticmethod
    def create_access_token(
        user_id: UUID,
        role: UserRole,
        permissions: Optional[list[str]] = None,
        expires_delta: Optional[timedelta] = None,
    ) -> str:
        """
        Створює JWT токен доступу.

        Args:
            user_id: ID користувача.
            role: Роль користувача.
            permissions: Список прав доступу. Якщо None — використовуються
                         права за замовчуванням для ролі.
            expires_delta: Час дії токена (за замовчуванням з конфігу).

        Returns:
            JWT токен у вигляді рядка.
        """
        if permissions is None:
            permissions = get_default_permissions(role)

        to_encode = {
            "sub": str(user_id),
            "role": role.value if hasattr(role, "value") else role,
            "permissions": permissions,
            "type": "access",
            "iat": datetime.utcnow(),
        }
        if expires_delta:
            expire = datetime.utcnow() + expires_delta
        else:
            expire = datetime.utcnow() + timedelta(
                minutes=settings.ACCESS_TOKEN_EXPIRE_MINUTES
            )
        to_encode["exp"] = expire
        return jwt.encode(
            to_encode,
            settings.SECRET_KEY,
            algorithm="HS256",
        )

    @staticmethod
    def create_refresh_token(
        user_id: UUID,
        role: UserRole,
        expires_delta: Optional[timedelta] = None,
    ) -> str:
        """
        Створює JWT refresh токен (для оновлення access токена).

        Args:
            user_id: ID користувача.
            role: Роль користувача.
            expires_delta: Час дії токена (за замовчуванням 7 днів).

        Returns:
            JWT refresh токен у вигляді рядка.
        """
        to_encode = {
            "sub": str(user_id),
            "role": role.value if hasattr(role, "value") else role,
            "type": "refresh",
            "iat": datetime.utcnow(),
        }
        if expires_delta:
            expire = datetime.utcnow() + expires_delta
        else:
            expire = datetime.utcnow() + timedelta(
                minutes=settings.REFRESH_TOKEN_EXPIRE_MINUTES
            )
        to_encode["exp"] = expire
        return jwt.encode(
            to_encode,
            settings.SECRET_KEY,
            algorithm="HS256",
        )

    @staticmethod
    def decode_access_token(token: str) -> dict:
        """
        Декодує та верифікує JWT токен.

        Args:
            token: JWT токен.

        Returns:
            Словник з даними токена.

        Raises:
            HTTPException 401: Якщо токен недійсний або прострочений.
        """
        try:
            payload = jwt.decode(
                token,
                settings.SECRET_KEY,
                algorithms=["HS256"],
            )
            return payload
        except JWTError:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Недійсний або прострочений токен",
            )

    # ─── Логін ───────────────────────────────────────────────────────────────

    async def login_by_password(self, login: str, password: str) -> tuple[User, str]:
        """
        Аутентифікація користувача за логіном та паролем.

        Args:
            login: Логін користувача.
            password: Пароль у відкритому вигляді.

        Returns:
            Кортеж (User, JWT токен).

        Raises:
            HTTPException 401: Якщо логін або пароль невірні,
                               або користувач неактивний.
        """
        # Шукаємо користувача за логіном
        result = await self.session.execute(
            select(User).where(User.login == login)
        )
        user = result.scalar_one_or_none()

        if not user:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Невірний логін або пароль",
            )

        if not user.is_active:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="Користувач деактивований",
            )

        # Перевіряємо пароль
        if not self.verify_password(password, user.password_hash):
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Невірний логін або пароль",
            )

        # Отримуємо права доступу
        permissions = user.permissions if user.permissions is not None else get_default_permissions(user.role)

        # Генеруємо токен з правами
        token = self.create_access_token(user.id, user.role, permissions)
        return user, token

    async def login_by_pin(self, login: str, pin_code: str) -> tuple[User, str]:
        """
        Аутентифікація користувача за логіном та PIN-кодом.

        Використовується для швидкого входу на касі.

        Args:
            login: Логін користувача.
            pin_code: PIN-код у відкритому вигляді.

        Returns:
            Кортеж (User, JWT токен).

        Raises:
            HTTPException 401: Якщо логін або PIN невірні,
                               або користувач неактивний.
        """
        # Шукаємо користувача за логіном
        result = await self.session.execute(
            select(User).where(User.login == login)
        )
        user = result.scalar_one_or_none()

        if not user:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Невірний логін або PIN-код",
            )

        if not user.is_active:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="Користувач деактивований",
            )

        # Перевіряємо чи встановлений PIN-код
        if not user.pin_code:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="PIN-код не встановлений для цього користувача",
            )

        # Перевіряємо PIN-код
        if not self.verify_password(pin_code, user.pin_code):
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Невірний логін або PIN-код",
            )

        # Отримуємо права доступу
        permissions = user.permissions if user.permissions is not None else get_default_permissions(user.role)

        # Генеруємо токен з правами
        token = self.create_access_token(user.id, user.role, permissions)
        return user, token

    # ─── Отримання поточного користувача ─────────────────────────────────────

    @staticmethod
    async def get_current_user(
        credentials: Optional[HTTPAuthorizationCredentials] = Depends(security_scheme),
        session: AsyncSession = Depends(get_session),
    ) -> User:
        """
        Dependency: Отримує поточного авторизованого користувача з токена.

        Args:
            credentials: Bearer токен з заголовка Authorization.
            session: Асинхронна сесія БД.

        Returns:
            Об'єкт User.

        Raises:
            HTTPException 401: Якщо токен недійсний або користувача не знайдено.
        """
        # Перевіряємо наявність токена
        if not credentials or not credentials.credentials:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Відсутній заголовок авторизації",
            )

        # Декодуємо токен
        payload = AuthService.decode_access_token(credentials.credentials)
        user_id = payload.get("sub")

        if not user_id:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Недійсний токен: відсутній ідентифікатор користувача",
            )

        # Шукаємо користувача в БД
        result = await session.execute(
            select(User).where(User.id == UUID(user_id))
        )
        user = result.scalar_one_or_none()

        if not user:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Користувача не знайдено",
            )

        if not user.is_active:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="Користувач деактивований",
            )

        return user

    @staticmethod
    async def get_current_user_optional(
        credentials: Optional[HTTPAuthorizationCredentials] = Depends(security_scheme),
        session: AsyncSession = Depends(get_session),
    ) -> Optional[User]:
        """
        Dependency: Отримує поточного користувача з токена (опціонально).

        На відміну від get_current_user, ця функція НЕ кидає 401,
        якщо токен відсутній або недійсний. Замість цього повертає None.

        Використовується для публічних ендпоінтів, які хочуть визначити,
        чи є користувач авторизованим, але не вимагають цього.

        Args:
            credentials: Bearer токен (або None, якщо не переданий).
            session: Асинхронна сесія БД.

        Returns:
            Об'єкт User або None, якщо токен відсутній або недійсний.
        """
        # Якщо токен не переданий — повертаємо None
        if not credentials or not credentials.credentials:
            return None

        try:
            # Декодуємо токен
            payload = AuthService.decode_access_token(credentials.credentials)
            user_id = payload.get("sub")

            if not user_id:
                return None

            # Шукаємо користувача в БД
            result = await session.execute(
                select(User).where(User.id == UUID(user_id))
            )
            user = result.scalar_one_or_none()

            if not user or not user.is_active:
                return None

            return user

        except (HTTPException, JWTError, ValueError):
            # Якщо токен недійсний або будь-яка інша помилка — повертаємо None
            return None

    # ─── Перевірка ролі ──────────────────────────────────────────────────────

    @staticmethod
    def require_admin(user: User = Depends(get_current_user)) -> User:
        """
        Dependency: Перевіряє, що користувач має роль ADMIN.

        Args:
            user: Поточний користувач.

        Returns:
            Об'єкт User, якщо роль ADMIN.

        Raises:
            HTTPException 403: Якщо роль не ADMIN.
        """
        if user.role != UserRole.ADMIN:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="Доступ заборонено: потрібна роль адміністратора",
            )
        return user

    # ─── Перевірка прав доступу ──────────────────────────────────────────────

    @staticmethod
    def require_permission(permission: Permission):
        """
        Dependency factory: Перевіряє, що користувач має конкретне право доступу.

        Використання:
            @router.get("/products")
            async def list_products(
                user: User = Depends(AuthService.require_permission(Permission.PRODUCTS_VIEW)),
            ):
                ...

        Args:
            permission: Право доступу, яке перевіряється.

        Returns:
            Dependency функція, яка повертає User, якщо право є.
        """
        async def _check_permission(
            credentials: Optional[HTTPAuthorizationCredentials] = Depends(security_scheme),
            session: AsyncSession = Depends(get_session),
        ) -> User:
            # Спочатку отримуємо користувача
            user = await AuthService.get_current_user(credentials, session)

            # Отримуємо права з токена або з БД
            payload = AuthService.decode_access_token(credentials.credentials)
            token_permissions = payload.get("permissions", [])

            # Якщо в токені немає прав, беремо з БД
            if not token_permissions:
                token_permissions = user.permissions if user.permissions is not None else get_default_permissions(user.role)

            # Перевіряємо наявність права
            if permission.value not in token_permissions:
                raise HTTPException(
                    status_code=status.HTTP_403_FORBIDDEN,
                    detail=f"Доступ заборонено: потрібне право '{permission.value}'",
                )

            return user

        return _check_permission
