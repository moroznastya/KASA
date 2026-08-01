"""Unit tests: фіскалізація повернення (2.3).

Покриває:
  - повернення (T=1) збільшує fiscal_stock;
  - XML містить RT="0" (повернення товару);
  - повернення без original_receipt_id → не фіскалізується (none);
  - повернення, оригінал якого не фіскалізований → не фіскалізується (none);
  - повернення з фіскальним оригіналом → фіскалізується (sent).
"""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

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


def make_response(*, status: int = 1, id: str = "FISCAL-RET-1", error_message: str = ""):
    return SimpleNamespace(
        status=status, id=id, error_message=error_message, id_sign=b"id-sign-ret"
    )


def make_grpc(send_response=None):
    client = MagicMock()
    client.send_chk = AsyncMock(return_value=send_response or make_response())
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


async def _reload_receipt(session: AsyncSession, receipt_id) -> Receipt:
    stmt = (
        select(Receipt)
        .options(selectinload(Receipt.items))
        .where(Receipt.id == receipt_id)
        .execution_options(populate_existing=True)
    )
    return (await session.execute(stmt)).scalar_one()


@pytest.fixture
def setup(session: AsyncSession, key_store):
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)

    async def _make(
        *,
        product_stock: Decimal = Decimal("5"),
        fiscal_quantity: Decimal = Decimal("2"),
        quantity: Decimal = Decimal("2"),
        original_status: str = "sent",   # статус оригінального чека
        with_original: bool = True,
    ):
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
            stock=Decimal("20"), fiscal_stock=product_stock,
            is_fiscal=True, tax_rate=Decimal("20.00"), unit="шт",
        )
        session.add(product)

        original = None
        if with_original:
            original = Receipt(
                id=uuid4(), receipt_number="ORIG-001", cashier_id=cashier.id,
                is_return=False, payment_method="cash",
                total_amount=float(quantity * 100),
                is_fiscal=True, fiscal_status=original_status,
                fiscal_number="FISCAL-ORIG",
            )
            session.add(original)

        receipt = Receipt(
            id=uuid4(), receipt_number="RET-001", cashier_id=cashier.id,
            is_return=True, payment_method="cash",
            total_amount=float(quantity * 100),
            is_fiscal=True,
            original_receipt_id=original.id if original else None,
        )
        session.add(receipt)
        session.add(ReceiptItem(
            id=uuid4(), receipt_id=receipt.id, product_id=product.id,
            quantity=quantity, price=Decimal("100.00"),
            total=float(quantity * 100), fiscal_quantity=fiscal_quantity,
        ))

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
        grpc_client = make_grpc()
        context.grpc_client = AsyncMock(return_value=grpc_client)
        context.build_crypto_signer = AsyncMock(return_value=make_crypto())

        fiscalizer = FiscalizeReceiptUseCase(
            session=session,
            prro_repo=prro_repo,
            settings_repo=settings_repo,
            context_factory=context,
            offline_queue=PrroOfflineQueue(prro_repo),
        )
        return {
            "fiscalizer": fiscalizer,
            "receipt": receipt,
            "product": product,
            "grpc": grpc_client,
            "session": session,
            "prro_repo": prro_repo,
        }

    return _make


class TestFiscalizeReturn:
    async def test_return_fiscalizes_t1_and_increases_stock(self, setup):
        """Повернення з фіскальним оригіналом: T=1, RT='0', fiscal_stock += qty."""
        data = await setup()
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"
        assert result.fiscal_number == "FISCAL-RET-1"

        # fiscal_stock: 5 + 2 = 7
        await data["session"].refresh(data["product"])
        assert data["product"].fiscal_stock == 7

        # XML містить RT="0" (повернення товару) та T="1"
        items = await data["prro_repo"].list_by_receipt(data["receipt"].id)
        assert len(items) == 1
        assert 'RT="0"' in items[0].xml_body
        assert 'T="1"' in items[0].xml_body
        assert items[0].status.value == "sent"

    async def test_return_without_original_not_fiscalized(self, setup):
        """Повернення без original_receipt_id → статус none."""
        data = await setup(with_original=False)
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "none"
        assert "original_receipt_id" in (result.error or "")
        data["grpc"].send_chk.assert_not_awaited()

    async def test_return_original_not_sent_not_fiscalized(self, setup):
        """Оригінал не фіскалізований → повернення не фіскалізується."""
        data = await setup(original_status="pending")
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "none"
        assert "не фіскалізований" in (result.error or "")
        data["grpc"].send_chk.assert_not_awaited()

    async def test_return_original_failed_not_fiscalized(self, setup):
        """Оригінал зі статусом failed → повернення не фіскалізується."""
        data = await setup(original_status="failed")
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "none"
        data["grpc"].send_chk.assert_not_awaited()
