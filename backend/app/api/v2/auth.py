"""Auth API v2 — використовує AuthUseCases."""

from __future__ import annotations

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field

from app.application.use_cases import AuthUseCases
from .deps import get_auth_use_cases

router = APIRouter(prefix="/auth", tags=["auth_v2"])


# ─── Pydantic схеми ──────────────────────────────────────────────────────────

class UserResponse(BaseModel):
    id: UUID
    name: str
    login: str
    role: str = "cashier"
    is_active: bool = True
    email: str = ""
    phone: str = ""
    created_at: str | None = None
    last_login_at: str | None = None

    model_config = {"from_attributes": True}


class LoginRequest(BaseModel):
    login: str = Field(..., min_length=1, max_length=100)
    password: str = Field(..., min_length=1)


class LoginPinRequest(BaseModel):
    login: str = Field(..., min_length=1, max_length=100)
    pin_code: str = Field(..., min_length=4, max_length=10)


class TokenResponse(BaseModel):
    access_token: str
    token_type: str = "bearer"
    user: UserResponse


class CreateUserRequest(BaseModel):
    name: str = Field(..., min_length=1, max_length=255)
    login: str | None = None
    password: str = ""
    role: str = "cashier"
    is_active: bool = True
    email: str = ""
    phone: str = ""
    pin_code: str | None = None


class UserListResponse(BaseModel):
    items: list[UserResponse]
    total: int
    page: int
    size: int


# ─── Ендпоінти ───────────────────────────────────────────────────────────────

@router.post("/login", response_model=TokenResponse)
async def login(
    data: LoginRequest,
    use_cases: AuthUseCases = Depends(get_auth_use_cases),
):
    """Вхід за логіном та паролем."""
    try:
        user, token = await use_cases.login(data.login, data.password)
        return {
            "access_token": token,
            "token_type": "bearer",
            "user": user,
        }
    except ValueError as e:
        raise HTTPException(status_code=401, detail=str(e))


@router.post("/login-pin", response_model=TokenResponse)
async def login_pin(
    data: LoginPinRequest,
    use_cases: AuthUseCases = Depends(get_auth_use_cases),
):
    """Вхід за логіном та PIN-кодом."""
    try:
        user, token = await use_cases.login_by_pin(data.login, data.pin_code)
        return {
            "access_token": token,
            "token_type": "bearer",
            "user": user,
        }
    except ValueError as e:
        raise HTTPException(status_code=401, detail=str(e))


@router.post("/refresh", response_model=TokenResponse)
async def refresh_token(
    user_id: UUID,
    use_cases: AuthUseCases = Depends(get_auth_use_cases),
):
    """Оновити JWT токен."""
    try:
        user, token = await use_cases.refresh_token(user_id)
        return {
            "access_token": token,
            "token_type": "bearer",
            "user": user,
        }
    except ValueError as e:
        raise HTTPException(status_code=401, detail=str(e))


@router.get("/users/me", response_model=UserResponse)
async def get_current_user(
    user_id: UUID = Query(..., description="ID поточного користувача"),
    use_cases: AuthUseCases = Depends(get_auth_use_cases),
):
    """Отримати поточного користувача."""
    try:
        return await use_cases.get_current_user(user_id)
    except ValueError as e:
        raise HTTPException(status_code=404, detail=str(e))


@router.post("/users", response_model=UserResponse, status_code=201)
async def create_user(
    data: CreateUserRequest,
    use_cases: AuthUseCases = Depends(get_auth_use_cases),
):
    """Створити нового користувача."""
    try:
        from app.application.dto.user_dto import UserCreateDTO
        dto = UserCreateDTO(
            name=data.name,
            login=data.login or data.name,
            password=data.password,
            role=data.role,
            is_active=data.is_active,
            email=data.email,
            phone=data.phone,
            pin_code=data.pin_code,
        )
        return await use_cases.create_user(dto)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.get("/users", response_model=UserListResponse)
async def list_users(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    search: str | None = None,
    role: str | None = None,
    is_active: bool | None = None,
    use_cases: AuthUseCases = Depends(get_auth_use_cases),
):
    """Отримати список користувачів з пагінацією та фільтрацією."""
    users, total = await use_cases.list_users(
        query=search,
        role=role,
        is_active=is_active,
        page=page,
        size=size,
    )
    return {
        "items": users,
        "total": total,
        "page": page,
        "size": size,
    }
