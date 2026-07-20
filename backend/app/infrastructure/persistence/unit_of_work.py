"""
Infrastructure Layer: SQLAlchemyUnitOfWork — реалізація IUnitOfWork.

Забезпечує атомарність транзакцій через SQLAlchemy session.
Використовується як контекстний менеджер.
"""

from __future__ import annotations

import logging
from typing import Optional

from sqlalchemy.ext.asyncio import AsyncSession

from app.database import async_session
from app.domain.repositories.i_unit_of_work import IUnitOfWork

logger = logging.getLogger(__name__)


class SQLAlchemyUnitOfWork(IUnitOfWork):
    """
    Unit of Work для SQLAlchemy.

    Реалізує IUnitOfWork використовуючи асинхронну сесію SQLAlchemy.
    Підтримує використання як контекстний менеджер (async with).

    Приклад використання:
        async with SQLAlchemyUnitOfWork() as uow:
            repo = ProductRepository()
            repo.set_session(uow.session)
            await repo.save(product)
            await uow.commit()
    """

    def __init__(self, session: Optional[AsyncSession] = None) -> None:
        """
        Ініціалізує Unit of Work.

        Args:
            session: Існуюча сесія (опціонально).
                     Якщо не передана — створюється нова.
        """
        self._session: Optional[AsyncSession] = session
        self._owns_session: bool = session is None
        self._active: bool = False

    @property
    def session(self) -> AsyncSession:
        """Поточна сесія БД."""
        if self._session is None:
            raise RuntimeError("Session not initialized. Use 'async with' block.")
        return self._session

    @property
    def is_active(self) -> bool:
        """Чи активна транзакція."""
        return self._active

    # ─── Контекстний менеджер ──────────────────────────────────────────────

    async def __aenter__(self) -> IUnitOfWork:
        """
        Вхід в контекстний менеджер.

        Створює нову сесію (якщо не передана) та починає транзакцію.

        Returns:
            Екземпляр Unit of Work.
        """
        if self._session is None:
            self._session = async_session()
        self._active = True
        logger.debug("Unit of Work started")
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: object | None,
    ) -> None:
        """
        Вихід з контекстного менеджера.

        Якщо була помилка — виконує rollback.
        Якщо сесія була створена цим UoW — закриває її.
        """
        try:
            if exc_type is not None:
                # Була помилка — відкочуємо транзакцію
                await self.rollback()
                logger.error(
                    f"Unit of Work rolled back due to {exc_type.__name__}: {exc_val}"
                )
            else:
                # Якщо не було явного commit — робимо rollback
                if self._active:
                    logger.warning(
                        "Unit of Work exited without explicit commit — rolling back"
                    )
                    await self.rollback()
        finally:
            self._active = False
            # Закриваємо сесію, якщо вона була створена цим UoW
            if self._owns_session and self._session is not None:
                await self._session.close()
                logger.debug("Unit of Work session closed")

    # ─── Управління транзакцією ────────────────────────────────────────────

    async def begin(self) -> None:
        """
        Починає нову транзакцію.

        Викликається автоматично при вході в контекстний менеджер.
        """
        if self._session is None:
            self._session = async_session()
        self._active = True
        logger.debug("Transaction started")

    async def commit(self) -> None:
        """
        Фіксує всі зміни в межах транзакції.

        Raises:
            Exception: Якщо помилка при коміті.
        """
        if not self._active:
            raise RuntimeError("No active transaction to commit")

        try:
            await self.session.commit()
            self._active = False
            logger.debug("Transaction committed")
        except Exception as e:
            logger.error(f"Commit failed: {e}")
            await self.rollback()
            raise

    async def rollback(self) -> None:
        """
        Відкочує всі зміни в межах транзакції.
        """
        if self._session is not None:
            try:
                await self.session.rollback()
                logger.debug("Transaction rolled back")
            except Exception as e:
                logger.error(f"Rollback failed: {e}")
        self._active = False

    async def flush(self) -> None:
        """
        Примусове виконання SQL-запитів без коміту.

        Корисно для отримання згенерованих ID.
        """
        if self._session is not None:
            await self.session.flush()
