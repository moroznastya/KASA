"""Unit tests: інтеграція авто-фіскалізації у ReceiptUseCases."""

from __future__ import annotations

from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.dto.receipt_dto import ReceiptCreateDTO, ReceiptItemDTO
from app.application.use_cases.receipt_use_cases import ReceiptUseCases
from app.domain.entities.product import Product
from app.domain.entities.receipt import Receipt
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity


def _build_use_cases(*, fiscalizer_factory=None) -> ReceiptUseCases:
    """ReceiptUseCases з моками."""
    receipt_repo = AsyncMock()
    product_repo = AsyncMock()
    uow = MagicMock()

    # UoW як асинхронний контекстний менеджер
    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        return False

    uow.__aenter__ = __aenter__
    uow.__aexit__ = __aexit__
    uow.commit = AsyncMock()

    event_bus = AsyncMock()

    product = Product(
        id=uuid4(),
        name="Товар",
        price=Money(Decimal("100")),
        stock=Quantity(Decimal("50")),
        unit="шт",
    )
    product_repo.find_by_id.return_value = product
    product_repo.update = AsyncMock()

    saved = Receipt(id=uuid4(), number="R-1", total=Money(Decimal("300")))
    receipt_repo.save = AsyncMock(return_value=saved)

    return ReceiptUseCases(
        receipt_repo=receipt_repo,
        product_repo=product_repo,
        unit_of_work=uow,
        event_bus=event_bus,
        fiscalizer_factory=fiscalizer_factory,
    )


def _make_dto() -> ReceiptCreateDTO:
    return ReceiptCreateDTO(
        items=[
            ReceiptItemDTO(
                product_id=uuid4(),
                name="Товар",
                quantity=Decimal("3"),
                price=Decimal("100"),
                tax_rate=20,
            )
        ],
        payment_method="cash",
        is_fiscal=True,
    )


class TestAutoFiscalizeIntegration:
    async def test_fiscalizer_called_after_sale(self):
        """Після створення чеку викликається fiscalize_receipt(manual=False)."""
        fiscalizer = AsyncMock()
        fiscalizer.fiscalize_receipt = AsyncMock(return_value=None)
        use_cases = _build_use_cases(
            fiscalizer_factory=lambda: fiscalizer
        )

        dto = await use_cases.create_sale_receipt(_make_dto())

        assert dto.id is not None
        fiscalizer.fiscalize_receipt.assert_awaited_once()
        assert fiscalizer.fiscalize_receipt.call_args.args[0] == dto.id
        assert fiscalizer.fiscalize_receipt.call_args.kwargs["manual"] is False

    async def test_fiscalizer_error_does_not_block_sale(self):
        """Помилка фіскалізації не блокує продаж (try/except)."""
        fiscalizer = AsyncMock()
        fiscalizer.fiscalize_receipt = AsyncMock(
            side_effect=RuntimeError("ПРРО недоступний")
        )
        use_cases = _build_use_cases(
            fiscalizer_factory=lambda: fiscalizer
        )

        # Продаж має завершитись успішно, попри помилку ПРРО
        dto = await use_cases.create_sale_receipt(_make_dto())
        assert dto.id is not None

    async def test_no_fiscalizer_no_call(self):
        """Без fiscalizer_factory фіскалізація не викликається."""
        use_cases = _build_use_cases(fiscalizer_factory=None)

        dto = await use_cases.create_sale_receipt(_make_dto())
        assert dto.id is not None

    async def test_fiscalizer_called_after_return(self):
        """Після повернення також викликається фіскалізація."""
        fiscalizer = AsyncMock()
        fiscalizer.fiscalize_receipt = AsyncMock(return_value=None)
        use_cases = _build_use_cases(
            fiscalizer_factory=lambda: fiscalizer
        )

        dto = await use_cases.create_return_receipt(_make_dto())

        assert dto.id is not None
        fiscalizer.fiscalize_receipt.assert_awaited_once()


class TestBackgroundFiscalize:
    """Авто-фіскалізація у фоні (BackgroundTasks) — підзадача C."""

    async def test_with_background_tasks_defers_fiscalize(self):
        """З BackgroundTasks fiscalize НЕ викликається синхронно — ставиться у фон."""
        fiscalizer = AsyncMock()
        fiscalizer.fiscalize_receipt = AsyncMock(return_value=None)
        # Mock зовнішнього фіскалізатора має властивість session (для close)
        session_mock = AsyncMock()
        fiscalizer.session = session_mock

        # Мок BackgroundTasks (FastAPI/Starlette)
        background_tasks = MagicMock()

        use_cases = _build_use_cases(
            fiscalizer_factory=lambda: fiscalizer
        )

        dto = await use_cases.create_sale_receipt(
            _make_dto(), background_tasks=background_tasks
        )

        # HTTP-відповідь повернулась одразу
        assert dto.id is not None
        # fiscalize НЕ викликався під час запиту
        fiscalizer.fiscalize_receipt.assert_not_awaited()
        # Задача зареєстрована у фоні (callable + аргумент receipt_id)
        background_tasks.add_task.assert_called_once()
        args = background_tasks.add_task.call_args.args
        assert callable(args[0])
        assert args[1] == dto.id

    async def test_background_task_runs_fiscalize(self):
        """Запуск фонової задачі виконує фіскалізацію та закриває сесію."""
        fiscalizer = AsyncMock()
        fiscalizer.fiscalize_receipt = AsyncMock(return_value=None)
        session_mock = AsyncMock()
        fiscalizer.session = session_mock

        background_tasks = MagicMock()
        use_cases = _build_use_cases(
            fiscalizer_factory=lambda: fiscalizer
        )

        dto = await use_cases.create_sale_receipt(
            _make_dto(), background_tasks=background_tasks
        )

        # Імітуємо виконання фонової задачі FastAPI
        task = background_tasks.add_task.call_args.args[0]
        await task(dto.id)

        fiscalizer.fiscalize_receipt.assert_awaited_once_with(dto.id, manual=False)
        session_mock.close.assert_awaited_once()

    async def test_background_task_error_does_not_break(self):
        """Помилка ПРРО у фоні не кидає виключення назовні."""
        fiscalizer = AsyncMock()
        fiscalizer.fiscalize_receipt = AsyncMock(
            side_effect=RuntimeError("ПРРО недоступний")
        )
        session_mock = AsyncMock()
        fiscalizer.session = session_mock

        background_tasks = MagicMock()
        use_cases = _build_use_cases(
            fiscalizer_factory=lambda: fiscalizer
        )

        dto = await use_cases.create_sale_receipt(
            _make_dto(), background_tasks=background_tasks
        )

        task = background_tasks.add_task.call_args.args[0]
        # Фонова задача не має кидати виключення (логуються всередині)
        await task(dto.id)
        session_mock.close.assert_awaited_once()

    async def test_sync_fallback_without_background_tasks(self):
        """Без background_tasks фіскалізація виконується синхронно (fallback)."""
        fiscalizer = AsyncMock()
        fiscalizer.fiscalize_receipt = AsyncMock(return_value=None)
        session_mock = AsyncMock()
        fiscalizer.session = session_mock

        use_cases = _build_use_cases(
            fiscalizer_factory=lambda: fiscalizer
        )

        dto = await use_cases.create_return_receipt(_make_dto())

        assert dto.id is not None
        fiscalizer.fiscalize_receipt.assert_awaited_once_with(dto.id, manual=False)
        # Сесія закривається і при синхронному шляху
        session_mock.close.assert_awaited_once()
