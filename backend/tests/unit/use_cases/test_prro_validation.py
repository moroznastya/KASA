"""Unit tests: валідація перед фіскалізацією (2.4).

Покриває помилки:
  - PRRO_NOT_CONFIGURED: не задано prro_fn;
  - PRRO_NOT_CONFIGURED: ключ КЕП не збережено;
  - PRRO_ALREADY_FISCALIZED: чек вже фіскалізований;
  - PRRO_ZERO_TOTAL: сума чека ≤ 0;
  - ERROR_NOT_OPEN_SHIFT: зміна не відкрита.
"""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.application.use_cases.prro.context import PrroContextFactory
from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    FiscalizeReceiptUseCase,
    PrroFiscalizeError,
)
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.prro import PrroShift
from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem
from app.infrastructure.persistence.models.user import User, UserRole
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.key_store import PrroKeyStore
from app.infrastructure.services.prro.offline_queue import PrroOfflineQueue


def make_response(*, status: int = 1, id: str = "FISCAL-100"):
    return SimpleNamespace(status=status, id=id, error_message="", id_sign=b"id")


def make_grpc():
    client = MagicMock()
    client.send_chk = AsyncMock(return_value=make_response())
    client.last_chk = AsyncMock(return_value=make_response())
    return client


def make_crypto():
    crypto = MagicMock()
    crypto.sign = MagicMock(return_value=b"<signed/>")
    crypto.get_serial_number = MagicMock(return_value="SERIAL-1")
    crypto.get_signer_name = MagicMock(return_value="Підписант")
    return crypto


@pytest.fixture
def key_store(tmp_path):
    return PrroKeyStore(
        master_key="g4vCmo5ErFfkwhhXghxBbso6GmM-OXsWMStjP27YVEs=",
        keystore_path=tmp_path / "keystore.json",
        master_key_path=tmp_path / "master.key",
    )


@pytest.fixture
def setup(session: AsyncSession, key_store):
    """Базовий фіскалізатор з можливістю налаштування стану ПРРО."""
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)

    async def _make(
        *,
        with_fn: bool = True,
        with_key: bool = True,
        fiscal_status: str = "pending",
        total_amount: Decimal = Decimal("300.00"),
        with_shift: bool = True,
    ):
        if with_fn:
            await settings_repo.set("prro_fn", "4538765845")
        if with_key:
            key_store.save_key_path(
                str(key_store._keystore_path.parent / "key.pfx"), key_format="pfx"
            )
            key_store.save_password_encrypted("secret")

        cashier = User(
            id=uuid4(), name="Касир", login=f"cashier-{uuid4().hex[:8]}",
            password_hash="hash", pin_code="0000",
            role=UserRole.CASHIER, is_active=True,
        )
        session.add(cashier)

        product = Product(
            id=uuid4(), title="Товар", price=Decimal("100.00"),
            stock=Decimal("20"), fiscal_stock=Decimal("10"),
            is_fiscal=True, tax_rate=Decimal("20.00"), unit="шт",
        )
        session.add(product)

        receipt = Receipt(
            id=uuid4(), receipt_number="SALE-VAL-001", cashier_id=cashier.id,
            is_return=False, payment_method="cash",
            total_amount=float(total_amount),
            is_fiscal=True, fiscal_status=fiscal_status,
            fiscal_number="F-1" if fiscal_status == "sent" else None,
        )
        session.add(receipt)
        session.add(ReceiptItem(
            id=uuid4(), receipt_id=receipt.id, product_id=product.id,
            quantity=Decimal("3"), price=Decimal("100.00"),
            total=float(Decimal("300.00")), fiscal_quantity=Decimal("3"),
        ))

        shift = None
        if with_shift:
            shift = PrroShift(
                id=uuid4(), shift_number=1, opened_at=datetime.utcnow(),
                status="open", receipt_count=0, total_amount=0,
                last_local_number=0, last_mac=None,
            )
            session.add(shift)
        await session.flush()

        service_factory = MagicMock()
        context = PrroContextFactory(
            settings_repo=settings_repo,
            key_store=key_store,
            service_factory=service_factory,
        )
        context.grpc_client = AsyncMock(return_value=make_grpc())
        context.build_crypto_signer = AsyncMock(return_value=make_crypto())

        fiscalizer = FiscalizeReceiptUseCase(
            session=session,
            prro_repo=prro_repo,
            settings_repo=settings_repo,
            context_factory=context,
            offline_queue=PrroOfflineQueue(prro_repo),
        )
        return {"fiscalizer": fiscalizer, "receipt": receipt}

    return _make


class TestPrroValidation:
    async def test_error_not_configured_no_fn(self, setup):
        """Без prro_fn → PrroFiscalizeError PRRO_NOT_CONFIGURED."""
        data = await setup(with_fn=False)
        with pytest.raises(PrroFiscalizeError) as exc:
            await data["fiscalizer"].fiscalize_receipt(
                data["receipt"].id, manual=True
            )
        assert exc.value.code == "PRRO_NOT_CONFIGURED"
        assert "prro_fn" in exc.value.args[0]

    async def test_error_not_configured_no_key(self, setup):
        """Без ключа КЕП → PrroFiscalizeError PRRO_NOT_CONFIGURED."""
        data = await setup(with_key=False)
        with pytest.raises(PrroFiscalizeError) as exc:
            await data["fiscalizer"].fiscalize_receipt(
                data["receipt"].id, manual=True
            )
        assert exc.value.code == "PRRO_NOT_CONFIGURED"

    async def test_error_already_fiscalized(self, setup):
        """Чек вже фіскалізований (fiscal_status=sent) → PRRO_ALREADY_FISCALIZED."""
        data = await setup(fiscal_status="sent")
        with pytest.raises(PrroFiscalizeError) as exc:
            await data["fiscalizer"].fiscalize_receipt(
                data["receipt"].id, manual=True
            )
        assert exc.value.code == "PRRO_ALREADY_FISCALIZED"

    async def test_error_zero_total(self, setup):
        """Сума чека ≤ 0 → PRRO_ZERO_TOTAL."""
        data = await setup(total_amount=Decimal("0"))
        with pytest.raises(PrroFiscalizeError) as exc:
            await data["fiscalizer"].fiscalize_receipt(
                data["receipt"].id, manual=True
            )
        assert exc.value.code == "PRRO_ZERO_TOTAL"

    async def test_error_not_open_shift(self, setup):
        """Зміна не відкрита → ERROR_NOT_OPEN_SHIFT."""
        data = await setup(with_shift=False)
        with pytest.raises(PrroFiscalizeError) as exc:
            await data["fiscalizer"].fiscalize_receipt(
                data["receipt"].id, manual=True
            )
        assert exc.value.code == "ERROR_NOT_OPEN_SHIFT"

    async def test_validation_ok(self, setup):
        """Всі перевірки пройдені → фіскалізація успішна."""
        data = await setup()
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )
        assert result.fiscal_status == "sent"
