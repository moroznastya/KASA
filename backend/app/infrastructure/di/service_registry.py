"""
Infrastructure Layer: Service Registry — реєстрація всіх сервісів.

Центральне місце для реєстрації всіх залежностей в DI Container.
Викликається при старті застосунку в lifespan.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING

from app.application.event_handlers import (
    AuditHandler,
    CacheInvalidationHandler,
    LoggingHandler,
)

# ─── Application Services ────────────────────────────────────────────────────
from app.application.services.settings_service import SettingsService
from app.application.use_cases.auth_use_cases import AuthUseCases
from app.application.use_cases.invoice_use_cases import InvoiceUseCases
from app.application.use_cases.ledger_use_cases import LedgerUseCases

# ─── Application Use Cases ───────────────────────────────────────────────────
from app.application.use_cases.product_use_cases import ProductUseCases
from app.application.use_cases.receipt_use_cases import ReceiptUseCases

# ─── Event Handlers ──────────────────────────────────────────────────────────
from app.domain.events import BaseDomainEvent
from app.domain.services.auth_service import AuthService
from app.domain.services.document_service import DocumentService
from app.domain.services.ledger_service import LedgerService
from app.domain.services.pricing_service import PricingService

# ─── Services (старі, для зворотної сумісності) ─────────────────────────────
from app.domain.services.product_service import ProductService

# ─── Domain Services ─────────────────────────────────────────────────────────
from app.domain.services.stock_service import StockService

# ─── Cache ───────────────────────────────────────────────────────────────────
from app.infrastructure.cache import RedisCacheService
from app.infrastructure.cache.memory_cache import MemoryCacheService
from app.infrastructure.cache.redis_cache import REDIS_AVAILABLE

# ─── ПРРО (програмний РРО) ───────────────────────────────────────────────────
from app.infrastructure.di.prro import (
    build_fiscalize_use_case,
    get_prro_key_store,
    get_prro_service_factory,
)

# ─── Event Bus ───────────────────────────────────────────────────────────────
from app.infrastructure.event_bus import LocalEventBus

# ─── Repository Implementations & Unit of Work ──────────────────────────────
from app.infrastructure.persistence.repositories import (
    SQLAlchemyCategoryRepository,
    SQLAlchemyInvoiceRepository,
    SQLAlchemyLedgerRepository,
    SQLAlchemyProductRepository,
    SQLAlchemyReceiptRepository,
    SQLAlchemySupplierRepository,
    SQLAlchemyUnitOfWork,
    SQLAlchemyUserRepository,
)

if TYPE_CHECKING:
    from app.infrastructure.di.container import DIContainer

logger = logging.getLogger(__name__)


def _make_fiscalizer_factory():
    """
    Фабрика FiscalizeReceiptUseCase для авто-фіскалізації після створення чеку.

    Створює нову сесію БД та fiscalizer на неї. Використовується
    у ReceiptUseCases (fiscalizer_factory) — викликається per-receipt.
    """
    from app.database import async_session

    def _factory():
        return build_fiscalize_use_case(async_session())

    return _factory


def register_all_services(container: DIContainer) -> None:
    """
    Реєструє всі сервіси в DI Container.

    Викликається при старті застосунку.
    Порядок реєстрації важливий: спочатку базові компоненти,
    потім ті, що від них залежать.

    Args:
        container: Екземпляр DIContainer для реєстрації.
    """

    # ═══════════════════════════════════════════════════════════════════════
    # 1. Інфраструктурні компоненти (без залежностей)
    # ═══════════════════════════════════════════════════════════════════════

    # Event Bus — singleton, один на весь застосунок
    event_bus = LocalEventBus()
    container.register_instance("event_bus", event_bus)

    # Cache Service — singleton
    # Якщо redis.asyncio доступний — Redis, інакше in-memory TTL-кеш (fallback)
    from app.config import settings as app_settings
    if REDIS_AVAILABLE:
        cache_service = RedisCacheService(
            url=app_settings.REDIS_ACTUAL_URL,
            default_ttl=app_settings.CACHE_TTL_DEFAULT,
        )
    else:
        logger.warning("⚠️ redis.asyncio недоступний — використовується MemoryCacheService (in-memory TTL)")
        cache_service = MemoryCacheService(default_ttl=app_settings.CACHE_TTL_DEFAULT)
    container.register_instance("cache_service", cache_service)

    # ПРРО — singleton компоненти (gRPC-канали, сховище ключів)
    container.register_instance("prro_service_factory", get_prro_service_factory())
    container.register_instance("prro_key_store", get_prro_key_store())

    # ═══════════════════════════════════════════════════════════════════════
    # 2. Repository Implementations (transient)
    # ═══════════════════════════════════════════════════════════════════════

    container.register("product_repository", lambda c: SQLAlchemyProductRepository(), singleton=False)
    container.register("invoice_repository", lambda c: SQLAlchemyInvoiceRepository(), singleton=False)
    container.register("receipt_repository", lambda c: SQLAlchemyReceiptRepository(), singleton=False)
    container.register("user_repository", lambda c: SQLAlchemyUserRepository(), singleton=False)
    container.register("supplier_repository", lambda c: SQLAlchemySupplierRepository(), singleton=False)
    container.register("category_repository", lambda c: SQLAlchemyCategoryRepository(), singleton=False)
    container.register("ledger_repository", lambda c: SQLAlchemyLedgerRepository(), singleton=False)

    # ═══════════════════════════════════════════════════════════════════════
    # 3. Unit of Work (transient)
    # ═══════════════════════════════════════════════════════════════════════

    container.register("unit_of_work", lambda c: SQLAlchemyUnitOfWork(), singleton=False)

    # ═══════════════════════════════════════════════════════════════════════
    # 4. Domain Services (transient)
    # ═══════════════════════════════════════════════════════════════════════

    container.register("stock_domain_service", lambda c: StockService(), singleton=False)
    container.register("pricing_domain_service", lambda c: PricingService(), singleton=False)

    # ═══════════════════════════════════════════════════════════════════════
    # 5. Application Use Cases (transient)
    # ═══════════════════════════════════════════════════════════════════════

    container.register(
        "product_use_cases",
        lambda c: ProductUseCases(
            product_repo=c.resolve("product_repository"),
            event_bus=c.resolve("event_bus"),
            unit_of_work=c.resolve("unit_of_work"),
        ),
        singleton=False,
    )
    container.register(
        "invoice_use_cases",
        lambda c: InvoiceUseCases(
            invoice_repo=c.resolve("invoice_repository"),
            event_bus=c.resolve("event_bus"),
            unit_of_work=c.resolve("unit_of_work"),
        ),
        singleton=False,
    )
    container.register(
        "receipt_use_cases",
        lambda c: ReceiptUseCases(
            receipt_repo=c.resolve("receipt_repository"),
            product_repo=c.resolve("product_repository"),
            event_bus=c.resolve("event_bus"),
            unit_of_work=c.resolve("unit_of_work"),
            fiscalizer_factory=_make_fiscalizer_factory(),
        ),
        singleton=False,
    )
    container.register(
        "ledger_use_cases",
        lambda c: LedgerUseCases(
            ledger_repo=c.resolve("ledger_repository"),
            event_bus=c.resolve("event_bus"),
            unit_of_work=c.resolve("unit_of_work"),
        ),
        singleton=False,
    )
    container.register(
        "auth_use_cases",
        lambda c: AuthUseCases(
            user_repo=c.resolve("user_repository"),
            event_bus=c.resolve("event_bus"),
            unit_of_work=c.resolve("unit_of_work"),
        ),
        singleton=False,
    )

    # ═══════════════════════════════════════════════════════════════════════
    # 6. Application Services (transient)
    # ═══════════════════════════════════════════════════════════════════════

    container.register(
        "settings_service",
        lambda c: SettingsService(session=None),  # session буде передано через dependency
        singleton=False,
    )

    # ═══════════════════════════════════════════════════════════════════════
    # 7. Сервіси (старі, для зворотної сумісності з API v1)
    # ═══════════════════════════════════════════════════════════════════════

    container.register("product_service", lambda c: ProductService(session=None), singleton=False)
    container.register("document_service", lambda c: DocumentService(session=None), singleton=False)
    container.register("ledger_service", lambda c: LedgerService(session=None), singleton=False)
    container.register("auth_service", lambda c: AuthService(session=None), singleton=False)

    # ═══════════════════════════════════════════════════════════════════════
    # 8. Event Handlers — підписка на події
    # ═══════════════════════════════════════════════════════════════════════

    logging_handler = LoggingHandler()
    cache_handler = CacheInvalidationHandler(cache_service=cache_service)
    audit_handler = AuditHandler()

    event_bus.subscribe(BaseDomainEvent, logging_handler.handle)
    event_bus.subscribe(BaseDomainEvent, cache_handler.handle)
    event_bus.subscribe(BaseDomainEvent, audit_handler.handle)

    logger.info("✅ Event Handlers зареєстровано")
    logger.info(
        f"✅ Cache Service зареєстровано: {app_settings.REDIS_ACTUAL_URL}"
    )
    logger.info(
        f"Registered {len(container.registered_services)} services in DI Container"
    )
