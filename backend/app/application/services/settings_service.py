"""
Application Layer: SettingsService — сервіс для читання налаштувань з кешуванням.
"""

from __future__ import annotations

from decimal import Decimal

from sqlalchemy.ext.asyncio import AsyncSession

from app.infrastructure.persistence.repositories.settings_repository import SettingsRepository


class SettingsService:
    """
    Сервіс для роботи з системними налаштуваннями.

    Кешує налаштування в пам'яті для швидкого доступу.
    """

    def __init__(self, session: AsyncSession):
        self._repo = SettingsRepository(session)
        self._cache: dict[str, str] = {}
        self._loaded = False

    async def _ensure_loaded(self) -> None:
        """Завантажує всі налаштування в кеш."""
        if not self._loaded:
            modules = await self._repo.get_all()
            for module_settings in modules.values():
                self._cache.update(module_settings)
            self._loaded = True

    async def get_string(self, key: str, default: str = "") -> str:
        """Отримує рядкове налаштування."""
        await self._ensure_loaded()
        return self._cache.get(key, default)

    async def get_bool(self, key: str, default: bool = False) -> bool:
        """Отримує булеве налаштування."""
        await self._ensure_loaded()
        val = self._cache.get(key)
        if val is None:
            return default
        return val.lower() in ("true", "1", "yes", "on")

    async def get_int(self, key: str, default: int = 0) -> int:
        """Отримує цілочисельне налаштування."""
        await self._ensure_loaded()
        val = self._cache.get(key)
        if val is None:
            return default
        try:
            return int(val)
        except (ValueError, TypeError):
            return default

    async def get_decimal(self, key: str, default: Decimal = Decimal("0")) -> Decimal:
        """Отримує Decimal налаштування."""
        await self._ensure_loaded()
        val = self._cache.get(key)
        if val is None:
            return default
        try:
            return Decimal(val)
        except (ValueError, TypeError):
            return default

    def invalidate_cache(self) -> None:
        """Скидає кеш — примусово перезавантажить при наступному запиті."""
        self._loaded = False
        self._cache.clear()
