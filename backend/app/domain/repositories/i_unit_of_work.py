"""
Repository Interface: IUnitOfWork.

Визначає контракт для Unit of Work патерну.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from typing import Protocol


class IUnitOfWork(Protocol):
    """
    Інтерфейс Unit of Work.

    Забезпечує атомарність транзакцій.
    Всі зміни в межах UoW або комітяться, або відкочуються.
    """

    async def __aenter__(self) -> IUnitOfWork:
        """
        Вхід в контекстний менеджер.

        Returns:
            Екземпляр Unit of Work.
        """
        ...

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: object | None,
    ) -> None:
        """
        Вихід з контекстного менеджера.

        Якщо була помилка — виконує rollback.
        """
        ...

    async def commit(self) -> None:
        """
        Фіксує всі зміни в межах транзакції.

        Raises:
            CommitError: Якщо помилка при коміті.
        """
        ...

    async def rollback(self) -> None:
        """
        Відкочує всі зміни в межах транзакції.
        """
        ...

    @property
    def is_active(self) -> bool:
        """
        Чи активна транзакція.

        Returns:
            True якщо транзакція активна.
        """
        ...
