"""Unit tests: B3 — rro_fn_sign заповнюється в усіх службових RPC.

Критерій: жоден CheckRequest/CheckRequestId не містить порожнього rro_fn_sign,
коли клієнт створено з підписаним ФН ПРРО.
"""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock

import pytest

from app.infrastructure.services.prro import prro_pb2
from app.infrastructure.services.prro.grpc_client import PrroGrpcClient


def _make_client(rro_fn_sign: bytes = b"sign-of-fn-4538765845") -> tuple[PrroGrpcClient, MagicMock]:
    channel = MagicMock()
    stub = MagicMock()
    stub.statusRro = AsyncMock(return_value=prro_pb2.StatusResponse())
    stub.infoRro = AsyncMock(return_value=prro_pb2.RroInfoResponse())
    stub.lastChk = AsyncMock(return_value=prro_pb2.CheckResponse())
    stub.delLastChk = AsyncMock(return_value=prro_pb2.CheckResponse())
    stub.delLastChkId = AsyncMock(return_value=prro_pb2.CheckResponse())
    channel.__enter__ = MagicMock()
    client = PrroGrpcClient.__new__(PrroGrpcClient)
    client._channel = channel
    client._rro_fn = "4538765845"
    client._rro_fn_sign = rro_fn_sign
    client._stub = stub
    return client, stub


def _make_client_empty() -> tuple[PrroGrpcClient, MagicMock]:
    return _make_client(rro_fn_sign=b"")


@pytest.mark.asyncio
async def test_status_rro_fn_sign_non_empty():
    client, stub = _make_client()
    await client.status()
    req = stub.statusRro.await_args.args[0]
    assert req.rro_fn_sign == b"sign-of-fn-4538765845"
    assert req.rro_fn_sign, "B3: rro_fn_sign не порожній у statusRro"


@pytest.mark.asyncio
async def test_info_rro_fn_sign_non_empty():
    client, stub = _make_client()
    await client.info()
    req = stub.infoRro.await_args.args[0]
    assert req.rro_fn_sign, "B3: rro_fn_sign не порожній у infoRro"


@pytest.mark.asyncio
async def test_last_chk_rro_fn_sign_non_empty():
    client, stub = _make_client()
    await client.last_chk()
    req = stub.lastChk.await_args.args[0]
    assert req.rro_fn_sign, "B3: rro_fn_sign не порожній у lastChk"


@pytest.mark.asyncio
async def test_del_last_chk_rro_fn_sign_non_empty():
    client, stub = _make_client()
    await client.del_last_chk()
    req = stub.delLastChk.await_args.args[0]
    assert req.rro_fn_sign, "B3: rro_fn_sign не порожній у delLastChk"


@pytest.mark.asyncio
async def test_del_last_chk_id_rro_fn_sign_non_empty():
    client, stub = _make_client()
    await client.del_last_chk_id("chk-42")
    req = stub.delLastChkId.await_args.args[0]
    assert req.id == "chk-42"
    assert req.rro_fn_sign, "B3: rro_fn_sign не порожній у delLastChkId"


@pytest.mark.asyncio
async def test_all_service_rpc_requests_never_empty_when_signed():
    """Жоден службовий RPC не надсилає порожній rro_fn_sign (коли ФН підписано)."""
    client, stub = _make_client()
    await client.status()
    await client.info()
    await client.last_chk()
    await client.del_last_chk()
    await client.del_last_chk_id("x")
    for m in (stub.statusRro, stub.infoRro, stub.lastChk, stub.delLastChk, stub.delLastChkId):
        req = m.await_args.args[0]
        assert req.rro_fn_sign, f"B3: {m._extract_mock_name()} — rro_fn_sign порожній"


@pytest.mark.asyncio
async def test_empty_rro_fn_sign_when_not_configured():
    """Без підписаного ФН (клієнт створено без signer) — запит усе одно формується."""
    client, stub = _make_client_empty()
    await client.status()
    req = stub.statusRro.await_args.args[0]
    assert req.rro_fn_sign == b""
