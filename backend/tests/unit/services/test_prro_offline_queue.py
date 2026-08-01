"""Unit tests: PrroOfflineQueue — черга офлайн-документів ПРРО."""

from __future__ import annotations

from datetime import datetime, timedelta
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.prro import (
    PrroQueueItem,
    PrroQueueStatus,
)
from app.infrastructure.persistence.repositories.prro_repository import (
    PrroRepository,
)
from app.infrastructure.services.prro.offline_queue import (
    PrroOfflineQueue,
    PRRO_OFFLINE_LIMIT_HOURS,
    CHECK_TYPE_CHK,
)


@pytest.fixture
def mock_repo() -> MagicMock:
    """Мок-репозиторій PrroRepository (без БД)."""
    repo = MagicMock(spec=PrroRepository)
    repo.add_to_queue = AsyncMock()
    repo.list_pending = AsyncMock(return_value=[])
    repo.mark_sent = AsyncMock()
    repo.mark_failed = AsyncMock()
    repo.count_pending = AsyncMock(return_value=0)
    repo.list_by_shift = AsyncMock(return_value=[])
    return repo


@pytest.fixture
def queue(mock_repo) -> PrroOfflineQueue:
    """Черга на основі мок-репозиторію."""
    return PrroOfflineQueue(mock_repo)


def _make_item(**kwargs) -> PrroQueueItem:
    """Створює PrroQueueItem з дефолтними значеннями."""
    defaults = dict(
        id=uuid4(),
        receipt_id=None,
        shift_id=uuid4(),
        local_number=1,
        check_type=CHECK_TYPE_CHK,
        xml_body="<DAT></DAT>",
        mac=None,
        status=PrroQueueStatus.PENDING,
        created_at=datetime.utcnow(),
        sent_at=None,
        error=None,
    )
    defaults.update(kwargs)
    return PrroQueueItem(**defaults)


class TestAddDocument:
    """Додавання документів у чергу."""

    async def test_add_document_calls_repository(self, queue, mock_repo):
        """add_document створює PrroQueueItem через репозиторій."""
        shift_id = uuid4()
        receipt_id = uuid4()
        mock_repo.add_to_queue.return_value = _make_item(
            shift_id=shift_id, receipt_id=receipt_id, local_number=5
        )

        item = await queue.add_document(
            receipt_id=receipt_id,
            shift_id=shift_id,
            local_number=5,
            check_type=CHECK_TYPE_CHK,
            xml_body="<DAT DI=\"1\"></DAT>",
            mac="base64mac",
        )

        mock_repo.add_to_queue.assert_awaited_once()
        created: PrroQueueItem = mock_repo.add_to_queue.call_args.args[0]
        assert created.receipt_id == receipt_id
        assert created.shift_id == shift_id
        assert created.local_number == 5
        assert created.check_type == CHECK_TYPE_CHK
        assert created.mac == "base64mac"
        assert created.status == PrroQueueStatus.PENDING

    async def test_negative_local_number_raises(self, queue):
        """Від'ємний локальний номер → ValueError."""
        with pytest.raises(ValueError):
            await queue.add_document(
                receipt_id=None, shift_id=None, local_number=-1,
                check_type=CHECK_TYPE_CHK, xml_body="<DAT></DAT>",
            )

    async def test_empty_xml_raises(self, queue):
        """Порожній xml_body → ValueError."""
        with pytest.raises(ValueError):
            await queue.add_document(
                receipt_id=None, shift_id=None, local_number=1,
                check_type=CHECK_TYPE_CHK, xml_body="   ",
            )


class TestGetAndMark:
    """Читання черги та оновлення статусів."""

    async def test_get_pending_delegates(self, queue, mock_repo):
        """get_pending делегує у репозиторій list_pending."""
        items = [_make_item(local_number=1), _make_item(local_number=2)]
        mock_repo.list_pending.return_value = items

        result = await queue.get_pending(limit=50)
        mock_repo.list_pending.assert_awaited_once_with(limit=50)
        assert result == items

    async def test_mark_sent_delegates(self, queue, mock_repo):
        """mark_sent делегує у репозиторій."""
        item_id = uuid4()
        sent_at = datetime.utcnow()
        mock_repo.mark_sent.return_value = _make_item(
            id=item_id, status=PrroQueueStatus.SENT, sent_at=sent_at
        )

        result = await queue.mark_sent(item_id, sent_at=sent_at)
        mock_repo.mark_sent.assert_awaited_once_with(item_id, sent_at=sent_at)
        assert result is not None
        assert result.status == PrroQueueStatus.SENT

    async def test_mark_failed_delegates(self, queue, mock_repo):
        """mark_failed делегує у репозиторій із текстом помилки."""
        item_id = uuid4()
        mock_repo.mark_failed.return_value = _make_item(
            id=item_id, status=PrroQueueStatus.FAILED, error="timeout"
        )

        result = await queue.mark_failed(item_id, "timeout")
        mock_repo.mark_failed.assert_awaited_once_with(item_id, "timeout")
        assert result is not None
        assert result.status == PrroQueueStatus.FAILED


class TestOfflineLimit:
    """Ліміт офлайн-режиму (168 годин)."""

    def test_limit_constant(self):
        """Константа ліміту = 168 годин."""
        assert PRRO_OFFLINE_LIMIT_HOURS == 168

    def test_not_expired(self):
        """Свіжий документ не є простроченим."""
        created = datetime.utcnow() - timedelta(hours=10)
        assert PrroOfflineQueue.is_expired(created) is False

    def test_expired_at_limit(self):
        """Документ старіший за 168 годин — прострочений."""
        created = datetime.utcnow() - timedelta(hours=168, minutes=1)
        assert PrroOfflineQueue.is_expired(created) is True

    async def test_get_expired_filters(self, queue, mock_repo):
        """get_expired повертає лише прострочені документи."""
        fresh = _make_item(local_number=1, created_at=datetime.utcnow())
        expired = _make_item(
            local_number=2,
            created_at=datetime.utcnow() - timedelta(hours=200),
        )
        mock_repo.list_pending.return_value = [fresh, expired]

        result = await queue.get_expired()
        assert len(result) == 1
        assert result[0].local_number == 2
