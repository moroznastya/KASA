"""
API роутер для роботи з користувачами (Users) та авторизацією.

Ендпоінти:
  - POST   /auth/login         — логін за паролем
  - POST   /auth/login-pin     — логін за PIN-кодом
  - POST   /auth/refresh       — оновлення JWT токена
  - POST   /auth/logout        — вихід із системи
  - GET    /users               — список користувачів (admin)
  - GET    /users/{id}          — отримати користувача за ID
  - POST   /users               — створити користувача (admin)
  - PUT    /users/{id}          — оновити користувача (admin)
  - DELETE /users/{id}          — видалити користувача (admin)
  - PUT    /users/{id}/permissions — оновити права доступу користувача (admin)
  - GET    /users/permissions/list — отримати список всіх доступних прав
"""

from datetime import timedelta
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Request, status
from slowapi import Limiter
from slowapi.util import get_remote_address
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.models.user import User
from app.models.permission import Permission, PERMISSION_GROUPS, PERMISSION_LABELS
from app.schemas.user import (
    UserCreate,
    UserUpdate,
    UserResponse,
    UserPermissionsUpdate,
    UserLoginRequest,
    UserPinLoginRequest,
    UserTokenResponse,
)
from app.services.auth_service import AuthService

# Rate limiter для auth ендпоінтів (5 запитів на хвилину)
limiter = Limiter(key_func=get_remote_address)

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
@limiter.limit("5/minute")
async def login(
    request: Request,
    data: UserLoginRequest,
    session: AsyncSession = Depends(get_session),
):
    """
    Аутентифікація користувача за логіном та паролем.

    Повертає JWT токен доступу, refresh токен та дані користувача.
    Rate limit: 5 запитів на хвилину.
    """
    auth_service = AuthService(session)
    user, token = await auth_service.login_by_password(data.login, data.password)
    refresh_token = AuthService.create_refresh_token(user.id, user.role)
    return UserTokenResponse(
        access_token=token,
        refresh_token=refresh_token,
        token_type="bearer",
        user=UserResponse.model_validate(user),
    )


@auth_router.post("/login-pin", response_model=UserTokenResponse)
@limiter.limit("5/minute")
async def login_pin(
    request: Request,
    data: UserPinLoginRequest,
    session: AsyncSession = Depends(get_session),
):
    """
    Аутентифікація користувача за логіном та PIN-кодом.

    Використовується для швидкого входу на касі.
    Повертає JWT токен доступу, refresh токен та дані користувача.
    Rate limit: 5 запитів на хвилину.
    """
    auth_service = AuthService(session)
    user, token = await auth_service.login_by_pin(data.login, data.pin_code)
    refresh_token = AuthService.create_refresh_token(user.id, user.role)
    return UserTokenResponse(
        access_token=token,
        refresh_token=refresh_token,
        token_type="bearer",
        user=UserResponse.model_validate(user),
    )


@auth_router.post("/refresh", response_model=UserTokenResponse)
async def refresh_token(
    data: dict,
    session: AsyncSession = Depends(get_session),
):
    """
    Оновлення JWT токена за refresh_token.

    Приймає `{"refresh_token": "..."}`, декодує його,
    перевіряє користувача в БД та повертає нову пару токенів (access + refresh).
    """
    refresh_token_value = data.get("refresh_token")
    if not refresh_token_value:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Відсутній refresh_token",
        )

    # Декодуємо refresh токен
    payload = AuthService.decode_access_token(refresh_token_value)
    user_id = payload.get("sub")

    if not user_id:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Недійсний refresh_token",
        )

    # Отримуємо користувача з БД
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
            detail="Користувача деактивовано",
        )

    # Створюємо нову пару токенів
    access_token = AuthService.create_access_token(user.id, user.role)
    new_refresh_token = AuthService.create_refresh_token(user.id, user.role)

    return UserTokenResponse(
        access_token=access_token,
        refresh_token=new_refresh_token,
        token_type="bearer",
        user=UserResponse.model_validate(user),
    )


@auth_router.post("/logout")
async def logout(
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Вихід із системи.

    На стороні клієнта потрібно видалити токени.
    """
    return {"message": "Успішний вихід із системи"}


# ─── Управління користувачами ────────────────────────────────────────────────

@users_router.get("", response_model=list[UserResponse])
async def list_users(
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """Отримує список всіх користувачів (тільки admin)."""
    result = await session.execute(
        select(User).order_by(User.name)
    )
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
        permissions=data.permissions,
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


@users_router.put("/{user_id}/permissions", response_model=UserResponse)
async def update_user_permissions(
    user_id: UUID,
    data: UserPermissionsUpdate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Оновлює права доступу користувача (тільки admin).

    Приймає масив рядків-пермішенів, які будуть встановлені користувачу.
    """
    result = await session.execute(
        select(User).where(User.id == user_id)
    )
    user = result.scalar_one_or_none()
    if not user:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Користувача з ID '{user_id}' не знайдено",
        )

    # Валідуємо, що всі передані права існують
    valid_permissions = {p.value for p in Permission}
    for perm in data.permissions:
        if perm not in valid_permissions:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Невідоме право доступу: '{perm}'. "
                       f"Доступні права: {', '.join(sorted(valid_permissions))}",
            )

    user.permissions = data.permissions
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


# ─── Список доступних прав ───────────────────────────────────────────────────

@users_router.get("/permissions/list")
async def list_permissions(
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Повертає список всіх доступних прав доступу, згрупованих за модулями.

    Використовується на фронтенді для відображення галочок.
    """
    return {
        "groups": [
            {
                "name": group_name,
                "icon": group_data["icon"],
                "permissions": [
                    {
                        "key": p.value,
                        "label": PERMISSION_LABELS.get(p.value, p.value.split(":")[1].capitalize()),
                        "description": PERMISSION_LABELS.get(p.value, p.value.split(":")[1].capitalize()),
                    }
                    for p in group_data["permissions"]
                ],
            }
            for group_name, group_data in PERMISSION_GROUPS.items()
        ],
        "all_permissions": sorted([p.value for p in Permission]),
    }
