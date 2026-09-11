"""
Repository Implementation: SQLAlchemyUnitOfWork.

Реалізація IUnitOfWork з використанням SQLAlchemy async session.
Забезпечує атомарність транзакцій та доступ до всіх репозиторіїв.
"""

from __future__ import annotations

from typing import Optional

from sqlalchemy.ext.asyncio import (
    AsyncSession,
    async_sessionmaker,
)

from app.domain.repositories import IUnitOfWork

from .category_repository import SQLAlchemyCategoryRepository
from .invoice_repository import SQLAlchemyInvoiceRepository
from .ledger_repository import SQLAlchemyLedgerRepository
from .product_repository import SQLAlchemyProductRepository
from .receipt_repository import SQLAlchemyReceiptRepository
from .supplier_repository import SQLAlchemySupplierRepository
from .user_repository import SQLAlchemyUserRepository


class SQLAlchemyUnitOfWork(IUnitOfWork):
    """
    SQLAlchemy реалізація Unit of Work.

    Використання:
        async with unit_of_work as uow:
            uow.products.save(product)
            uow.invoices.save(invoice)
            await uow.commit()
    """

    def __init__(self, session_factory: async_sessionmaker[AsyncSession]):
        """
        Ініціалізація UoW.

        Args:
            session_factory: Фабрика асинхронних сесій SQLAlchemy.
        """
        self._session_factory = session_factory
        self._session: Optional[AsyncSession] = None

        # Репозиторії (ініціалізуються пізніше)
        self._products: Optional[SQLAlchemyProductRepository] = None
        self._invoices: Optional[SQLAlchemyInvoiceRepository] = None
        self._receipts: Optional[SQLAlchemyReceiptRepository] = None
        self._categories: Optional[SQLAlchemyCategoryRepository] = None
        self._ledger: Optional[SQLAlchemyLedgerRepository] = None
        self._suppliers: Optional[SQLAlchemySupplierRepository] = None
        self._users: Optional[SQLAlchemyUserRepository] = None

    # ── Властивості для доступу до репозиторіїв ──

    @property
    def products(self) -> SQLAlchemyProductRepository:
        """Репозиторій товарів."""
        if self._products is None:
            self._products = SQLAlchemyProductRepository(self._session)
        return self._products

    @property
    def invoices(self) -> SQLAlchemyInvoiceRepository:
        """Репозиторій прибуткових накладних."""
        if self._invoices is None:
            self._invoices = SQLAlchemyInvoiceRepository(self._session)
        return self._invoices

    @property
    def receipts(self) -> SQLAlchemyReceiptRepository:
        """Репозиторій чеків продажу."""
        if self._receipts is None:
            self._receipts = SQLAlchemyReceiptRepository(self._session)
        return self._receipts

    @property
    def categories(self) -> SQLAlchemyCategoryRepository:
        """Репозиторій категорій."""
        if self._categories is None:
            self._categories = SQLAlchemyCategoryRepository(self._session)
        return self._categories

    @property
    def ledger(self) -> SQLAlchemyLedgerRepository:
        """Репозиторій журналу взаєморозрахунків."""
        if self._ledger is None:
            self._ledger = SQLAlchemyLedgerRepository(self._session)
        return self._ledger

    @property
    def suppliers(self) -> SQLAlchemySupplierRepository:
        """Репозиторій постачальників."""
        if self._suppliers is None:
            self._suppliers = SQLAlchemySupplierRepository(self._session)
        return self._suppliers

    @property
    def users(self) -> SQLAlchemyUserRepository:
        """Репозиторій користувачів."""
        if self._users is None:
            self._users = SQLAlchemyUserRepository(self._session)
        return self._users

    # ── Управління транзакціями ──

    @property
    def is_active(self) -> bool:
        """Чи активна транзакція."""
        return self._session is not None and self._session.is_active

    async def commit(self) -> None:
        """Фіксує всі зміни в межах транзакції."""
        if self._session is not None:
            await self._session.commit()

    async def rollback(self) -> None:
        """Відкочує всі зміни в межах транзакції."""
        if self._session is not None:
            await self._session.rollback()

    # ── Контекстний менеджер ──

    async def __aenter__(self) -> SQLAlchemyUnitOfWork:
        """
        Вхід в контекстний менеджер.

        Створює нову сесію та починає транзакцію.
        """
        self._session = self._session_factory()
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: object | None,
    ) -> None:
        """
        Вихід з контекстного менеджера.

        При помилці — rollback, інакше — commit.
        Закриває сесію в будь-якому випадку.
        """
        try:
            if exc_type is not None:
                # Була помилка — відкочуємо
                await self.rollback()
            else:
                # Все добре — автоматично комітимо
                await self.commit()
        finally:
            # Завжди закриваємо сесію
            if self._session is not None:
                await self._session.close()
                self._session = None
