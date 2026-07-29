"""
Pydantic схеми для моделі User (Користувач системи).

Містить схеми для CRUD, логіну за паролем та PIN-кодом.
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict

from app.infrastructure.persistence.models.user import UserRole
from app.infrastructure.persistence.models.permission import Permission


class UserCreate(BaseModel):
    """Схема створення нового користувача."""
    name: str = Field(..., max_length=255, description="Повне ім'я користувача")
    login: Optional[str] = Field(None, max_length=100, description="Логін для входу (якщо не вказано — генерується з імені)")
    password: str = Field(..., min_length=4, max_length=100, description="Пароль")
    pin_code: Optional[str] = Field(None, min_length=4, max_length=10, description="PIN-код для каси")
    role: UserRole = Field(UserRole.CASHIER, description="Роль користувача")
    is_active: bool = Field(True, description="Активний користувач")
    permissions: Optional[list[str]] = Field(
        None,
        description="Список прав доступу. Якщо None — використовуються права за замовчуванням для ролі.",
    )


class UserUpdate(BaseModel):
    """Схема оновлення користувача. Всі поля опціональні."""
    name: Optional[str] = Field(None, max_length=255, description="Повне ім'я користувача")
    login: Optional[str] = Field(None, max_length=100, description="Логін для входу")
    password: Optional[str] = Field(None, min_length=4, max_length=100, description="Пароль")
    pin_code: Optional[str] = Field(None, min_length=4, max_length=10, description="PIN-код для каси")
    role: Optional[UserRole] = Field(None, description="Роль користувача")
    is_active: Optional[bool] = Field(None, description="Активний користувач")
    permissions: Optional[list[str]] = Field(
        None,
        description="Список прав доступу. Якщо None — використовуються права за замовчуванням для ролі.",
    )


class UserPermissionsUpdate(BaseModel):
    """Схема оновлення тільки прав доступу користувача."""
    permissions: list[str] = Field(
        ...,
        description="Новий список прав доступу для користувача",
    )


class UserResponse(BaseModel):
    """Схема відповіді з даними користувача (без пароля та PIN)."""
    id: UUID
    name: str
    login: str
    role: UserRole
    is_active: bool
    permissions: Optional[list[str]] = None
    created_at: datetime
    updated_at: datetime

    model_config = ConfigDict(from_attributes=True)


class UserLoginRequest(BaseModel):
    """Схема запиту на логін за паролем."""
    login: str = Field(..., description="Логін користувача")
    password: str = Field(..., description="Пароль")


class UserPinLoginRequest(BaseModel):
    """Схема запиту на логін за PIN-кодом."""
    login: str = Field(..., description="Логін користувача")
    pin_code: str = Field(..., description="PIN-код")


class UserTokenResponse(BaseModel):
    """Схема відповіді з токеном авторизації."""
    access_token: str = Field(..., description="JWT токен доступу")
    refresh_token: Optional[str] = Field(None, description="JWT токен оновлення")
    token_type: str = Field("bearer", description="Тип токена")
    user: UserResponse = Field(..., description="Дані користувача")
