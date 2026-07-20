"""
API роутер для роботи з користувачами (Users) та авторизацією.

Ендпоінти:
  - POST   /auth/login         — логін за паролем
  - POST   /auth/login-pin     — логін за PIN-кодом
  - GET    /users               — список користувачів (admin)
  - GET    /users/{id}          — отримати користувача за ID
  - POST   /users               — створити користувача (admin)
  - PUT    /users/{id}          — оновити користувача (admin)
  - DELETE /users/{id}          — видалити користувача (admin)
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.models.user import User
from app.schemas.user import (
    UserCreate,
    UserUpdate,
    UserResponse,
    UserLoginRequest,
    UserPinLoginRequest,
    UserTokenResponse,
)
from app.services.auth_service import AuthService

# Роутер для авторизації (публічний)
auth_router = APIRouter(
    prefix="/auth",
    tags=["Авторизація"],
)

# Роутер для управління користувачами (тільки admin)
users_router = APIRouter(
    prefix="/users",
    tags=["Користувачі"],
)


# ─── Авторизація ─────────────────────────────────────────────────────────────

@auth_router.post("/login", response_model=UserTokenResponse)
async def login(
    data: UserLoginRequest,
    session: AsyncSession = Depends(get_session),
):
    """
    Аутентифікація користувача за логіном та паролем.

    Повертає JWT токен доступу та дані користувача.
    """
    auth_service = AuthService(session)
    user, token = await auth_service.login_by_password(data.login, data.password)
    return UserTokenResponse(
        access_token=token,
        token_type="bearer",
        user=UserResponse.model_validate(user),
    )


@auth_router.post("/login-pin", response_model=UserTokenResponse)
async def login_pin(
    data: UserPinLoginRequest,
    session: AsyncSession = Depends(get_session),
):
    """
    Аутентифікація користувача за логіном та PIN-кодом.

    Використовується для швидкого входу на касі.
    Повертає JWT токен доступу та дані користувача.
    """
    auth_service = AuthService(session)
    user, token = await auth_service.login_by_pin(data.login, data.pin_code)
    return UserTokenResponse(
        access_token=token,
        token_type="bearer",
        user=UserResponse.model_validate(user),
    )


# ─── CRUD Користувачі ────────────────────────────────────────────────────────

@users_router.get("", response_model=list[UserResponse])
async def list_users(
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """Отримує список всіх користувачів (тільки admin)."""
    result = await session.execute(select(User).order_by(User.name))
    users = result.scalars().all()
    return [UserResponse.model_validate(u) for u in users]


@users_router.get("/{user_id}", response_model=UserResponse)
async def get_user(
    user_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """Отримує користувача за ID (тільки admin)."""
    result = await session.execute(
        select(User).where(User.id == user_id)
    )
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Користувача з ID '{user_id}' не знайдено",
        )
    return UserResponse.model_validate(user)


@users_router.post("", response_model=UserResponse, status_code=201)
async def create_user(
    data: UserCreate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """Створює нового користувача (тільки admin)."""
    # Перевіряємо унікальність логіну
    result = await session.execute(
        select(User).where(User.login == data.login)
    )
    if result.scalar_one_or_none():
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"Користувач з логіном '{data.login}' вже існує",
        )

    # Хешуємо пароль та PIN-код
    password_hash = AuthService.hash_password(data.password)
    pin_hash = None
    if data.pin_code:
        pin_hash = AuthService.hash_password(data.pin_code)

    user = User(
        name=data.name,
        login=data.login,
        password_hash=password_hash,
        pin_code=pin_hash,
        role=data.role,
        is_active=data.is_active,
    )
    session.add(user)
    await session.flush()
    return UserResponse.model_validate(user)


@users_router.put("/{user_id}", response_model=UserResponse)
async def update_user(
    user_id: UUID,
    data: UserUpdate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """Оновлює дані користувача (тільки admin)."""
    result = await session.execute(
        select(User).where(User.id == user_id)
    )
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Користувача з ID '{user_id}' не знайдено",
        )

    # Перевіряємо унікальність логіну
    if data.login is not None and data.login != user.login:
        result = await session.execute(
            select(User).where(User.login == data.login)
        )
        if result.scalar_one_or_none():
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=f"Користувач з логіном '{data.login}' вже існує",
            )

    update_data = data.model_dump(exclude_unset=True)

    # Хешуємо пароль, якщо переданий
    if "password" in update_data and update_data["password"]:
        update_data["password_hash"] = AuthService.hash_password(
            update_data.pop("password")
        )
    else:
        update_data.pop("password", None)

    # Хешуємо PIN-код, якщо переданий
    if "pin_code" in update_data and update_data["pin_code"]:
        update_data["pin_code"] = AuthService.hash_password(
            update_data["pin_code"]
        )

    for field, value in update_data.items():
        setattr(user, field, value)

    await session.flush()
    return UserResponse.model_validate(user)


@users_router.delete("/{user_id}", status_code=204)
async def delete_user(
    user_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """Видаляє користувача (тільки admin)."""
    result = await session.execute(
        select(User).where(User.id == user_id)
    )
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Користувача з ID '{user_id}' не знайдено",
        )
    await session.delete(user)
    await session.flush()
