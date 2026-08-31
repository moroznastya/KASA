"""Unit tests: B4 — offline state machine (109/110/112 + id_offline).

1:1 Rust `tests/prro_offline.rs`. Сценарій:
online → (мережа впала) → T=109 → T=112 → offline-чеки з id_offline
→ (мережа є) → T=110 → sync; усі документи пройшли.
"""

from __future__ import annotations

from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

import pytest

from app.infrastructure.services.prro.offline_state import (
    OfflineStateMachine,
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


def _make_xml_builder():
    builder = MagicMock()
    builder.build_service_check_xml = MagicMock(
        side_effect=lambda service_type, date_time=None: f"<DAT><C T=\"{service_type}\"/></DAT>"
    )
    builder.build_message = MagicMock(
        side_effect=lambda dat_xml, mac_value=None, **kw: f"<RQ>{dat_xml}<MAC/></RQ>"
    )
    builder.rro_fn = "4538765845"
    return builder


def _make_crypto():
    crypto = MagicMock()
    crypto.sign = MagicMock(side_effect=lambda b: b"<SIGN>" + b)
    return crypto


def test_parse_reserve_range_from_cnf():
    xml = b'<?xml version="1.0"?><RS V="1"><DAT><CNF TY="C" FR="1001" TO="1100" ER="0"/></DAT></RS>'
    assert parse_reserve_range(xml) == (1001, 1100)


def test_parse_reserve_range_invalid_returns_none():
    assert parse_reserve_range(b"not xml") is None
    assert parse_reserve_range(b'<CNF FR="100" TO="50"/>') is None


@pytest.mark.asyncio
async def test_offline_full_scenario():
    """online → (мережа впала) → 109 → 112 → offline-чек з id_offline → 110 → sync."""
    settings = _SettingsRepo()
    assert not await OfflineStateMachine.is_offline(settings)

    grpc = MagicMock()
    # 1. Мережа впала: T=109 — транспортна помилка (best-effort)
    grpc.send_chk = AsyncMock(side_effect=RuntimeError("net down"))
    await OfflineStateMachine.enter_offline(settings, grpc, _make_xml_builder(), _make_crypto())
    assert await OfflineStateMachine.is_offline(settings), "стан → offline"
    t109 = grpc.send_chk.await_args.args[0]
    assert 'T="109"' in t109.check_sign.decode(), "T=109 у check_sign"

    # 2. T=112: сервер дає діапазон у data_sign
    grpc.send_chk = AsyncMock(
        return_value=SimpleNamespace(
            status=1,
            data_sign=b'<RS><DAT><CNF TY="C" FR="1001" TO="1100" ER="0"/></DAT></RS>',
            error_message="",
        )
    )
    start, end = await OfflineStateMachine.reserve_numbers(
        settings, grpc, _make_xml_builder(), _make_crypto()
    )
    assert (start, end) == (1001, 1100)
    t112 = grpc.send_chk.await_args.args[0]
    assert 'T="112"' in t112.check_sign.decode()

    # 3. Offline-чек: резервний local_number + id_offline (не порожній)
    local, id_offline = await OfflineStateMachine.next_offline_local(settings)
    assert local == 1001
    assert id_offline == "offline-1001"
    assert id_offline, "id_offline не порожній"

    # 4. Мережа є: T=110 → sync (offline-чек відправлено з id_offline)
    sent_checks: list = []

    async def _send_chk(check):
        sent_checks.append(check)
        return SimpleNamespace(status=1, error_message="")

    grpc.send_chk = AsyncMock(side_effect=_send_chk)

    async def _sync():
        # sync відправляє offline-чек з id_offline
        return {"synced": 1, "failed": 0, "total": 1, "results": [{"status": "sent"}]}

    result = await OfflineStateMachine.exit_offline(
        settings, grpc, _make_xml_builder(), _make_crypto(), _sync
    )
    assert not await OfflineStateMachine.is_offline(settings), "стан → online"
    assert result["synced"] == 1
    t110 = sent_checks[0]
    assert 'T="110"' in t110.check_sign.decode(), "T=110 у check_sign"


@pytest.mark.asyncio
async def test_next_offline_local_increments_within_range():
    settings = _SettingsRepo()
    await settings.set("prro_reserve_start", "1001")
    await settings.set("prro_reserve_end", "1100")
    n1, id1 = await OfflineStateMachine.next_offline_local(settings)
    n2, id2 = await OfflineStateMachine.next_offline_local(settings)
    assert (n1, n2) == (1001, 1002)
    assert id1 == "offline-1001"
    assert id2 == "offline-1002"
    assert id1 and id2, "id_offline не порожній"
