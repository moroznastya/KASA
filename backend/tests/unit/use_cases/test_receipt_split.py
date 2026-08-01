"""Unit tests: спліт чеку при частковій фіскалізації (2.2).

Покриває:
  - змішаний чек (фіскальні + нефіскальні позиції) → split:
    оригінальний чек стає фіскальним, створюється нефіскальний дублікат
    з split_group_id = id фіскального чека;
  - суми перераховуються пропорційно фіскальній кількості;
  - повністю фіскальний чек → без split;
  - повністю нефіскальний (fiscal_stock=0) → статус none, без split.
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


def make_response(*, status: int = 1, id: str = "FISCAL-100", error_message: str = ""):
    """Імітує CheckResponse gRPC."""
    return SimpleNamespace(
        status=status, id=id, error_message=error_message, id_sign=b"id-sign-100"
    )


def make_grpc(send_response=None):
    """Імітує PrroGrpcClient."""
    client = MagicMock()
    client.send_chk = AsyncMock(return_value=send_response or make_response())
    client.last_chk = AsyncMock(return_value=make_response())
    return client


def make_crypto():
    """Імітує PrroCryptoSigner."""
    crypto = MagicMock()
    crypto.sign = MagicMock(return_value=b"<signed/>")
    crypto.get_serial_number = MagicMock(return_value="SERIAL-1")
    crypto.get_signer_name = MagicMock(return_value="Підписант")
    return crypto


@pytest.fixture
def key_store(tmp_path):
    """PrroKeyStore з tmp-файлами."""
    return PrroKeyStore(
        master_key="g4vCmo5ErFfkwhhXghxBbso6GmM-OXsWMStjP27YVEs=",
        keystore_path=tmp_path / "keystore.json",
        master_key_path=tmp_path / "master.key",
    )


@pytest.fixture
def setup(session: AsyncSession, key_store):
    """Створює змішану ситуацію: 2 товари (фіскальний/нефіскальний) та чек."""
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)

    async def _make(
        *,
        fiscal_stock: Decimal = Decimal("10"),
        fiscal_qty: Decimal = Decimal("3"),   # fiscal_quantity у чеку
        quantity: Decimal = Decimal("3"),
        non_fiscal_qty: Decimal = Decimal("2"),  # позиція без fiscal_quantity
        with_shift: bool = True,
        is_return: bool = False,
    ):
        # ПРРО налаштований
        await settings_repo.set(KEY_PRRO_FN, "4538765845")
        await settings_repo.set(KEY_PRRO_TN, "ПН 345612052809")
        await settings_repo.set(KEY_PRRO_ZN, "АА57506761")
        key_store.save_key_path(str(key_store._keystore_path.parent / "key.pfx"), key_format="pfx")
        key_store.save_password_encrypted("secret")

        cashier = User(
            id=uuid4(), name="Касир",
            login=f"cashier-{uuid4().hex[:8]}",
            password_hash="hash", pin_code="0000",
            role=UserRole.CASHIER, is_active=True,
        )
        session.add(cashier)

        fiscal_product = Product(
            id=uuid4(), title="Фіскальний товар", price=Decimal("100.00"),
            stock=quantity + Decimal("5"), fiscal_stock=fiscal_stock,
            is_fiscal=True, tax_rate=Decimal("20.00"), unit="шт",
        )
        session.add(fiscal_product)
        non_fiscal_product = Product(
            id=uuid4(), title="Звичайний товар", price=Decimal("50.00"),
            stock=non_fiscal_qty + Decimal("5"), fiscal_stock=Decimal("0"),
            is_fiscal=False, tax_rate=Decimal("20.00"), unit="шт",
        )
        session.add(non_fiscal_product)

        receipt = Receipt(
            id=uuid4(), receipt_number="SALE-MIX-001", cashier_id=cashier.id,
            is_return=is_return, payment_method="cash",
            total_amount=float(quantity * 100 + non_fiscal_qty * 50),
            is_fiscal=True,
        )
        session.add(receipt)
        session.add(ReceiptItem(
            id=uuid4(), receipt_id=receipt.id, product_id=fiscal_product.id,
            quantity=quantity, price=Decimal("100.00"),
            total=float(quantity * 100), fiscal_quantity=fiscal_qty,
        ))
        session.add(ReceiptItem(
            id=uuid4(), receipt_id=receipt.id, product_id=non_fiscal_product.id,
            quantity=non_fiscal_qty, price=Decimal("50.00"),
            total=float(non_fiscal_qty * 50), fiscal_quantity=Decimal("0"),
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
        grpc_client = make_grpc()
        crypto_signer = make_crypto()
        context.grpc_client = AsyncMock(return_value=grpc_client)
        context.build_crypto_signer = AsyncMock(return_value=crypto_signer)

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
            "fiscal_product": fiscal_product,
            "non_fiscal_product": non_fiscal_product,
            "shift": shift,
            "prro_repo": prro_repo,
            "session": session,
        }

    return _make


async def _reload_receipt(session: AsyncSession, receipt_id) -> Receipt:
    """Перезавантажує чек з позиціями (свіжі enum-значення)."""
    stmt = (
        select(Receipt)
        .options(selectinload(Receipt.items))
        .where(Receipt.id == receipt_id)
        .execution_options(populate_existing=True)
    )
    return (await session.execute(stmt)).scalar_one()


class TestReceiptSplit:
    async def test_split_mixed_receipt(self, setup):
        """Змішаний чек → split: фіскальний + нефіскальний дублікат."""
        data = await setup(
            fiscal_stock=Decimal("10"),
            fiscal_qty=Decimal("3"),
            quantity=Decimal("3"),
            non_fiscal_qty=Decimal("2"),
        )
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"
        assert result.split_receipt_id is not None
        assert "Часткова фіскалізація" in (result.warning or "")

        session = data["session"]

        # Оригінальний чек → фіскальний: лише 1 позиція (нефіскальна видалена)
        fresh = await _reload_receipt(session, data["receipt"].id)
        assert fresh.is_fiscal is True
        assert fresh.fiscal_status.value == "sent"
        assert len(fresh.items) == 1
        assert fresh.items[0].fiscal_quantity == 3
        assert float(fresh.items[0].quantity) == 3.0
        assert float(fresh.total_amount) == 300.0

        # Нефіскальний дублікат: split_group_id = id фіскального чека
        dup = await _reload_receipt(session, result.split_receipt_id)
        assert dup is not None
        assert dup.is_fiscal is False
        assert dup.fiscal_status.value == "none"
        assert dup.split_group_id == data["receipt"].id
        assert len(dup.items) == 1
        assert float(dup.items[0].quantity) == 2.0
        assert dup.items[0].fiscal_quantity == 0
        assert float(dup.total_amount) == 100.0

        # Лічильники зміни — лише фіскальна частина (300 грн)
        await session.refresh(data["shift"])
        assert float(data["shift"].total_amount) == 300.0

    async def test_split_when_stock_partially_covers(self, setup):
        """fiscal_stock частково покриває: фіскалізовано min, решта у дублікат."""
        data = await setup(
            fiscal_stock=Decimal("2"),  # покриває лише 2 з 3
            fiscal_qty=Decimal("3"),
            quantity=Decimal("3"),
            non_fiscal_qty=Decimal("0"),
        )
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"
        assert result.split_receipt_id is not None

        session = data["session"]
        fresh = await _reload_receipt(session, data["receipt"].id)
        assert len(fresh.items) == 1
        assert fresh.items[0].fiscal_quantity == 2
        assert float(fresh.items[0].quantity) == 2.0
        assert float(fresh.total_amount) == 200.0

        # Фіскальний залишок: 2 - 2 = 0
        await session.refresh(data["fiscal_product"])
        assert data["fiscal_product"].fiscal_stock == 0

    async def test_no_split_fully_fiscal(self, setup):
        """Повністю фіскальний чек → без split."""
        data = await setup(
            fiscal_stock=Decimal("10"),
            fiscal_qty=Decimal("3"),
            quantity=Decimal("3"),
            non_fiscal_qty=Decimal("0"),
        )
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"
        assert result.split_receipt_id is None
        assert "Часткова фіскалізація" not in (result.warning or "")

    async def test_no_split_fully_non_fiscal(self, setup):
        """fiscal_stock=0 для всіх позицій → чек повністю нефіскальний (none)."""
        data = await setup(
            fiscal_stock=Decimal("0"),
            fiscal_qty=Decimal("3"),
            quantity=Decimal("3"),
            non_fiscal_qty=Decimal("2"),
        )
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "none"
        assert result.split_receipt_id is None
        # Оригінальний чек позначено нефіскальним, дублікат НЕ створюється
        session = data["session"]
        fresh = await _reload_receipt(session, data["receipt"].id)
        assert fresh.fiscal_status.value == "none"
        assert fresh.is_fiscal is False
        # Позиції збережені, fiscal_quantity = 0
        assert len(fresh.items) == 2
        assert all(i.fiscal_quantity == 0 for i in fresh.items)
