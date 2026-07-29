"""
Infrastructure Layer: SettingsRepository — читання налаштувань з БД.
"""

from __future__ import annotations

from typing import Optional
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from app.infrastructure.persistence.models.system_setting import SystemSetting


class SettingsRepository:
    """Репозиторій для читання системних налаштувань."""

    def __init__(self, session: AsyncSession):
        self._session = session

    async def get_all(self) -> dict[str, dict[str, str]]:
        """
        Повертає всі налаштування у вигляді:
        {
            "general": {"company_name": "Мій магазин", ...},
            "pos": {"allow_negative_stock": "false", ...},
        }
        """
        result = await self._session.execute(
            select(SystemSetting).where(SystemSetting.is_active == True)
        )
        settings = result.scalars().all()

        modules: dict[str, dict[str, str]] = {}
        for s in settings:
            if s.module not in modules:
                modules[s.module] = {}
            modules[s.module][s.key] = s.value or ""

        return modules

    async def get_by_module(self, module: str) -> dict[str, str]:
        """Повертає налаштування конкретного модуля."""
        result = await self._session.execute(
            select(SystemSetting).where(
                SystemSetting.module == module,
                SystemSetting.is_active == True,
            )
        )
        settings = result.scalars().all()
        return {s.key: s.value or "" for s in settings}

    async def get_one(self, key: str) -> Optional[str]:
        """Повертає значення конкретного налаштування за ключем."""
        result = await self._session.execute(
            select(SystemSetting).where(
                SystemSetting.key == key,
                SystemSetting.is_active == True,
            )
        )
        setting = result.scalar_one_or_none()
        return setting.value if setting else None
