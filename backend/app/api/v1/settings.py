"""
API роутер для системних налаштувань Kasa POS.

GET  /api/v1/settings          — отримати всі налаштування, згруповані за модулями
GET  /api/v1/settings/:module  — отримати налаштування конкретного модуля
PUT  /api/v1/settings          — масове оновлення налаштувань
PUT  /api/v1/settings/:key     — оновити конкретне налаштування
"""
from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.infrastructure.persistence.models.system_setting import SystemSetting
from app.schemas.settings import (
    SystemSettingRead,
    SystemSettingUpdate,
    SettingsModuleResponse,
    SystemSettingBatchUpdate,
)
from app.domain.services.auth_service import AuthService
from app.infrastructure.persistence.models.user import User

router = APIRouter(prefix="/settings", tags=["Settings"])


@router.get("", response_model=SettingsModuleResponse)
async def get_all_settings(
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Отримати всі активні налаштування, згруповані за модулями.
    """
    result = await session.execute(
        select(SystemSetting)
        .where(SystemSetting.is_active == True)
        .order_by(SystemSetting.module, SystemSetting.key)
    )
    settings = result.scalars().all()

    modules: dict[str, list[SystemSettingRead]] = {}
    for setting in settings:
        if setting.module not in modules:
            modules[setting.module] = []
        modules[setting.module].append(SystemSettingRead.model_validate(setting))

    return SettingsModuleResponse(modules=modules)


@router.get("/{module}", response_model=list[SystemSettingRead])
async def get_settings_by_module(
    module: str,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Отримати налаштування конкретного модуля (general, pos, тощо).
    """
    result = await session.execute(
        select(SystemSetting)
        .where(SystemSetting.module == module, SystemSetting.is_active == True)
        .order_by(SystemSetting.key)
    )
    settings = result.scalars().all()
    if not settings:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Модуль '{module}' не знайдено або він порожній",
        )
    return [SystemSettingRead.model_validate(s) for s in settings]


@router.put("", response_model=SettingsModuleResponse)
async def batch_update_settings(
    data: SystemSettingBatchUpdate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Масове оновлення налаштувань.

    Очікує словник {key: value} для оновлення декількох налаштувань одночасно.
    """
    if current_user.role != "admin":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Тільки адміністратор може змінювати налаштування",
        )

    updated_keys = []
    for key, value in data.settings.items():
        result = await session.execute(
            select(SystemSetting).where(SystemSetting.key == key)
        )
        setting = result.scalar_one_or_none()
        if setting:
            setting.value = str(value) if value is not None else None
            updated_keys.append(key)

    if updated_keys:
        await session.commit()

    # Повертаємо оновлені налаштування
    result = await session.execute(
        select(SystemSetting)
        .where(SystemSetting.is_active == True)
        .order_by(SystemSetting.module, SystemSetting.key)
    )
    settings = result.scalars().all()
    modules: dict[str, list[SystemSettingRead]] = {}
    for setting in settings:
        if setting.module not in modules:
            modules[setting.module] = []
        modules[setting.module].append(SystemSettingRead.model_validate(setting))

    return SettingsModuleResponse(modules=modules)


@router.put("/{key}", response_model=SystemSettingRead)
async def update_setting(
    key: str,
    data: SystemSettingUpdate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Оновити конкретне налаштування за ключем.
    """
    if current_user.role != "admin":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Тільки адміністратор може змінювати налаштування",
        )

    result = await session.execute(
        select(SystemSetting).where(SystemSetting.key == key)
    )
    setting = result.scalar_one_or_none()
    if not setting:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Налаштування з ключем '{key}' не знайдено",
        )

    setting.value = data.value
    await session.commit()
    await session.refresh(setting)

    return SystemSettingRead.model_validate(setting)
