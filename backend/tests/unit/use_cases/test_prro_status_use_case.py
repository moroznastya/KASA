"""Unit tests: PrroStatusUseCase (app/application/use_cases/prro/prro_status_use_case.py).

Покриває:
- get_status — сервер доступний (status + info)
- get_status — сервер недоступний (виняток) → локальний стан
- get_status — без відкритої зміни та без fn
- get_queue — з фільтром та пагінацією
- get_queue — без фільтра
- _queue_to_dto — конвертація
"""

from __future__ import annotations

from datetime import datetime
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.use_cases.prro.prro_status_use_case import PrroStatusUseCase
from app.infrastructure.persistence.models.prro import PrroQueueStatus


def _make_context(grpc_client=None):
    """Mock PrroContextFactory з AsyncMock grpc_client."""
    context = MagicMock()
    context.grpc_client = AsyncMock(return_value=grpc_client or _make_grpc_client())
    return context


def _make_grpc_client(
    *,
    open_shift: bool = True,
    online: bool = True,
    last_signer: str | None = "SIGNER-1",
    name: str | None = "Тест ПРРО",
    addr: str | None = "м. Київ",
    fn: str | None = "4000012345",
):
    client = MagicMock()
    client.status = AsyncMock(
        return_value=SimpleNamespace(
            open_shift=open_shift, online=online, last_signer=last_signer
        )
    )
    client.info = AsyncMock(
        return_value=SimpleNamespace(name=name, addr=addr, fn=fn)
    )
    return client


def _make_queue_item(
    *,
    status: PrroQueueStatus = PrroQueueStatus.PENDING,
    local_number: int = 1,
):
    return SimpleNamespace(
        id=uuid4(),
        receipt_id=uuid4(),
        shift_id=uuid4(),
        local_number=local_number,
        check_type="CHK",
        status=status,
        error=None if status == PrroQueueStatus.PENDING else "помилка",
        created_at=datetime(2026, 1, 1, 12, 0, 0),
        sent_at=datetime(2026, 1, 1, 12, 5, 0) if status == PrroQueueStatus.SENT else None,
    )


def _build_use_case(
    *,
    prro_repo: AsyncMock | None = None,
    settings_repo: AsyncMock | None = None,
    context: MagicMock | None = None,
) -> PrroStatusUseCase:
    return PrroStatusUseCase(
        prro_repo=prro_repo or AsyncMock(),
        settings_repo=settings_repo or AsyncMock(),
        context_factory=context or _make_context(),
    )


class TestGetStatus:
    """Тести отримання статусу ПРРО."""

    @pytest.mark.asyncio
    async def test_get_status_server_available(self):
        """Сервер доступний — статус з gRPC (statusRro/infoRro)."""
        prro_repo = AsyncMock()
        prro_repo.get_open_shift.return_value = SimpleNamespace(id=uuid4())
        settings_repo = AsyncMock()
        settings_repo.get.return_value = "4000012345"
        client = _make_grpc_client()
        context = _make_context(client)

        uc = _build_use_case(
            prro_repo=prro_repo, settings_repo=settings_repo, context=context
        )
        dto = await uc.get_status()

        assert dto.open_shift is True
        assert dto.online is True
        assert dto.last_signer == "SIGNER-1"
        assert dto.name == "Тест ПРРО"
        assert dto.addr == "м. Київ"
        assert dto.fn == "4000012345"
        client.status.assert_awaited_once_with(timeout=5)
        client.info.assert_awaited_once_with(timeout=5)

    @pytest.mark.asyncio
    async def test_get_status_server_unavailable(self):
        """Сервер недоступний — повертається локальний стан (online=False)."""
        prro_repo = AsyncMock()
        prro_repo.get_open_shift.return_value = SimpleNamespace(id=uuid4())
        settings_repo = AsyncMock()
        settings_repo.get.return_value = "4000012345"
        context = MagicMock()
        context.grpc_client = AsyncMock(side_effect=ConnectionError("no network"))

        uc = _build_use_case(
            prro_repo=prro_repo, settings_repo=settings_repo, context=context
        )
        dto = await uc.get_status()

        assert dto.open_shift is True  # локальна відкрита зміна
        assert dto.online is False
        assert dto.last_signer is None
        assert dto.name is None
        assert dto.addr is None
        assert dto.fn == "4000012345"  # fn з налаштувань

    @pytest.mark.asyncio
    async def test_get_status_no_open_shift_no_fn(self):
        """Немає відкритої зміни та fn — локальний стан за замовчуванням."""
        prro_repo = AsyncMock()
        prro_repo.get_open_shift.return_value = None
        settings_repo = AsyncMock()
        settings_repo.get.return_value = None
        context = MagicMock()
        context.grpc_client = AsyncMock(side_effect=Exception("down"))

        uc = _build_use_case(
            prro_repo=prro_repo, settings_repo=settings_repo, context=context
        )
        dto = await uc.get_status()

        assert dto.open_shift is False
        assert dto.online is False
        assert dto.fn is None

    @pytest.mark.asyncio
    async def test_get_status_partial_info(self):
        """Сервер доступний, але info не має fn — fn береться з налаштувань."""
        prro_repo = AsyncMock()
        prro_repo.get_open_shift.return_value = None
        settings_repo = AsyncMock()
        settings_repo.get.return_value = "LOCAL-FN"
        client = _make_grpc_client(fn=None)
        context = _make_context(client)

        uc = _build_use_case(
            prro_repo=prro_repo, settings_repo=settings_repo, context=context
        )
        dto = await uc.get_status()

        assert dto.fn == "LOCAL-FN"
        assert dto.name == "Тест ПРРО"


class TestGetQueue:
    """Тести журналу офлайн-черги."""

    @pytest.mark.asyncio
    async def test_get_queue_with_status_filter(self):
        """Фільтр за статусом pending."""
        items = [
            _make_queue_item(status=PrroQueueStatus.PENDING, local_number=1),
            _make_queue_item(status=PrroQueueStatus.SENT, local_number=2),
            _make_queue_item(status=PrroQueueStatus.FAILED, local_number=3),
        ]
        prro_repo = AsyncMock()
        prro_repo.list_pending.return_value = items
        prro_repo.count_pending.return_value = 1

        uc = _build_use_case(prro_repo=prro_repo)
        result = await uc.get_queue(status_filter="pending")

        assert result["total"] == 1
        assert result["pending"] == 1
        assert len(result["items"]) == 1
        assert result["items"][0].status == "pending"

    @pytest.mark.asyncio
    async def test_get_queue_no_filter(self):
        """Без фільтра — всі елементи черги."""
        items = [
            _make_queue_item(status=PrroQueueStatus.PENDING, local_number=1),
            _make_queue_item(status=PrroQueueStatus.PENDING, local_number=2),
        ]
        prro_repo = AsyncMock()
        prro_repo.list_pending.return_value = items
        prro_repo.count_pending.return_value = 2

        uc = _build_use_case(prro_repo=prro_repo)
        result = await uc.get_queue(page=1, size=20)

        assert result["total"] == 2
        assert result["pending"] == 2
        assert len(result["items"]) == 2
        assert result["page"] == 1
        assert result["size"] == 20

    @pytest.mark.asyncio
    async def test_get_queue_pagination_beyond_range(self):
        """Сторінка за межами діапазону — порожній список."""
        items = [_make_queue_item(local_number=1)]
        prro_repo = AsyncMock()
        prro_repo.list_pending.return_value = items
        prro_repo.count_pending.return_value = 0

        uc = _build_use_case(prro_repo=prro_repo)
        result = await uc.get_queue(page=5, size=20)

        assert result["total"] == 1
        assert result["items"] == []

    @pytest.mark.asyncio
    async def test_queue_to_dto(self):
        """Конвертація PrroQueueItem у DTO."""
        item = _make_queue_item(
            status=PrroQueueStatus.FAILED, local_number=42
        )
        dto = PrroStatusUseCase._queue_to_dto(item)

        assert dto.id == item.id
        assert dto.receipt_id == item.receipt_id
        assert dto.shift_id == item.shift_id
        assert dto.local_number == 42
        assert dto.check_type == "CHK"
        assert dto.status == "failed"
        assert dto.error == "помилка"
        assert dto.created_at is not None
        assert dto.sent_at is None
