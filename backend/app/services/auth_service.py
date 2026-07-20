"""
Сервіс авторизації користувачів.

Забезпечує:
  - Логін за паролем (bcrypt через passlib)
  - Логін за PIN-кодом (bcrypt через passlib)
  - Генерацію JWT токенів (через python-jose)
  - Верифікацію токенів
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
from app.models.user import User, UserRole

# Схема Bearer токена для Swagger
security_scheme = HTTPBearer()

# Контекст хешування паролів (bcrypt)
pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")


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
        to_encode = {
            "sub": str(user_id),
            "role": role.value if hasattr(role, "value") else role,
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

        # Генеруємо токен
        token = self.create_access_token(user.id, user.role)
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

        # Генеруємо токен
        token = self.create_access_token(user.id, user.role)
        return user, token

    # ─── Отримання поточного користувача ─────────────────────────────────────

    @staticmethod
    async def get_current_user(
        credentials: HTTPAuthorizationCredentials = Depends(security_scheme),
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
            select(User).where(User.id == user_id)
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
