"""
Infrastructure Layer: DI-збірка use cases ПРРО.

build_prro_use_cases(session) — єдина точка створення фасаду PrroUseCases
з компонентами інфраструктури:
  - PrroServiceFactory  (gRPC-клієнти, кеш каналів) — singleton на процес;
  - PrroKeyStore        (шлях/пароль ключа, Fernet) — singleton на процес;
  - PrroRepository / PrroSettingsRepository — transient (на сесію).

Використовується у api/v2/deps.get_prro_use_cases та при інтеграції
авто-фіскалізації у receipt_use_cases.
"""

from __future__ import annotations

import logging

from sqlalchemy.ext.asyncio import AsyncSession

from app.application.use_cases.prro.context import PrroContextFactory
from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    FiscalizeReceiptUseCase,
)
from app.application.use_cases.prro.prro_settings_use_case import PrroSettingsUseCase
from app.application.use_cases.prro.prro_status_use_case import PrroStatusUseCase
from app.application.use_cases.prro.prro_use_cases import PrroUseCases
from app.application.use_cases.prro.shift_use_case import PrroShiftUseCase
from app.application.use_cases.prro.sync_offline_queue_use_case import (
    SyncOfflineQueueUseCase,
)
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.factory import PrroServiceFactory
from app.infrastructure.services.prro.key_store import PrroKeyStore
from app.infrastructure.services.prro.offline_queue import PrroOfflineQueue

logger = logging.getLogger(__name__)

# Singleton-компоненти (на весь процес)
_service_factory: PrroServiceFactory | None = None
_key_store: PrroKeyStore | None = None


def get_prro_service_factory() -> PrroServiceFactory:
    """Повертає singleton PrroServiceFactory (кеш gRPC-каналів)."""
    global _service_factory
    if _service_factory is None:
        _service_factory = PrroServiceFactory()
    return _service_factory


def get_prro_key_store() -> PrroKeyStore:
    """Повертає singleton PrroKeyStore (Fernet, файли .prro_*)."""
    global _key_store
    if _key_store is None:
        _key_store = PrroKeyStore()
    return _key_store


def build_prro_context(session: AsyncSession) -> PrroContextFactory:
    """Будує PrroContextFactory на основі сесії та singleton-компонентів."""
    settings_repo = PrroSettingsRepository(session)
    return PrroContextFactory(
        settings_repo=settings_repo,
        key_store=get_prro_key_store(),
        service_factory=get_prro_service_factory(),
    )


def build_prro_use_cases(session: AsyncSession) -> PrroUseCases:
    """
    Створює фасад PrroUseCases на основі сесії БД.

    Args:
        session: асинхронна сесія (per-request).

    Returns:
        PrroUseCases — готовий фасад з усіма use cases.
    """
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)
    context = PrroContextFactory(
        settings_repo=settings_repo,
        key_store=get_prro_key_store(),
        service_factory=get_prro_service_factory(),
    )
    offline_queue = PrroOfflineQueue(prro_repo)

    settings_uc = PrroSettingsUseCase(
        settings_repo=settings_repo,
        prro_repo=prro_repo,
        key_store=get_prro_key_store(),
        context_factory=context,
    )
    shift_uc = PrroShiftUseCase(
        session=session,
        prro_repo=prro_repo,
        settings_repo=settings_repo,
        context_factory=context,
        offline_queue=offline_queue,
    )
    fiscalize_uc = FiscalizeReceiptUseCase(
        session=session,
        prro_repo=prro_repo,
        settings_repo=settings_repo,
        context_factory=context,
        offline_queue=offline_queue,
    )
    sync_uc = SyncOfflineQueueUseCase(
        session=session,
        prro_repo=prro_repo,
        settings_repo=settings_repo,
        context_factory=context,
        offline_queue=offline_queue,
    )
    status_uc = PrroStatusUseCase(
        prro_repo=prro_repo,
        settings_repo=settings_repo,
        context_factory=context,
    )

    return PrroUseCases(
        settings=settings_uc,
        shift=shift_uc,
        fiscalize=fiscalize_uc,
        sync=sync_uc,
        status=status_uc,
    )




def build_fiscalize_use_case(session: AsyncSession) -> FiscalizeReceiptUseCase:
    """
    Створює FiscalizeReceiptUseCase на основі сесії БД.

    Використовується для авто-фіскалізації після створення чеку
    (інтеграція у ReceiptUseCases через fiscalizer_factory).
    """
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)
    context = PrroContextFactory(
        settings_repo=settings_repo,
        key_store=get_prro_key_store(),
        service_factory=get_prro_service_factory(),
    )
    return FiscalizeReceiptUseCase(
        session=session,
        prro_repo=prro_repo,
        settings_repo=settings_repo,
        context_factory=context,
        offline_queue=PrroOfflineQueue(prro_repo),
    )

async def close_prro_service_factory() -> None:
    """Закриває всі gRPC-канали (викликається при graceful shutdown)."""
    global _service_factory
    if _service_factory is not None:
        await _service_factory.close()
        _service_factory = None


__all__ = [
    "build_fiscalize_use_case",
    "build_prro_context",
    "build_prro_use_cases",
    "close_prro_service_factory",
    "get_prro_key_store",
    "get_prro_service_factory",
]
