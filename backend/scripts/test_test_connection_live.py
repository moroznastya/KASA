"""
Реальний тест test_connection з оновленою логікою (XML T=111).
Запуск: ./venv/bin/python scripts/test_test_connection_live.py
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

from app.application.use_cases.prro.context import PrroContextFactory
from app.application.use_cases.prro.prro_settings_use_case import PrroSettingsUseCase
from app.config import settings as app_settings
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.factory import PrroServiceFactory
from app.infrastructure.services.prro.key_store import PrroKeyStore


async def main() -> None:
    engine = create_async_engine(app_settings.DATABASE_URL)
    Session = async_sessionmaker(engine, expire_on_commit=False)
    session: AsyncSession = Session()

    key_store = PrroKeyStore()
    service_factory = PrroServiceFactory()
    settings_repo = PrroSettingsRepository(session)
    prro_repo = PrroRepository(session)
    ctx = PrroContextFactory(
        settings_repo=settings_repo,
        key_store=key_store,
        service_factory=service_factory,
    )

    uc = PrroSettingsUseCase(
        settings_repo=settings_repo,
        prro_repo=prro_repo,
        key_store=key_store,
        context_factory=ctx,
    )

    check_sign, sign_error = await uc._build_ping_check_sign()
    print("check_sign len:", len(check_sign))
    print("sign_error:", (sign_error or "OK (ключ прочитано)")[:150])

    result = await uc.test_connection()
    print("\n=== test_connection результат ===")
    for k, v in result.items():
        print(f"  {k}: {v}")

    await session.close()
    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(main())
