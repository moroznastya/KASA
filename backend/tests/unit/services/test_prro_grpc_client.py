"""Unit tests: PrroGrpcClient — формат Check (date_time, ping check_sign)."""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock

from app.infrastructure.services.prro import prro_pb2
from app.infrastructure.services.prro.grpc_client import (
    PrroGrpcClient,
    _check_date_time,
)


def _make_client() -> tuple[PrroGrpcClient, MagicMock]:
    """Створює клієнт з мок-каналом та мок-стабом."""
    channel = MagicMock()
    stub = MagicMock()
    stub.ping = AsyncMock()
    client = PrroGrpcClient(channel, rro_fn="4538765845")
    client._stub = stub
    return client, stub


class TestCheckDateTime:
    """Формат date_time (yyyyMMddHHmmss — офіційний семпл ДПС)."""

    def test_format_is_14_digits(self):
        """date_time = yyyyMMddHHmmss (14 цифр), а не Unix epoch."""
        from datetime import datetime

        value = _check_date_time(datetime(2026, 8, 1, 14, 30, 15))
        assert value == 20260801143015

    def test_current_time_format(self):
        value = _check_date_time()
        assert 20_000_000_000_000 <= value <= 99_999_999_999_999  # 14 цифр


class TestPingCheckSign:
    """ping() передає XML T=111 у check_sign (не порожній)."""

    async def test_ping_passes_check_sign(self):
        client, stub = _make_client()
        stub.ping.return_value = prro_pb2.CheckResponse(status=1)

        xml = b"<RQ V=\"1\"><DAT DI=\"1\" FN=\"4538765845\"></DAT></RQ>"
        await client.ping(check_sign=xml)

        sent: prro_pb2.Check = stub.ping.await_args.args[0]
        assert sent.check_sign == xml
        assert sent.check_type == prro_pb2.Check.SERVICECHK
        assert sent.local_number == 0x7FFFFFFF
        assert sent.rro_fn == "4538765845"

    async def test_ping_default_empty(self):
        client, stub = _make_client()
        stub.ping.return_value = prro_pb2.CheckResponse(status=1)

        await client.ping()

        sent: prro_pb2.Check = stub.ping.await_args.args[0]
        assert sent.check_sign == b""
        assert sent.check_type == prro_pb2.Check.SERVICECHK
        assert sent.local_number == 0x7FFFFFFF

    async def test_ping_date_time_format(self):
        client, stub = _make_client()
        stub.ping.return_value = prro_pb2.CheckResponse(status=1)

        await client.ping()

        sent: prro_pb2.Check = stub.ping.await_args.args[0]
        assert sent.date_time > 20_000_000_000_000  # 14 цифр, не Unix epoch


class TestMakeCheck:
    """_make_check: реквізити Check."""

    def test_make_check_defaults(self):
        client, _ = _make_client()
        check = client._make_check()
        assert check.rro_fn == "4538765845"
        assert check.local_number == 0
        assert check.check_type == prro_pb2.Check.CHK
        assert check.check_sign == b""
        assert check.date_time > 20_000_000_000_000

    def test_make_check_custom(self):
        client, _ = _make_client()
        check = client._make_check(
            check_sign=b"<xml/>",
            local_number=0x7FFFFFFF,
            check_type=prro_pb2.Check.SERVICECHK,
            date_time=20260801143015,
        )
        assert check.local_number == 0x7FFFFFFF
        assert check.check_type == prro_pb2.Check.SERVICECHK
        assert check.date_time == 20260801143015
