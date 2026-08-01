"""Dependencies для API v2 — отримання Use Cases та сервісів.

Use Cases будуються з per-request сесією БД (Depends(get_session)),
щоб репозиторії та UnitOfWork працювали в межах однієї транзакції запиту.
Синглтони (event_bus, cache_service, ПРРО key_store/gRPC) резолвляться
з DI-контейнера через request.app.state.di_container.
"""

from __future__ import annotations

from fastapi import Depends, Request
from sqlalchemy.ext.asyncio import AsyncSession

from app.application.use_cases import (
    ProductUseCases,
    InvoiceUseCases,
    ReceiptUseCases,
    AuthUseCases,
    LedgerUseCases,
)
from app.application.use_cases.prro import PrroUseCases
from app.domain.repositories import ICategoryRepository
from app.domain.services.cache_service import ICacheService
from app.database import get_session, async_session

from app.infrastructure.persistence.repositories import (
    SQLAlchemyProductRepository,
    SQLAlchemyInvoiceRepository,
    SQLAlchemyReceiptRepository,
    SQLAlchemyUserRepository,
    SQLAlchemyLedgerRepository,
    SQLAlchemyCategoryRepository,
)
from app.infrastructure.persistence.unit_of_work import SQLAlchemyUnitOfWork
from app.infrastructure.di.prro import build_prro_use_cases, build_fiscalize_use_case


async def get_product_use_cases(
    request: Request,
    session: AsyncSession = Depends(get_session),
) -> ProductUseCases:
    """Отримати ProductUseCases з поточною сесією БД."""
    return ProductUseCases(
        product_repo=SQLAlchemyProductRepository(session=session),
        event_bus=request.app.state.di_container.resolve("event_bus"),
        unit_of_work=SQLAlchemyUnitOfWork(session=session),
    )


async def get_invoice_use_cases(
    request: Request,
    session: AsyncSession = Depends(get_session),
) -> InvoiceUseCases:
    """Отримати InvoiceUseCases з поточною сесією БД."""
    return InvoiceUseCases(
        invoice_repo=SQLAlchemyInvoiceRepository(session=session),
        event_bus=request.app.state.di_container.resolve("event_bus"),
        unit_of_work=SQLAlchemyUnitOfWork(session=session),
    )


async def get_receipt_use_cases(
    request: Request,
    session: AsyncSession = Depends(get_session),
) -> ReceiptUseCases:
    """Отримати ReceiptUseCases з поточною сесією БД."""
    return ReceiptUseCases(
        receipt_repo=SQLAlchemyReceiptRepository(session=session),
        product_repo=SQLAlchemyProductRepository(session=session),
        event_bus=request.app.state.di_container.resolve("event_bus"),
        unit_of_work=SQLAlchemyUnitOfWork(session=session),
        fiscalizer_factory=lambda: build_fiscalize_use_case(async_session()),
    )


async def get_auth_use_cases(
    request: Request,
    session: AsyncSession = Depends(get_session),
) -> AuthUseCases:
    """Отримати AuthUseCases з поточною сесією БД."""
    return AuthUseCases(
        user_repo=SQLAlchemyUserRepository(session=session),
        event_bus=request.app.state.di_container.resolve("event_bus"),
        unit_of_work=SQLAlchemyUnitOfWork(session=session),
    )


async def get_ledger_use_cases(
    request: Request,
    session: AsyncSession = Depends(get_session),
) -> LedgerUseCases:
    """Отримати LedgerUseCases з поточною сесією БД."""
    return LedgerUseCases(
        ledger_repo=SQLAlchemyLedgerRepository(session=session),
        event_bus=request.app.state.di_container.resolve("event_bus"),
        unit_of_work=SQLAlchemyUnitOfWork(session=session),
    )


async def get_category_repository(
    request: Request,
    session: AsyncSession = Depends(get_session),
) -> ICategoryRepository:
    """Отримати CategoryRepository з поточною сесією БД."""
    return SQLAlchemyCategoryRepository(session=session)


async def get_cache_service(request: Request) -> ICacheService:
    """Отримати ICacheService з DI контейнера.

    Повертає RedisCacheService, зареєстрований як singleton.
    Якщо Redis недоступний, повертає NullCacheService (без кешу).
    """
    return request.app.state.di_container.resolve("cache_service")


async def get_prro_use_cases(
    session: AsyncSession = Depends(get_session),
) -> PrroUseCases:
    """
    Отримати PrroUseCases (фасад ПРРО) з поточною сесією БД.

    Будує фасад безпосередньо через build_prro_use_cases(session) —
    компоненти ПРРО (key_store, gRPC-фабрика) є singleton на процес,
    а репозиторії прив'язані до per-request сесії.
    """
    return build_prro_use_cases(session)
