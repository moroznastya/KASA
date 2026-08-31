"""Unit tests: SyncOfflineQueueUseCase (app/application/use_cases/prro/sync_offline_queue_use_case.py).

Покриває:
- sync з порожньою чергою
- sync — всі документи успішно передані (status=1)
- sync — документ зі статусом помилки від сервера (status!=1)
- sync — виняток під час передачі документа
- sync — частковий успіх (кілька документів)
"""

from __future__ import annotations

from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.application.use_cases.prro.sync_offline_queue_use_case import (
    SyncOfflineQueueUseCase,
)


def _make_queue_item(
    *,
    local_number: int = 1,
    check_type: str = "CHK",
    xml_body: str = "<DAT>...</DAT>",
):
    return SimpleNamespace(
        id=uuid4(),
        local_number=str(local_number),
        check_type=check_type,
        xml_body=xml_body,
    )


def _make_check_response(*, status: int = 1, error_message: str = ""):
    return SimpleNamespace(status=status, error_message=error_message)


def _make_context():
    """Mock PrroContextFactory з AsyncMock-методами."""
    context = MagicMock()
    context.build_xml_builder = AsyncMock()
    context.build_crypto_signer = AsyncMock()
    context.grpc_client = AsyncMock()
    context.build_check = AsyncMock(return_value=SimpleNamespace(ok=True))
    context.persist_builder_counters = AsyncMock()
    return context


def _build_use_case(
    *,
    offline_queue: AsyncMock | None = None,
    context: MagicMock | None = None,
    session: AsyncMock | None = None,
) -> SyncOfflineQueueUseCase:
    return SyncOfflineQueueUseCase(
        session=session or AsyncMock(spec=AsyncSession),
        prro_repo=AsyncMock(),
        settings_repo=AsyncMock(),
        context_factory=context or _make_context(),
        offline_queue=offline_queue or AsyncMock(),
    )


class TestSync:
    """Тести синхронізації офлайн-черги."""

    @pytest.mark.asyncio
    async def test_sync_empty_queue(self):
        """Порожня черга — нічого не синхронізується."""
        offline_queue = AsyncMock()
        offline_queue.get_pending.return_value = []
        session = AsyncMock(spec=AsyncSession)
        context = _make_context()

        uc = _build_use_case(
            offline_queue=offline_queue, context=context, session=session
        )
        result = await uc.sync()

        assert result == {"synced": 0, "failed": 0, "skipped": 0, "total": 0, "results": []}
        # жодного звернення до контексту/БД
        context.grpc_client.assert_not_awaited()
        session.commit.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_sync_all_success(self):
        """Всі документи передані успішно."""
        items = [_make_queue_item(local_number=1), _make_queue_item(local_number=2)]
        offline_queue = AsyncMock()
        offline_queue.get_pending.return_value = items

        xml_builder = MagicMock()
        xml_builder.build_message.return_value = "<signed-message/>"
        crypto = MagicMock()
        crypto.sign.return_value = b"<signed-doc/>"

        grpc_client = MagicMock()
        grpc_client.send_chk = AsyncMock(return_value=_make_check_response(status=1))

        context = _make_context()
        context.build_xml_builder.return_value = xml_builder
        context.build_crypto_signer.return_value = crypto
        context.grpc_client.return_value = grpc_client

        session = AsyncMock(spec=AsyncSession)
        uc = _build_use_case(
            offline_queue=offline_queue, context=context, session=session
        )
        result = await uc.sync()

        assert result["synced"] == 2
        assert result["failed"] == 0
        assert result["total"] == 2
        assert all(r["status"] == "sent" for r in result["results"])
        assert offline_queue.mark_sent.await_count == 2
        offline_queue.mark_failed.assert_not_awaited()
        context.persist_builder_counters.assert_awaited_once_with(xml_builder)
        session.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_sync_server_error_status(self):
        """Сервер повертає статус != 1 — документ помічається як failed."""
        items = [_make_queue_item(local_number=7)]
        offline_queue = AsyncMock()
        offline_queue.get_pending.return_value = items

        xml_builder = MagicMock()
        xml_builder.build_message.return_value = "<msg/>"
        crypto = MagicMock()
        crypto.sign.return_value = b"<sig/>"

        grpc_client = MagicMock()
        grpc_client.send_chk = AsyncMock(
            return_value=_make_check_response(status=0, error_message="Помилка ФС")
        )

        context = _make_context()
        context.build_xml_builder.return_value = xml_builder
        context.build_crypto_signer.return_value = crypto
        context.grpc_client.return_value = grpc_client

        session = AsyncMock(spec=AsyncSession)
        uc = _build_use_case(
            offline_queue=offline_queue, context=context, session=session
        )
        result = await uc.sync()

        assert result["synced"] == 0
        assert result["failed"] == 1
        assert result["results"][0]["status"] == "failed"
        assert result["results"][0]["error"] == "Помилка ФС"
        offline_queue.mark_sent.assert_not_awaited()
        offline_queue.mark_failed.assert_awaited_once()
        session.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_sync_exception_during_send(self):
        """Виняток під час передачі — документ помічається як failed."""
        items = [_make_queue_item(local_number=3)]
        offline_queue = AsyncMock()
        offline_queue.get_pending.return_value = items

        context = _make_context()
        # build_message кидає виняток
        xml_builder = MagicMock()
        xml_builder.build_message.side_effect = RuntimeError("boom")
        context.build_xml_builder.return_value = xml_builder

        session = AsyncMock(spec=AsyncSession)
        uc = _build_use_case(
            offline_queue=offline_queue, context=context, session=session
        )
        result = await uc.sync()

        assert result["synced"] == 0
        assert result["failed"] == 1
        assert "boom" in result["results"][0]["error"]
        offline_queue.mark_failed.assert_awaited_once()
        # лічильники зберігаються навіть при помилці
        context.persist_builder_counters.assert_awaited_once()
        session.commit.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_sync_partial_success(self):
        """Частковий успіх: перший успішний, другий — помилка."""
        items = [_make_queue_item(local_number=1), _make_queue_item(local_number=2)]
        offline_queue = AsyncMock()
        offline_queue.get_pending.return_value = items

        xml_builder = MagicMock()
        xml_builder.build_message.return_value = "<msg/>"
        crypto = MagicMock()
        crypto.sign.return_value = b"<sig/>"

        grpc_client = MagicMock()
        grpc_client.send_chk = AsyncMock(
            side_effect=[
                _make_check_response(status=1),
                _make_check_response(status=2, error_message="status=2"),
            ]
        )

        context = _make_context()
        context.build_xml_builder.return_value = xml_builder
        context.build_crypto_signer.return_value = crypto
        context.grpc_client.return_value = grpc_client

        session = AsyncMock(spec=AsyncSession)
        uc = _build_use_case(
            offline_queue=offline_queue, context=context, session=session
        )
        result = await uc.sync()

        assert result["synced"] == 1
        assert result["failed"] == 1
        assert result["total"] == 2
        statuses = {r["local_number"]: r["status"] for r in result["results"]}
        assert statuses[1] == "sent"
        assert statuses[2] == "failed"
        assert offline_queue.mark_sent.await_count == 1
        assert offline_queue.mark_failed.await_count == 1


class TestSyncIdempotentB2:
    """B2: ідемпотентність sync — повний підписаний check_sign відправляється as-is.

    Критерій: build_message викликається рівно 1 раз на документ; повторні
    sync не змінюють NT/MAC/підпис (check_sign ідентичний між спробами).
    """

    def _make_item_with_sign(self):
        return SimpleNamespace(
            id=uuid4(),
            local_number="1",
            check_type="CHK",
            xml_body="<DAT>...</DAT>",
            check_sign="<full-signed-check/>",
        )

    def _make_legacy_item(self):
        return SimpleNamespace(
            id=uuid4(),
            local_number="1",
            check_type="CHK",
            xml_body="<DAT>...</DAT>",
            check_sign=None,
        )

    @pytest.mark.asyncio
    async def test_sync_sends_stored_check_sign_as_is(self):
        """Документ із check_sign → відправляється as-is, build_message/sign — 0 викликів."""
        items = [self._make_item_with_sign()]
        offline_queue = AsyncMock()
        offline_queue.get_pending.return_value = items

        xml_builder = MagicMock()
        crypto = MagicMock()
        grpc_client = MagicMock()
        grpc_client.send_chk = AsyncMock(return_value=_make_check_response(status=1))

        context = _make_context()
        context.build_xml_builder.return_value = xml_builder
        context.build_crypto_signer.return_value = crypto
        context.grpc_client.return_value = grpc_client

        uc = _build_use_case(
            offline_queue=offline_queue,
            context=context,
            session=AsyncMock(spec=AsyncSession),
        )
        result = await uc.sync()

        assert result["synced"] == 1
        sent = context.build_check.await_args.kwargs["check_sign"]
        assert sent == b"<full-signed-check/>", "check_sign as-is"
        xml_builder.build_message.assert_not_called()
        crypto.sign.assert_not_called()
        offline_queue.update_check_sign.assert_not_awaited()

    @pytest.mark.asyncio
    async def test_sync_two_attempts_identical_check_sign(self):
        """2 спроби sync (документ повертається у чергу) → ІДЕНТИЧНИЙ check_sign."""
        items = [self._make_item_with_sign()]
        offline_queue = AsyncMock()
        offline_queue.get_pending.side_effect = [items, items]

        xml_builder = MagicMock()
        crypto = MagicMock()
        grpc_client = MagicMock()
        grpc_client.send_chk = AsyncMock(return_value=_make_check_response(status=1))

        context = _make_context()
        context.build_xml_builder.return_value = xml_builder
        context.build_crypto_signer.return_value = crypto
        context.grpc_client.return_value = grpc_client

        uc = _build_use_case(
            offline_queue=offline_queue,
            context=context,
            session=AsyncMock(spec=AsyncSession),
        )
        r1 = await uc.sync()
        r2 = await uc.sync()

        assert r1["synced"] == 1 and r2["synced"] == 1
        sent1 = context.build_check.await_args.kwargs["check_sign"]
        assert sent1 == b"<full-signed-check/>"
        # build_message викликається рівно 0 разів за 2 спроби (check_sign є)
        xml_builder.build_message.assert_not_called()
        crypto.sign.assert_not_called()

    @pytest.mark.asyncio
    async def test_sync_legacy_item_formats_once_and_persists(self):
        """Legacy-документ (check_sign=None): формується рівно 1 раз і фіксується.

        Друга спроба — as-is, crypto.sign викликається рівно 1 раз загалом.
        """
        items = [self._make_legacy_item()]

        def _persist_check_sign(item_id, check_sign):
            # update_check_sign у реальній БД оновлює рядок → list_pending
            # наступного разу повертає item із check_sign
            items[0].check_sign = check_sign
            return SimpleNamespace(id=item_id, check_sign=check_sign)

        offline_queue = AsyncMock()
        offline_queue.get_pending.side_effect = [items, items]
        offline_queue.update_check_sign = AsyncMock(side_effect=_persist_check_sign)

        xml_builder = MagicMock()
        xml_builder.build_message.return_value = "<msg/>"
        crypto = MagicMock()
        crypto.sign.return_value = b"<legacy-signed/>"

        grpc_client = MagicMock()
        grpc_client.send_chk = AsyncMock(return_value=_make_check_response(status=1))

        context = _make_context()
        context.build_xml_builder.return_value = xml_builder
        context.build_crypto_signer.return_value = crypto
        context.grpc_client.return_value = grpc_client

        uc = _build_use_case(
            offline_queue=offline_queue,
            context=context,
            session=AsyncMock(spec=AsyncSession),
        )
        r1 = await uc.sync()
        assert r1["synced"] == 1
        # сформовано 1 раз і збережено
        offline_queue.update_check_sign.assert_awaited_once()
        assert crypto.sign.call_count == 1
        assert xml_builder.build_message.call_count == 1

        r2 = await uc.sync()
        assert r2["synced"] == 1
        # друга спроба — as-is, sign більше не викликається
        assert crypto.sign.call_count == 1, "build_message рівно 1 раз на документ"
        assert xml_builder.build_message.call_count == 1
