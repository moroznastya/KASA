"""Unit tests: offline state machine (109/110/112 + id_offline) — спека F.

Сценарій:
online → (мережа впала) → T=109 (у черзі/ланцюзі) → T=112 (<ID>-діапазон)
→ offline-чек: id_offline з діапазону (local_number — послідовний зі зміни,
як онлайн) → (мережа є) → T=110 → sync; усі документи пройшли.
"""

from __future__ import annotations

from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.infrastructure.services.prro.offline_state import (
    OfflineStateMachine,
    parse_reserve_ids,
    parse_reserve_range,
)


class _SettingsRepo:
    """Фейковий PrroSettingsRepository (get/set у dict)."""

    def __init__(self) -> None:
        self._data: dict[str, str] = {}

    async def get(self, key: str):
        return self._data.get(key)

    async def set(self, key: str, value: str) -> None:
        self._data[key] = value


class _ShiftRepo:
    """Мінімальний PrroRepository: get_shift/update_shift_last_mac."""

    def __init__(self, last_mac: str = ""):
        self.last_mac = last_mac

    async def get_shift(self, shift_id):
        return SimpleNamespace(id=shift_id, last_mac=self.last_mac)

    async def update_shift_last_mac(self, shift_id, last_mac: str):
        self.last_mac = last_mac
        return SimpleNamespace(id=shift_id, last_mac=last_mac)


class _OfflineQueue:
    """Мінімальний PrroOfflineQueue для перевірки, що 109 не губиться."""

    def __init__(self) -> None:
        self.items: list = []

    async def add_document(self, **kwargs):
        item = SimpleNamespace(id=uuid4(), **kwargs)
        item.status = "pending"
        self.items.append(item)
        return item

    async def mark_sent(self, item_id):
        for item in self.items:
            if item.id == item_id:
                item.status = "sent"
        return None


def _make_xml_builder():
    builder = MagicMock()
    builder.build_service_check_xml = MagicMock(
        side_effect=lambda service_type, date_time=None, reserve_size=150:
            f"<DAT><C T=\"{service_type}\">{'<H SIZE=\"150\"></H>' if service_type == '112' else ''}</C></DAT>"
    )
    builder.build_message = MagicMock(
        side_effect=lambda dat_xml, mac_value=None, mac_id="", **kw:
            f"<?xml version=\"1.0\" encoding=\"windows-1251\"?><RQ>{dat_xml}"
            f"<MAC ID=\"{mac_id}\">{mac_value or ''}</MAC></RQ>"
    )
    builder.rro_fn = "4538765845"
    return builder


def _make_crypto():
    crypto = MagicMock()
    crypto.sign = MagicMock(side_effect=lambda b: b"<SIGN>" + b)
    return crypto


def test_parse_reserve_ids_from_docx_sample():
    """Відповідь ПРРО на T=112 — перелік <ID> (спека F)."""
    xml = (
        b'<?xml version="1.0" encoding="windows-1251"?><RS V="1">'
        b'<C T="112"><ID>1001</ID><ID>1002</ID><ID>1003</ID></C></RS>'
    )
    assert parse_reserve_ids(xml) == [1001, 1002, 1003]
    assert parse_reserve_range(xml) == (1001, 1003)


def test_parse_reserve_ids_no_ids_returns_empty():
    assert parse_reserve_ids(b"not xml") == []
    assert parse_reserve_ids(b'<RS><C T="112"></C></RS>') == []
    # модемний <CNF FR TO> більше НЕ парситься як ПРРО-відповідь
    assert parse_reserve_ids(b'<CNF TY="C" FR="100" TO="50"/>') == []


@pytest.mark.asyncio
async def test_offline_full_scenario():
    """online → (мережа впала) → 109 (черга) → 112 → offline-чек → 110 → sync."""
    settings = _SettingsRepo()
    queue = _OfflineQueue()
    shift_repo = _ShiftRepo(last_mac="chain-0")
    shift_id = uuid4()
    assert not await OfflineStateMachine.is_offline(settings)

    grpc = MagicMock()
    # 1. Мережа впала: T=109 — транспортна помилка; документ у черзі (не губиться)
    grpc.send_chk = AsyncMock(side_effect=RuntimeError("net down"))
    builder = _make_xml_builder()
    await OfflineStateMachine.enter_offline(
        settings, grpc, builder, _make_crypto(),
        offline_queue=queue, prro_repo=shift_repo, shift_id=shift_id,
    )
    assert await OfflineStateMachine.is_offline(settings), "стан → offline"
    t109 = grpc.send_chk.await_args.args[0]
    assert 'T="109"' in t109.check_sign.decode(errors="replace"), "T=109 у check_sign"
    # 109 у черзі (pending, бо не доставлено) і ланцюг зсунуто на hash(109)
    assert len(queue.items) == 1
    assert queue.items[0].check_type == "SERVICECHK"
    assert queue.items[0].status == "pending"
    assert shift_repo.last_mac != "chain-0", "ланцюг зсунуто після 109"

    # 2. T=112: сервер повертає <ID>-перелік
    grpc.send_chk = AsyncMock(
        return_value=SimpleNamespace(
            status=1,
            data_sign=(
                b'<RS V="1"><C T="112"><ID>1001</ID><ID>1002</ID>'
                b'<ID>1003</ID></C></RS>'
            ),
            error_message="",
        )
    )
    start, end = await OfflineStateMachine.reserve_numbers(
        settings, grpc, _make_xml_builder(), _make_crypto()
    )
    assert (start, end) == (1001, 1003)
    t112 = grpc.send_chk.await_args.args[0]
    assert 'T="112"' in t112.check_sign.decode(errors="replace")

    # 3. id_offline — фіскальний номер з діапазону (НЕ "offline-{n}")
    n1 = await OfflineStateMachine.next_offline_number(settings)
    n2 = await OfflineStateMachine.next_offline_number(settings)
    assert (n1, n2) == (1001, 1002)

    # 4. Мережа є: T=110 → sync (offline-чек відправлено з id_offline)
    sent_checks: list = []

    async def _send_chk(check):
        sent_checks.append(check)
        return SimpleNamespace(status=1, error_message="")

    grpc.send_chk = AsyncMock(side_effect=_send_chk)

    async def _sync():
        return {"synced": 1, "failed": 0, "total": 1, "results": [{"status": "sent"}]}

    result = await OfflineStateMachine.exit_offline(
        settings, grpc, _make_xml_builder(), _make_crypto(), _sync
    )
    assert not await OfflineStateMachine.is_offline(settings), "стан → online"
    assert result["synced"] == 1
    t110 = sent_checks[0]
    assert 'T="110"' in t110.check_sign.decode(errors="replace"), "T=110 у check_sign"


@pytest.mark.asyncio
async def test_next_offline_number_increments_within_range():
    settings = _SettingsRepo()
    await settings.set("prro_reserve_start", "1001")
    await settings.set("prro_reserve_end", "1100")
    n1 = await OfflineStateMachine.next_offline_number(settings)
    n2 = await OfflineStateMachine.next_offline_number(settings)
    assert (n1, n2) == (1001, 1002)


@pytest.mark.asyncio
async def test_next_offline_number_without_range_raises():
    """Без діапазону — зрозуміла помилка, а не фейковий дефолт (спека F)."""
    settings = _SettingsRepo()
    with pytest.raises(RuntimeError):
        await OfflineStateMachine.next_offline_number(settings)
