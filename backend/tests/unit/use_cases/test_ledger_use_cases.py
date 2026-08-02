"""Unit tests: LedgerUseCases (app/application/use_cases/ledger_use_cases.py).

Покриває:
- create_entry — успіх (з розрахунком балансу), постачальника не знайдено
- get_ledger_history — з фільтрами та без
- get_supplier_balance — успіх, постачальника не знайдено
- get_all_balances
"""

from __future__ import annotations

from decimal import Decimal
from unittest.mock import AsyncMock
from uuid import uuid4

import pytest

from app.application.dto.ledger_dto import LedgerCreateDTO
from app.application.use_cases.ledger_use_cases import LedgerUseCases
from app.domain.entities.ledger_entry import LedgerEntry, OperationType
from app.domain.entities.supplier import Supplier
from app.domain.value_objects.money import Money


def _make_supplier() -> Supplier:
    return Supplier(id=uuid4(), name="Постачальник")


def _make_entry(
    *,
    supplier_id=None,
    amount: Decimal = Decimal("1000.00"),
    operation_type: OperationType = OperationType.INVOICE,
) -> LedgerEntry:
    return LedgerEntry(
        id=uuid4(),
        supplier_id=supplier_id or uuid4(),
        amount=Money(amount),
        operation_type=operation_type,
        balance_after=Money(Decimal("1000.00")),
    )


def _build_use_cases(
    *,
    ledger_repo: AsyncMock | None = None,
    supplier_repo: AsyncMock | None = None,
    uow: AsyncMock | None = None,
    event_bus: AsyncMock | None = None,
) -> LedgerUseCases:
    return LedgerUseCases(
        ledger_repo=ledger_repo or AsyncMock(),
        supplier_repo=supplier_repo or AsyncMock(),
        unit_of_work=uow or AsyncMock(),
        event_bus=event_bus or AsyncMock(),
    )


class TestCreateEntry:
    """Тести створення запису в журналі."""

    @pytest.mark.asyncio
    async def test_create_entry_success(self):
        """Успішне створення запису з розрахунком балансу."""
        supplier = _make_supplier()
        saved = _make_entry(supplier_id=supplier.id, amount=Decimal("1000.00"))

        ledger_repo = AsyncMock()
        ledger_repo.get_supplier_balance.return_value = Decimal("500.00")
        ledger_repo.save.return_value = saved
        supplier_repo = AsyncMock()
        supplier_repo.find_by_id.return_value = supplier
        uow = AsyncMock()
        event_bus = AsyncMock()

        uc = _build_use_cases(
            ledger_repo=ledger_repo,
            supplier_repo=supplier_repo,
            uow=uow,
            event_bus=event_bus,
        )
        dto = await uc.create_entry(
            LedgerCreateDTO(
                supplier_id=supplier.id,
                amount=Decimal("1000.00"),
                operation_type="invoice",
            )
        )

        assert dto.supplier_id == supplier.id
        assert dto.amount == 1000.00
        ledger_repo.save.assert_awaited_once()
        uow.commit.assert_awaited_once()
        # баланс після операції: поточний (500) + сума (1000) = 1500
        saved_entity = ledger_repo.save.call_args.args[0]
        assert saved_entity.balance_after.amount == Decimal("1500.00")
        published = event_bus.publish.call_args.args[0]
        assert published.entry_type == "invoice"

    @pytest.mark.asyncio
    async def test_create_entry_supplier_not_found_raises(self):
        """Помилка якщо постачальника не знайдено."""
        ledger_repo = AsyncMock()
        supplier_repo = AsyncMock()
        supplier_repo.find_by_id.return_value = None

        uc = _build_use_cases(ledger_repo=ledger_repo, supplier_repo=supplier_repo)
        with pytest.raises(ValueError, match=r"Постачальника.*не знайдено"):
            await uc.create_entry(
                LedgerCreateDTO(
                    supplier_id=uuid4(),
                    amount=Decimal("100.00"),
                )
            )
        ledger_repo.save.assert_not_awaited()


class TestGetLedgerHistory:
    """Тести історії операцій."""

    @pytest.mark.asyncio
    async def test_get_ledger_history_with_filters(self):
        """Історія з фільтрами."""
        entries = [_make_entry(), _make_entry()]
        ledger_repo = AsyncMock()
        ledger_repo.search.return_value = (entries, 2)

        uc = _build_use_cases(ledger_repo=ledger_repo)
        dtos, total = await uc.get_ledger_history(
            supplier_id=uuid4(),
            operation_type="invoice",
            page=1,
            size=10,
        )

        assert total == 2
        assert len(dtos) == 2
        assert all(d.operation_type == "invoice" for d in dtos)

    @pytest.mark.asyncio
    async def test_get_ledger_history_no_filters(self):
        """Історія без фільтрів."""
        ledger_repo = AsyncMock()
        ledger_repo.search.return_value = ([], 0)

        uc = _build_use_cases(ledger_repo=ledger_repo)
        dtos, total = await uc.get_ledger_history()

        assert total == 0
        assert dtos == []
        assert ledger_repo.search.call_args.kwargs["operation_type"] is None

    @pytest.mark.asyncio
    async def test_get_ledger_history_invalid_operation_type_raises(self):
        """Помилка при невалідному типі операції."""
        ledger_repo = AsyncMock()
        uc = _build_use_cases(ledger_repo=ledger_repo)

        with pytest.raises(ValueError):
            await uc.get_ledger_history(operation_type="not-a-real-type")


class TestGetSupplierBalance:
    """Тести отримання балансу постачальника."""

    @pytest.mark.asyncio
    async def test_get_supplier_balance_success(self):
        """Успішне отримання балансу."""
        supplier = _make_supplier()
        supplier_repo = AsyncMock()
        supplier_repo.find_by_id.return_value = supplier
        ledger_repo = AsyncMock()
        ledger_repo.get_supplier_balance.return_value = 1234.56

        uc = _build_use_cases(ledger_repo=ledger_repo, supplier_repo=supplier_repo)
        balance = await uc.get_supplier_balance(supplier.id)

        assert balance == 1234.56
        ledger_repo.get_supplier_balance.assert_awaited_once_with(supplier.id)

    @pytest.mark.asyncio
    async def test_get_supplier_balance_supplier_not_found_raises(self):
        """Помилка якщо постачальника не знайдено."""
        supplier_repo = AsyncMock()
        supplier_repo.find_by_id.return_value = None

        uc = _build_use_cases(supplier_repo=supplier_repo)
        with pytest.raises(ValueError, match=r"Постачальника.*не знайдено"):
            await uc.get_supplier_balance(uuid4())


class TestGetAllBalances:
    """Тести отримання балансів усіх постачальників."""

    @pytest.mark.asyncio
    async def test_get_all_balances(self):
        """Успішне отримання балансів усіх постачальників."""
        ledger_repo = AsyncMock()
        ledger_repo.get_all_supplier_balances.return_value = [
            {"supplier_id": uuid4(), "supplier_name": "Постачальник", "balance": 100.0},
        ]

        uc = _build_use_cases(ledger_repo=ledger_repo)
        result = await uc.get_all_balances()

        assert len(result) == 1
        assert result[0]["balance"] == 100.0
