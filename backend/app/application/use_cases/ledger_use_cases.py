"""
Use Cases для Ledger (Журнал взаєморозрахунків).

Реалізує бізнес-логіку для роботи з журналом взаєморозрахунків:
- CreateLedgerEntry: створення запису в журналі
- GetLedgerHistory: отримання історії операцій
- GetBalance: отримання балансу постачальника
"""

from __future__ import annotations

from datetime import datetime
from typing import Optional
from uuid import UUID

from app.domain.entities.ledger_entry import OperationType
from app.domain.repositories import ILedgerRepository, ISupplierRepository
from app.domain.repositories.i_unit_of_work import IUnitOfWork
from app.application.dto.ledger_dto import LedgerEntryDTO, LedgerCreateDTO
from app.application.mappers.ledger_mapper import LedgerMapper
from app.application.interfaces.i_event_bus import IEventBus


class LedgerUseCases:
    """
    Use Cases для журналу взаєморозрахунків.

    Використовує Dependency Injection через конструктор.
    Залежності: ILedgerRepository, ISupplierRepository, IUnitOfWork, IEventBus.
    """

    def __init__(
        self,
        ledger_repo: ILedgerRepository,
        supplier_repo: ISupplierRepository,
        unit_of_work: IUnitOfWork,
        event_bus: IEventBus,
    ):
        """
        Ініціалізація Use Cases.

        Args:
            ledger_repo: Репозиторій журналу взаєморозрахунків.
            supplier_repo: Репозиторій постачальників.
            unit_of_work: Unit of Work для транзакцій.
            event_bus: Event Bus для публікації подій.
        """
        self._ledger_repo = ledger_repo
        self._supplier_repo = supplier_repo
        self._uow = unit_of_work
        self._event_bus = event_bus

    async def create_entry(self, dto: LedgerCreateDTO) -> LedgerEntryDTO:
        """
        Створює новий запис у журналі взаєморозрахунків.

        Args:
            dto: DTO з даними для створення запису.

        Returns:
            LedgerEntryDTO створеного запису.

        Raises:
            ValueError: Якщо постачальника не знайдено.
        """
        # Перевіряємо існування постачальника
        supplier = await self._supplier_repo.find_by_id(dto.supplier_id)
        if not supplier:
            raise ValueError(f"Постачальника з ID '{dto.supplier_id}' не знайдено")

        # Конвертуємо DTO в Entity
        entry = LedgerMapper.create_dto_to_entity(dto)

        # Розраховуємо баланс після операції
        current_balance = await self._ledger_repo.get_supplier_balance(dto.supplier_id)
        from decimal import Decimal
        from app.domain.value_objects.money import Money
        entry.balance_after = Money(Decimal(str(current_balance))) + entry.amount

        async with self._uow:
            saved = await self._ledger_repo.save(entry)
            await self._uow.commit()

        return LedgerMapper.entity_to_dto(saved)

    async def get_ledger_history(
        self,
        supplier_id: Optional[UUID] = None,
        operation_type: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[LedgerEntryDTO], int]:
        """
        Отримує історію операцій з фільтрацією та пагінацією.

        Args:
            supplier_id: Фільтр за постачальником.
            operation_type: Фільтр за типом операції.
            date_from: Фільтр від дати.
            date_to: Фільтр до дати.
            page: Номер сторінки.
            size: Кількість на сторінці.

        Returns:
            Кортеж (список LedgerEntryDTO, загальна кількість).
        """
        op_type = OperationType(operation_type) if operation_type else None

        entries, total = await self._ledger_repo.search(
            supplier_id=supplier_id,
            operation_type=op_type,
            date_from=date_from,
            date_to=date_to,
            page=page,
            size=size,
        )
        return [LedgerMapper.entity_to_dto(e) for e in entries], total

    async def get_supplier_balance(self, supplier_id: UUID) -> float:
        """
        Отримує поточний баланс постачальника.

        Args:
            supplier_id: UUID постачальника.

        Returns:
            Поточний баланс.

        Raises:
            ValueError: Якщо постачальника не знайдено.
        """
        supplier = await self._supplier_repo.find_by_id(supplier_id)
        if not supplier:
            raise ValueError(f"Постачальника з ID '{supplier_id}' не знайдено")

        return await self._ledger_repo.get_supplier_balance(supplier_id)

    async def get_all_balances(self) -> list[dict]:
        """
        Отримує баланси всіх постачальників.

        Returns:
            Список словників {supplier_id, supplier_name, balance, last_operation_date}.
        """
        return await self._ledger_repo.get_all_supplier_balances()
