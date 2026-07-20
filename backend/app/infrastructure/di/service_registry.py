"""
Infrastructure Layer: Service Registry — реєстрація всіх сервісів.

Центральне місце для реєстрації всіх залежностей в DI Container.
Викликається при старті застосунку в lifespan.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING

# ─── Event Bus ───────────────────────────────────────────────────────────────
from app.infrastructure.event_bus import LocalEventBus

# ─── Repository Implementations ──────────────────────────────────────────────
from app.infrastructure.persistence.repositories import (
    ProductRepository,
    InvoiceRepository,
    ReceiptRepository,
    UserRepository,
    SupplierRepository,
    CategoryRepository,
    LedgerRepository,
)

# ─── Unit of Work ────────────────────────────────────────────────────────────
from app.infrastructure.persistence.unit_of_work import SQLAlchemyUnitOfWork

# ─── Domain Services ─────────────────────────────────────────────────────────
from app.domain.services.stock_service import StockService
from app.domain.services.pricing_service import PricingService

# ─── Application Use Cases ───────────────────────────────────────────────────
from app.application.use_cases.product_use_cases import ProductUseCases
from app.application.use_cases.invoice_use_cases import InvoiceUseCases
from app.application.use_cases.receipt_use_cases import ReceiptUseCases
from app.application.use_cases.ledger_use_cases import LedgerUseCases
from app.application.use_cases.auth_use_cases import AuthUseCases

# ─── Services (старі, для зворотної сумісності) ─────────────────────────────
from app.services.product_service import ProductService
from app.services.document_service import DocumentService
from app.services.ledger_service import LedgerService
from app.services.auth_service import AuthService

if TYPE_CHECKING:
    from app.infrastructure.di.container import DIContainer

logger = logging.getLogger(__name__)


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

    # ═══════════════════════════════════════════════════════════════════════
    # 2. Repository Implementations (transient)
    # ═══════════════════════════════════════════════════════════════════════

    container.register("product_repository", lambda c: ProductRepository(), singleton=False)
    container.register("invoice_repository", lambda c: InvoiceRepository(), singleton=False)
    container.register("receipt_repository", lambda c: ReceiptRepository(), singleton=False)
    container.register("user_repository", lambda c: UserRepository(), singleton=False)
    container.register("supplier_repository", lambda c: SupplierRepository(), singleton=False)
    container.register("category_repository", lambda c: CategoryRepository(), singleton=False)
    container.register("ledger_repository", lambda c: LedgerRepository(), singleton=False)

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
            event_bus=c.resolve("event_bus"),
            unit_of_work=c.resolve("unit_of_work"),
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
    # 6. Сервіси (старі, для зворотної сумісності з API v1)
    # ═══════════════════════════════════════════════════════════════════════

    container.register("product_service", lambda c: ProductService(session=None), singleton=False)
    container.register("document_service", lambda c: DocumentService(session=None), singleton=False)
    container.register("ledger_service", lambda c: LedgerService(session=None), singleton=False)
    container.register("auth_service", lambda c: AuthService(session=None), singleton=False)

    logger.info(
        f"Registered {len(container.registered_services)} services in DI Container"
    )
