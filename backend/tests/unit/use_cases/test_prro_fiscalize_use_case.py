"""Unit tests: FiscalizeReceiptUseCase — фіскалізація чеку через ПРРО."""

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
    KEY_AUTO_FISCALIZE,
    KEY_PRRO_FN,
    KEY_PRRO_TN,
    KEY_PRRO_ZN,
)
from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    FiscalizeReceiptUseCase,
    PrroFiscalizeError,
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


# ─── Допоміжні фабрики ──────────────────────────────────────────────────────

def make_response(*, status: int = 1, id: str = "FISCAL-100", error_message: str = ""):
    """Імітує CheckResponse gRPC."""
    return SimpleNamespace(
        status=status,
        id=id,
        error_message=error_message,
        id_sign=b"id-sign-100",
    )


def make_grpc(send_response=None, last_response=None):
    """Імітує PrroGrpcClient."""
    client = MagicMock()
    client.send_chk = AsyncMock(return_value=send_response or make_response())
    client.last_chk = AsyncMock(return_value=last_response or make_response())
    return client


def make_crypto():
    """Імітує PrroCryptoSigner."""
    crypto = MagicMock()
    crypto.sign = MagicMock(return_value=b"<signed/>")
    crypto.get_serial_number = MagicMock(return_value="SERIAL-1")
    crypto.get_signer_name = MagicMock(return_value="Підписант")
    return crypto


# ─── Фікстури ───────────────────────────────────────────────────────────────

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
    """Створює товар/чек/зміну та повертає fiscalizer з моками."""
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)
    service_factory = MagicMock()
    context = PrroContextFactory(
        settings_repo=settings_repo,
        key_store=key_store,
        service_factory=service_factory,
    )
    offline_queue = PrroOfflineQueue(prro_repo)

    async def _make(
        *,
        product_stock: Decimal = Decimal("10"),
        fiscal_quantity: Decimal = Decimal("3"),
        quantity: Decimal = Decimal("3"),
        price: Decimal = Decimal("100.00"),
        is_return: bool = False,
        payment_method: str = "cash",
        grpc=None,
        crypto=None,
        with_shift: bool = True,
        with_original: bool = False,
    ):
        # Налаштовуємо ПРРО (валiдація 2.4)
        await settings_repo.set(KEY_PRRO_FN, "4538765845")
        await settings_repo.set(KEY_PRRO_TN, "ПН 345612052809")
        await settings_repo.set(KEY_PRRO_ZN, "АА57506761")
        key_store.save_key_path(str(key_store._keystore_path.parent / "key.pfx"), key_format="pfx")
        key_store.save_password_encrypted("secret")

        cashier = User(
            id=uuid4(),
            name="Тестовий Касир",
            login=f"cashier-{uuid4().hex[:8]}",
            password_hash="hash",
            pin_code="0000",
            role=UserRole.CASHIER,
            is_active=True,
        )
        session.add(cashier)

        product = Product(
            id=uuid4(),
            title="Тестовий товар",
            price=price,
            stock=quantity,
            fiscal_stock=product_stock,
            is_fiscal=True,
            tax_rate=Decimal("20.00"),
            unit="шт",
        )
        session.add(product)

        original_receipt = None
        if is_return and with_original:
            original_receipt = Receipt(
                id=uuid4(),
                receipt_number="ORIG-001",
                cashier_id=cashier.id,
                is_return=False,
                payment_method="cash",
                total_amount=float(quantity * price),
                is_fiscal=True,
                fiscal_status="sent",
                fiscal_number="FISCAL-ORIG-1",
            )
            session.add(original_receipt)

        receipt = Receipt(
            id=uuid4(),
            receipt_number="SALE-001",
            cashier_id=cashier.id,
            is_return=is_return,
            payment_method=payment_method,
            total_amount=float(quantity * price),
            is_fiscal=True,
            original_receipt_id=original_receipt.id if original_receipt else None,
        )
        session.add(receipt)
        receipt_item = ReceiptItem(
            id=uuid4(),
            receipt_id=receipt.id,
            product_id=product.id,
            quantity=quantity,
            price=price,
            total=float(quantity * price),
            fiscal_quantity=fiscal_quantity,
        )
        session.add(receipt_item)

        shift = None
        if with_shift:
            shift = PrroShift(
                id=uuid4(),
                shift_number=1,
                opened_at=datetime.utcnow(),
                status="open",
                receipt_count=0,
                total_amount=0,
                last_local_number=0,
                last_mac=None,
            )
            session.add(shift)

        await session.flush()

        grpc_client = grpc or make_grpc()
        crypto_signer = crypto or make_crypto()
        context.grpc_client = AsyncMock(return_value=grpc_client)
        context.build_crypto_signer = AsyncMock(return_value=crypto_signer)

        fiscalizer = FiscalizeReceiptUseCase(
            session=session,
            prro_repo=prro_repo,
            settings_repo=settings_repo,
            context_factory=context,
            offline_queue=offline_queue,
        )
        return {
            "fiscalizer": fiscalizer,
            "receipt": receipt,
            "product": product,
            "shift": shift,
            "prro_repo": prro_repo,
            "settings_repo": settings_repo,
            "grpc": grpc_client,
            "crypto": crypto_signer,
            "session": session,
        }

    return _make


# ─── Тести ──────────────────────────────────────────────────────────────────

class TestFiscalizeReceipt:
    async def test_fiscalize_ok(self, setup):
        """Успішна фіскалізація: чек SENT, залишок зменшено, черга sent."""
        data = await setup()
        fiscalizer = data["fiscalizer"]

        result = await fiscalizer.fiscalize_receipt(data["receipt"].id, manual=True)

        assert result.fiscal_status == "sent"
        assert result.fiscal_number == "FISCAL-100"
        assert result.fiscal_serial == "id-sign-100"
        assert result.fiscal_sent_at is not None
        assert result.error is None

        # 2.6 QR-код: URL перевірки фіскального чеку
        assert result.fiscal_check_url is not None
        assert "cabinet.tax.gov.ua/cashregs/check" in result.fiscal_check_url
        assert "id=FISCAL-100" in result.fiscal_check_url
        assert "fn=4538765845" in result.fiscal_check_url

        # Чек оновлено
        await data["session"].refresh(data["receipt"])
        assert data["receipt"].fiscal_status.value == "sent"
        assert data["receipt"].fiscal_number == "FISCAL-100"

        # Залишок зменшено: 10 - 3 = 7
        await data["session"].refresh(data["product"])
        assert data["product"].fiscal_stock == 7

        # Лічильники зміни: 1 чек, сума 300
        await data["session"].refresh(data["shift"])
        assert data["shift"].receipt_count == 1
        assert float(data["shift"].total_amount) == 300.00
        assert data["shift"].last_local_number == 1

        # Черга: CHK #1 sent
        items = await data["prro_repo"].list_by_shift(data["shift"].id)
        assert len(items) == 1
        assert items[0].check_type == "CHK"
        assert items[0].status.value == "sent"

    async def test_fiscalize_no_fiscal_items(self, setup):
        """Немає фіскальних позицій → без дій (none)."""
        data = await setup(fiscal_quantity=Decimal("0"))
        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )
        assert result.fiscal_status == "none"
        assert "Немає фіскальних позицій" in (result.error or "")

    async def test_fiscalize_not_open_shift(self, setup):
        """Зміна не відкрита → PrroFiscalizeError ERROR_NOT_OPEN_SHIFT."""
        data = await setup(with_shift=False)
        with pytest.raises(PrroFiscalizeError) as exc_info:
            await data["fiscalizer"].fiscalize_receipt(
                data["receipt"].id, manual=True
            )
        assert exc_info.value.code == "ERROR_NOT_OPEN_SHIFT"

    async def test_fiscalize_receipt_not_found(self, setup):
        """Чек не знайдено → RECEIPT_NOT_FOUND."""
        data = await setup()
        with pytest.raises(PrroFiscalizeError) as exc_info:
            await data["fiscalizer"].fiscalize_receipt(uuid4(), manual=True)
        assert exc_info.value.code == "RECEIPT_NOT_FOUND"

    async def test_fiscalize_server_error(self, setup):
        """Помилка сервера → чек FAILED, черга failed."""
        grpc = make_grpc(
            send_response=make_response(
                status=-4, error_message="Unknown error"
            )
        )
        data = await setup(grpc=grpc)

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "failed"
        assert result.error == "Unknown error"

        await data["session"].refresh(data["receipt"])
        assert data["receipt"].fiscal_status.value == "failed"
        assert data["receipt"].fiscal_error == "Unknown error"

        items = await data["prro_repo"].list_by_shift(data["shift"].id)
        assert items[0].status.value == "failed"
        assert items[0].error == "Unknown error"

    async def test_fiscalize_partial_when_stock_short(self, setup):
        """Нестача fiscal_stock → ЧАСТКОВА фіскалізація + warning."""
        data = await setup(
            product_stock=Decimal("2"),   # залишок менше запланованих 3
            fiscal_quantity=Decimal("3"),
            quantity=Decimal("3"),
        )

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"
        assert result.warning is not None
        # Спліт: часткова фіскалізація → нефіскальний дублікат
        assert "Часткова фіскалізація" in result.warning
        assert result.split_receipt_id is not None

        # Залишок: 2 - 2 = 0
        await data["session"].refresh(data["product"])
        assert data["product"].fiscal_stock == 0

        # Сума фіскалізована за 2 шт: 200
        await data["session"].refresh(data["shift"])
        assert float(data["shift"].total_amount) == 200.00

        # Оригінальний чек став фіскальним з quantity=2
        await data["session"].refresh(data["receipt"], ["items"])
        assert data["receipt"].total_amount == 200
        assert len(data["receipt"].items) == 1
        assert data["receipt"].items[0].fiscal_quantity == 2

    async def test_fiscalize_return_increases_stock(self, setup):
        """Повернення: fiscal_stock збільшується, обмеження залишку немає."""
        data = await setup(
            product_stock=Decimal("5"),
            fiscal_quantity=Decimal("2"),
            quantity=Decimal("2"),
            is_return=True,
            with_original=True,
        )

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"
        await data["session"].refresh(data["product"])
        assert data["product"].fiscal_stock == 7

    async def test_fiscalize_dedup_on_error_save(self, setup):
        """ERROR_SAVE → lastChk знаходить чек → дедуплікація (SENT)."""
        grpc = make_grpc(
            send_response=make_response(status=-3, error_message="save error"),
            last_response=make_response(status=1, id="FISCAL-DUP"),
        )
        data = await setup(grpc=grpc)

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"
        assert result.fiscal_number == "FISCAL-DUP"
        data["grpc"].last_chk.assert_awaited_once()

    async def test_fiscalize_auto_disabled(self, setup):
        """auto_fiscalize вимкнено + manual=False → без дій."""
        data = await setup()
        await data["settings_repo"].set(KEY_AUTO_FISCALIZE, "false")

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=False
        )

        assert result.fiscal_status == "none"
        data["grpc"].send_chk.assert_not_awaited()

    async def test_fiscalize_auto_enabled(self, setup):
        """auto_fiscalize увімкнено + manual=False → фіскалізація виконується."""
        data = await setup()
        await data["settings_repo"].set(KEY_AUTO_FISCALIZE, "true")

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=False
        )

        assert result.fiscal_status == "sent"
        data["grpc"].send_chk.assert_awaited_once()

    async def test_fiscalize_card_payment(self, setup):
        """Оплата карткою → код оплати '1'."""
        data = await setup(payment_method="card")

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "sent"

    async def test_fiscalize_zero_stock(self, setup):
        """fiscal_stock=0 → чек повністю нефіскальний (none), split не потрібен."""
        data = await setup(
            product_stock=Decimal("0"),
            fiscal_quantity=Decimal("3"),
        )

        result = await data["fiscalizer"].fiscalize_receipt(
            data["receipt"].id, manual=True
        )

        assert result.fiscal_status == "none"
        assert result.warning is not None
        # Чек позначено як повністю нефіскальний
        await data["session"].refresh(data["receipt"], ["items"])
        assert data["receipt"].fiscal_status.value == "none"
        assert data["receipt"].is_fiscal is False
        assert data["receipt"].items[0].fiscal_quantity == 0
