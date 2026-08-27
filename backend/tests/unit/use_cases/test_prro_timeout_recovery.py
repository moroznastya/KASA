"""Unit tests: H1 (timeout recovery) + V1 (QR mac = MAC чека).
1:1 Rust `tests/prro_timeout.rs`.
"""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.application.use_cases.prro.context import (
    PrroContextFactory,
    KEY_PRRO_FN,
    KEY_PRRO_TN,
    KEY_PRRO_ZN,
)
from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    FiscalizeReceiptUseCase,
)
from app.infrastructure.persistence.models.prro import PrroShift
from app.infrastructure.persistence.models.user import User, UserRole
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.key_store import PrroKeyStore
from app.infrastructure.services.prro.offline_queue import PrroOfflineQueue
from app.infrastructure.services.prro.xml_builder import extract_check_no


def make_response(*, status: int = 1, id: str = "FISCAL-100", error_message: str = ""):
    return SimpleNamespace(
        status=status,
        id=id,
        error_message=error_message,
        id_sign=b"id-sign-100",
        data_sign=b"",
    )


def make_grpc(send_side_effect=None, last_response=None):
    client = MagicMock()
    if send_side_effect is None:
        client.send_chk = AsyncMock(return_value=make_response())
    else:
        client.send_chk = AsyncMock(side_effect=send_side_effect)
    client.last_chk = AsyncMock(return_value=last_response or make_response())
    return client


def make_crypto():
    crypto = MagicMock()
    crypto.sign = MagicMock(return_value=b"<signed/>")
    crypto.get_serial_number = MagicMock(return_value="SERIAL-1")
    crypto.get_signer_name = MagicMock(return_value="Підписант")
    return crypto


XML_WITH_NO_1 = (
    '<DAT FN="4538765845" TN="ПН 345612052809" ZN="АА57506761" DI="1" V="2.1.7">'
    '<C T="0"><E N="1" NO="1" SM="2500" TX="0"></E></C>'
    "<TS>20260827120000</TS></DAT>"
)
XML_WITH_NO_99 = XML_WITH_NO_1.replace('NO="1"', 'NO="99"')


@pytest.fixture
def key_store(tmp_path):
    return PrroKeyStore(
        master_key="g4vCmo5ErFfkwhhXghxBbso6GmM-OXsWMStjP27YVEs=",
        keystore_path=tmp_path / "keystore.json",
        master_key_path=tmp_path / "master.key",
    )


@pytest.fixture
def setup(session: AsyncSession, key_store):
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)
    service_factory = MagicMock()
    context = PrroContextFactory(
        settings_repo=settings_repo,
        key_store=key_store,
        service_factory=service_factory,
    )
    offline_queue = PrroOfflineQueue(prro_repo)

    async def _make(*, grpc=None, crypto=None):
        await settings_repo.set(KEY_PRRO_FN, "4538765845")
        await settings_repo.set(KEY_PRRO_TN, "ПН 345612052809")
        await settings_repo.set(KEY_PRRO_ZN, "АА57506761")
        key_store.save_key_path(str(key_store._keystore_path.parent / "key.pfx"), key_format="pfx")
        key_store.save_password_encrypted("secret")

        cashier = User(
            id=uuid4(), name="Касир", login=f"cashier-{uuid4().hex[:8]}",
            password_hash="hash", pin_code="0000",
            role=UserRole.CASHIER, is_active=True,
        )
        session.add(cashier)
        product = Product(
            id=uuid4(), title="Товар", price=Decimal("100.00"),
            stock=Decimal("3"), fiscal_stock=Decimal("10"),
            is_fiscal=True, tax_rate=Decimal("20.00"), unit="шт",
        )
        session.add(product)
        receipt = Receipt(
            id=uuid4(), receipt_number="SALE-H1", cashier_id=cashier.id,
            is_return=False, payment_method="cash",
            total_amount=300.0, cash_amount=300.0, card_amount=0.0,
            change_amount=0.0, is_fiscal=True,
        )
        session.add(receipt)
        session.add(
            ReceiptItem(
                id=uuid4(), receipt_id=receipt.id, product_id=product.id,
                quantity=Decimal("3"), price=Decimal("100.00"),
                total=300.0, fiscal_quantity=Decimal("3"),
            )
        )
        shift = PrroShift(
            id=uuid4(), shift_number=1, opened_at=datetime.utcnow(),
            status="open", receipt_count=0, total_amount=0,
            last_local_number=0, last_mac=None,
        )
        session.add(shift)
        await session.flush()

        grpc_client = grpc or make_grpc()
        crypto_signer = crypto or make_crypto()
        context.grpc_client = AsyncMock(return_value=grpc_client)
        context.build_crypto_signer = AsyncMock(return_value=crypto_signer)
        fiscalizer = FiscalizeReceiptUseCase(
            session=session, prro_repo=prro_repo, settings_repo=settings_repo,
            context_factory=context, offline_queue=offline_queue,
        )
        return {
            "fiscalizer": fiscalizer, "receipt": receipt, "shift": shift,
            "grpc": grpc_client, "prro_repo": prro_repo,
            "settings_repo": settings_repo,
        }

    return _make


class TestTimeoutRecoveryH1:
    async def test_lastchk_finds_check_marks_sent_no_duplicate(self, setup):
        """H1 сценарій 1: timeout → lastChk знаходить наш чек (NO=1) → SENT;
        send_chk викликано РІВНО 1 раз (без дубліката)."""
        grpc = make_grpc(
            send_side_effect=RuntimeError("deadline exceeded"),
            last_response=SimpleNamespace(
                status=1, id="FISCAL-TIMEOUT-1", id_sign=b"id-sign-timeout",
                data_sign=XML_WITH_NO_1.encode("utf-8"), error_message="",
            ),
        )
        data = await setup(grpc=grpc)
        result = await data["fiscalizer"].fiscalize_receipt(data["receipt"].id, manual=True)

        assert result.fiscal_status == "sent"
        assert result.fiscal_number == "FISCAL-TIMEOUT-1"
        assert grpc.send_chk.await_count == 1, "жодного повторного send (без дубліката)"
        assert grpc.last_chk.await_count == 1

    async def test_lastchk_not_found_then_retry_succeeds(self, setup):
        """H1 сценарій 2: lastChk не знаходить (NO=99) → 1 контрольований
        повторний send → SENT."""
        grpc = make_grpc(
            send_side_effect=[
                RuntimeError("deadline exceeded"),
                make_response(id="FISCAL-RETRY-1"),
            ],
            last_response=SimpleNamespace(
                status=1, id="OTHER-CHK", id_sign=b"",
                data_sign=XML_WITH_NO_99.encode("utf-8"), error_message="",
            ),
        )
        data = await setup(grpc=grpc)
        result = await data["fiscalizer"].fiscalize_receipt(data["receipt"].id, manual=True)

        assert result.fiscal_status == "sent"
        assert result.fiscal_number == "FISCAL-RETRY-1"
        assert grpc.send_chk.await_count == 2, "рівно 1 повторний send"

    async def test_retry_fails_document_queued_and_offline(self, setup):
        """H1 сценарій 3: lastChk не знаходить, повторний send теж транспортна
        помилка → документ у черзі (failed), ПРРО → offline."""
        grpc = make_grpc(
            send_side_effect=RuntimeError("deadline exceeded"),
            last_response=SimpleNamespace(
                status=1, id="OTHER-CHK", id_sign=b"",
                data_sign=XML_WITH_NO_99.encode("utf-8"), error_message="",
            ),
        )
        data = await setup(grpc=grpc)
        result = await data["fiscalizer"].fiscalize_receipt(data["receipt"].id, manual=True)

        assert result.fiscal_status == "failed", "документ у черзі (failed)"
        assert result.error is not None
        # документ НЕ втрачений: лишився в черзі
        items = await data["prro_repo"].list_by_shift(data["shift"].id)
        assert len(items) == 1
        assert items[0].local_number == 1
        # ПРРО → offline (B4)
        from app.infrastructure.services.prro.offline_state import OfflineStateMachine
        assert await OfflineStateMachine.is_offline(data["settings_repo"]) is True
        # принаймні 2 спроби нашого чека (первинна + retry)
        assert grpc.send_chk.await_count >= 2

    async def test_extract_check_no_parses_no(self):
        assert extract_check_no(XML_WITH_NO_1) == 1
        assert extract_check_no(XML_WITH_NO_99) == 99
        assert extract_check_no("<DAT><C T='0'><P N='1'></P></C></DAT>") is None


class TestQrMacV1:
    async def test_fiscalize_qr_uses_check_mac_not_id_sign(self, setup):
        """V1: QR mac = MAC чека (SHA-256 base64), а НЕ id_sign."""
        data = await setup()
        result = await data["fiscalizer"].fiscalize_receipt(data["receipt"].id, manual=True)

        url = result.fiscal_check_url
        assert url is not None
        assert url.startswith("https://cabinet.tax.gov.ua/cashregs/check?")
        mac_param = url.split("mac=")[1].split("&")[0]
        assert "id-sign" not in mac_param, f"id_sign не має потрапляти в QR: {url}"
        # SHA-256 base64: 44 символи + паддінг '='
        import base64
        from urllib.parse import unquote

        decoded = base64.b64decode(unquote(mac_param), validate=True)
        assert len(decoded) == 32, "SHA-256 = 32 байти"

    async def test_qr_parameters_match_dps(self, setup):
        """V1: параметри QR відповідають ДПС §5 (mac/date/time/id/sm/fn)."""
        from app.infrastructure.services.prro.qr_url import build_fiscal_check_url
        from datetime import timezone

        url = build_fiscal_check_url(
            fiscal_number="45",
            amount=Decimal("780.00"),
            prro_fn="3000898168",
            sent_at=datetime(2022, 9, 4, 11, 30, tzinfo=timezone.utc),
            mac="001A000005F00000000015001146D50002924A03E61CA20AF7C297A2D6",
        )
        assert url == (
            "https://cabinet.tax.gov.ua/cashregs/check"
            "?mac=001A000005F00000000015001146D50002924A03E61CA20AF7C297A2D6"
            "&date=20220904&time=1130&id=45&sm=780.00&fn=3000898168"
        )
