"""
Infrastructure Layer: PrroSettingsRepository — налаштування ПРРО (ключ-значення).

Працює з моделлю PrroSetting (таблиця prro_settings):
  - ключі: prro_fn, prro_tn, prro_zn, mode, url, last_shift_number,
           last_packet_id, last_mac_number, auto_fiscalize, ...
  - шлях/пароль ключа КЕП зберігаються ОКРЕМО у PrroKeyStore (Fernet),
    цей репозиторій їх НЕ торкається.

Використання:
    repo = PrroSettingsRepository(session)
    await repo.set("prro_fn", "4538765845")
    fn = await repo.get("prro_fn")
"""

from __future__ import annotations

from typing import Optional

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.infrastructure.persistence.models.prro import PrroSetting


class PrroSettingsRepository:
    """Репозиторій налаштувань ПРРО (ключ-значення)."""

    def __init__(self, session: AsyncSession):
        self._session = session

    async def get(self, key: str) -> Optional[str]:
        """Повертає значення налаштування (або None)."""
        stmt = select(PrroSetting).where(PrroSetting.key_name == key)
        result = await self._session.execute(stmt)
        setting = result.scalar_one_or_none()
        return setting.value if setting else None

    async def set(self, key: str, value: str) -> None:
        """Зберігає/оновлює значення налаштування (upsert)."""
        stmt = select(PrroSetting).where(PrroSetting.key_name == key)
        result = await self._session.execute(stmt)
        setting = result.scalar_one_or_none()
        if setting is None:
            setting = PrroSetting(key_name=key, value=value)
            self._session.add(setting)
        else:
            setting.value = value
        await self._session.flush()

    async def get_many(self, keys: list[str]) -> dict[str, Optional[str]]:
        """Повертає словник {key: value} для вказаних ключів."""
        stmt = select(PrroSetting).where(PrroSetting.key_name.in_(keys))
        result = await self._session.execute(stmt)
        settings = result.scalars().all()
        found = {s.key_name: s.value for s in settings}
        return {key: found.get(key) for key in keys}

    async def get_all(self) -> dict[str, str]:
        """Повертає всі налаштування ПРРО у вигляді {key: value}."""
        stmt = select(PrroSetting)
        result = await self._session.execute(stmt)
        settings = result.scalars().all()
        return {s.key_name: s.value or "" for s in settings}
