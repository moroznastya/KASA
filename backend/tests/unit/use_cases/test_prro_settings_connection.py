"""Unit tests: PrroSettingsUseCase.test_connection — ping з XML T=111."""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock

import pytest

from app.infrastructure.services.prro import prro_pb2
from app.application.use_cases.prro.prro_settings_use_case import (
    PrroSettingsUseCase,
)
from app.infrastructure.services.prro.crypto_signer import PrroCryptoError
from app.infrastructure.services.prro.xml_builder import SERVICE_PING


@pytest.fixture
def settings_uc():
    """PrroSettingsUseCase з моками (контекст, key_store, репозиторії)."""
    settings_repo = MagicMock()
    prro_repo = MagicMock()
    key_store = MagicMock()
    context = MagicMock()

    builder = MagicMock()
    builder.build_service_check_xml = MagicMock(return_value="<DAT>...T=111...</DAT>")
    builder.build_message = MagicMock(
        return_value='<RQ V="1"><DAT>...T=111...</DAT></RQ>'
    )
    context.build_xml_builder = AsyncMock(return_value=builder)

    uc = PrroSettingsUseCase(
        settings_repo=settings_repo,
        prro_repo=prro_repo,
        key_store=key_store,
        context_factory=context,
    )
    uc._context = context
    return uc, context


class TestBuildPingCheckSign:
    async def test_builds_service_ping_xml(self, settings_uc):
        """XML формується з service_type=111 та без MAC."""
        uc, context = settings_uc
        crypto = MagicMock()
        crypto.sign = MagicMock(return_value=b"<signed-xml/>")
        context.build_crypto_signer = AsyncMock(return_value=crypto)

        check_sign, error = await uc._build_ping_check_sign()

        builder = context.build_xml_builder.await_args
        assert builder is not None
        # build_service_check_xml викликано з SERVICE_PING
        assert context.build_xml_builder.return_value.build_service_check_xml.call_args.kwargs[
            "service_type"
        ] == SERVICE_PING
        # build_message без MAC (документація: «MAC не заповнюється»)
        assert context.build_xml_builder.return_value.build_message.call_args.kwargs[
            "include_mac"
        ] is False
        assert check_sign == b"<signed-xml/>"
        assert error is None

    async def test_sign_error_returns_unsigned_xml(self, settings_uc):
        """Якщо ключ не прочитано — повертається непідписаний XML + причина."""
        uc, context = settings_uc
        context.build_crypto_signer = AsyncMock(
            side_effect=PrroCryptoError("Файл ключа є контейнером ІІТ «ЦСК-1» ...")
        )

        check_sign, error = await uc._build_ping_check_sign()

        assert error is not None
        assert "ІІТ" in error
        # непідписаний XML все одно надсилається (щоб отримати відповідь сервера)
        assert check_sign == b'<RQ V="1"><DAT>...T=111...</DAT></RQ>'


class TestTestConnection:
    async def test_returns_server_status(self, settings_uc):
        """Повертає статус сервера + зрозуміле пояснення."""
        uc, context = settings_uc
        context.build_crypto_signer = AsyncMock(
            side_effect=PrroCryptoError("ключ недоступний")
        )
        client = MagicMock()
        client.ping = AsyncMock(
            return_value=prro_pb2.CheckResponse(
                status=-13,
                error_message="RRO not registered",
            )
        )
        context.grpc_client = AsyncMock(return_value=client)

        result = await uc.test_connection()

        assert result["status"] == -13
        assert result["ok"] is False
        assert "ПРРО не зареєстровано" in result["error"]
        assert "RRO not registered" in result["error"]
        # ping отримав check_sign (XML, а не b"")
        sent = client.ping.await_args.kwargs["check_sign"]
        assert sent.startswith(b"<RQ")

    async def test_ok_status(self, settings_uc):
        """status=1 → ok=True."""
        uc, context = settings_uc
        context.build_crypto_signer = AsyncMock(
            side_effect=PrroCryptoError("ключ недоступний")
        )
        client = MagicMock()
        client.ping = AsyncMock(
            return_value=prro_pb2.CheckResponse(status=1, error_message="")
        )
        context.grpc_client = AsyncMock(return_value=client)

        result = await uc.test_connection()
        assert result["status"] == 1
        assert result["ok"] is True

    async def test_grpc_error_returns_0(self, settings_uc):
        """gRPC-помилка (сервер недоступний) → status=0 з текстом помилки."""
        uc, context = settings_uc
        context.build_crypto_signer = AsyncMock(
            side_effect=PrroCryptoError("ключ недоступний")
        )
        client = MagicMock()
        client.ping = AsyncMock(side_effect=RuntimeError("UNAVAILABLE: connect failed"))
        context.grpc_client = AsyncMock(return_value=client)

        result = await uc.test_connection()
        assert result["status"] == 0
        assert result["ok"] is False
        assert "UNAVAILABLE" in result["error"]
