"""Unit tests: PrroShiftUseCase — відкриття/закриття зміни ПРРО."""

from __future__ import annotations

from datetime import datetime, timedelta
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.application.use_cases.prro.context import PrroContextFactory
from app.application.use_cases.prro.shift_use_case import (
    PrroShiftError,
    PrroShiftUseCase,
)
from app.infrastructure.persistence.models.prro import (
    PrroShift,
)
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.key_store import PrroKeyStore
from app.infrastructure.services.prro.offline_queue import PrroOfflineQueue

# ─── Допоміжні фабрики ──────────────────────────────────────────────────────

def make_check_response(*, status: int = 1, id: str = "FISCAL-001", error_message: str = ""):
    """Імітує CheckResponse gRPC."""
    return SimpleNamespace(
        status=status,
        id=id,
        error_message=error_message,
        id_sign=b"id-sign-bytes",
    )


def make_grpc_client(send_response=None):
    """Імітує PrroGrpcClient з AsyncMock-методами."""
    client = MagicMock()
    client.send_chk = AsyncMock(return_value=send_response or make_check_response())
    client.ping = AsyncMock(return_value=make_check_response())
    client.status = AsyncMock(
        return_value=SimpleNamespace(open_shift=False, online=True, last_signer="S1")
    )
    client.info = AsyncMock(
        return_value=SimpleNamespace(name="Тест ПРРО", addr="м. Київ", fn="12345")
    )
    client.last_chk = AsyncMock(return_value=make_check_response())
    return client


def make_crypto():
    """Імітує PrroCryptoSigner."""
    crypto = MagicMock()
    crypto.sign = MagicMock(return_value=b"<signed-doc/>")
    crypto.get_serial_number = MagicMock(return_value="3F2A9C01B7D4E8A2")
    crypto.get_signer_name = MagicMock(return_value="Тестовий Підписант")
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
def build_use_case(session: AsyncSession, key_store):
    """Фабрика PrroShiftUseCase з замоканими gRPC/crypto."""
    prro_repo = PrroRepository(session)
    settings_repo = PrroSettingsRepository(session)
    service_factory = MagicMock()

    context = PrroContextFactory(
        settings_repo=settings_repo,
        key_store=key_store,
        service_factory=service_factory,
    )
    offline_queue = PrroOfflineQueue(prro_repo)

    def _build(grpc_client=None, crypto=None):
        grpc_client = grpc_client or make_grpc_client()
        crypto = crypto or make_crypto()
        context.grpc_client = AsyncMock(return_value=grpc_client)
        context.build_crypto_signer = AsyncMock(return_value=crypto)
        return PrroShiftUseCase(
            session=session,
            prro_repo=prro_repo,
            settings_repo=settings_repo,
            context_factory=context,
            offline_queue=offline_queue,
        ), prro_repo, settings_repo, context

    return _build


async def _create_open_shift(
    prro_repo: PrroRepository,
    *,
    opened_at: datetime | None = None,
    receipt_count: int = 5,
    total_amount=1500.00,
) -> PrroShift:
    """Створює відкриту зміну у БД."""
    shift = PrroShift(
        shift_number=1,
        opened_at=opened_at or datetime.utcnow(),
        status="open",
        receipt_count=receipt_count,
        total_amount=total_amount,
        last_local_number=receipt_count,
        last_mac="mac-prev",
    )
    return await prro_repo.create_shift(shift)


# ─── Тести open_shift ───────────────────────────────────────────────────────

class TestOpenShift:
    async def test_open_shift_success(self, build_use_case):
        """Успішне відкриття зміни: створюється PrroShift + запис у чергу."""
        use_case, prro_repo, settings_repo, _context = build_use_case()

        dto = await use_case.open_shift(comment="Касир №1")

        assert dto.shift_number == 1
        assert dto.status == "open"
        assert dto.signer_name == "Тестовий Підписант"
        assert dto.receipt_count == 0

        # Зміна в БД
        open_shift = await prro_repo.get_open_shift()
        assert open_shift is not None
        assert open_shift.shift_number == 1
        assert open_shift.signer_serial == "3F2A9C01B7D4E8A2"

        # Черга: SERVICECHK #0 успішно передано
        queue_items = await prro_repo.list_by_shift(open_shift.id)
        assert len(queue_items) == 1
        assert queue_items[0].check_type == "SERVICECHK"
        assert queue_items[0].local_number == 0
        assert queue_items[0].status.value == "sent"

        # Лічильник змін збережено
        assert await settings_repo.get("last_shift_number") == "1"

    async def test_open_shift_already_open(self, build_use_case):
        """Помилка SHIFT_ALREADY_OPEN, якщо зміна вже відкрита."""
        use_case, prro_repo, _, _ = build_use_case()
        await _create_open_shift(prro_repo)

        with pytest.raises(PrroShiftError) as exc_info:
            await use_case.open_shift()
        assert exc_info.value.code == "SHIFT_ALREADY_OPEN"

    async def test_open_shift_server_error(self, build_use_case):
        """Помилка OPEN_SHIFT_FAILED при відмові сервера."""
        grpc = make_grpc_client(
            send_response=make_check_response(
                status=-13, error_message="RRO not registered"
            )
        )
        use_case, _, _, _ = build_use_case(grpc_client=grpc)

        with pytest.raises(PrroShiftError) as exc_info:
            await use_case.open_shift()
        assert exc_info.value.code == "OPEN_SHIFT_FAILED"
        # Текст сервера — ПОВНІСТЮ + код/ім'я/опис статусу (не голий status=-13).
        msg = str(exc_info.value)
        assert "RRO not registered" in msg
        assert "ERROR_NOT_REGISTERED_RRO" in msg
        assert "ПРРО не зареєстровано" in msg
        assert "status=-13" in msg

    async def test_open_shift_server_error_without_message_includes_name(self, build_use_case):
        """Без error_message сервера: «status=-13 (ERROR_NOT_REGISTERED_RRO: ...)»."""
        grpc = make_grpc_client(
            send_response=make_check_response(status=-13, error_message="")
        )
        use_case, _, _, _ = build_use_case(grpc_client=grpc)

        with pytest.raises(PrroShiftError) as exc_info:
            await use_case.open_shift()
        msg = str(exc_info.value)
        assert "status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)" in msg


# ─── Тести close_shift ──────────────────────────────────────────────────────

class TestCloseShift:
    async def test_close_shift_success(self, build_use_case):
        """Успішне закриття зміни: Z-звіт, closed_at, zreport_number."""
        use_case, prro_repo, _, _ = build_use_case()
        shift = await _create_open_shift(prro_repo, receipt_count=5, total_amount=1500.00)

        dto = await use_case.close_shift(comment="Касир №1")

        assert dto.status == "closed"
        assert dto.closed_at is not None
        assert dto.zreport_number == "FISCAL-001"
        assert dto.receipt_count == 5

        # Зміна в БД закрита
        closed = await prro_repo.get_shift(shift.id)
        assert closed is not None
        assert closed.status.value == "closed"
        assert closed.zreport_number == "FISCAL-001"

        # Черга: ZREPORT #0 успішно передано
        queue_items = await prro_repo.list_by_shift(shift.id)
        assert len(queue_items) == 1
        assert queue_items[0].check_type == "ZREPORT"
        assert queue_items[0].status.value == "sent"

    async def test_close_shift_no_open(self, build_use_case):
        """Помилка NO_OPEN_SHIFT, якщо немає відкритої зміни."""
        use_case, _, _, _ = build_use_case()

        with pytest.raises(PrroShiftError) as exc_info:
            await use_case.close_shift()
        assert exc_info.value.code == "NO_OPEN_SHIFT"

    async def test_close_shift_server_error(self, build_use_case):
        """Помилка CLOSE_SHIFT_FAILED при відмові сервера."""
        grpc = make_grpc_client(
            send_response=make_check_response(
                status=-10, error_message="Zreport XML invalid"
            )
        )
        use_case, prro_repo, _, _ = build_use_case(grpc_client=grpc)
        await _create_open_shift(prro_repo)

        with pytest.raises(PrroShiftError) as exc_info:
            await use_case.close_shift()
        assert exc_info.value.code == "CLOSE_SHIFT_FAILED"


# ─── Тести нагадувань та журналу ────────────────────────────────────────────

class TestReminderAndList:
    async def test_auto_reminder_check_none(self, build_use_case):
        """Немає відкритої зміни → попередження відсутнє."""
        use_case, _, _, _ = build_use_case()
        assert await use_case.auto_reminder_check() is None

    async def test_auto_reminder_check_under_24h(self, build_use_case):
        """Зміна відкрита < 24 год → без попередження."""
        use_case, prro_repo, _, _ = build_use_case()
        await _create_open_shift(
            prro_repo, opened_at=datetime.utcnow() - timedelta(hours=5)
        )
        assert await use_case.auto_reminder_check() is None

    async def test_auto_reminder_check_over_24h(self, build_use_case):
        """Зміна відкрита > 24 год → попередження."""
        use_case, prro_repo, _, _ = build_use_case()
        await _create_open_shift(
            prro_repo, opened_at=datetime.utcnow() - timedelta(hours=30)
        )
        result = await use_case.auto_reminder_check()
        assert result is not None
        assert result["shift_open"] is True
        assert result["hours_open"] > 24
        assert "warning" in result

    async def test_list_shifts(self, build_use_case):
        """Список змін з пагінацією."""
        use_case, prro_repo, _, _ = build_use_case()
        await _create_open_shift(prro_repo, receipt_count=2)
        await prro_repo.close_shift(
            shift_id=(await prro_repo.get_open_shift()).id,
            closed_at=datetime.utcnow(),
            closed_by="admin",
            zreport_number="Z-001",
        )

        shifts, total = await use_case.list_shifts(page=1, size=10)
        assert total == 1
        assert shifts[0].status == "closed"
        assert shifts[0].zreport_number == "Z-001"


# ─── Тести підсумків Z-звіту (2.5) ───────────────────────────────────────────

class TestZreportTotals:
    async def test_zreport_computes_totals_from_sent_checks(
        self, build_use_case
    ):
        """Z-звіт будується на основі переданих чеків (обіг, ПДВ, оплати)."""
        use_case, prro_repo, _, _ = build_use_case()
        shift = await _create_open_shift(prro_repo, receipt_count=0, total_amount=0)

        # Будуємо XML двох чеків (продаж 300 грн + повернення 100 грн)
        builder = __import__(
            "app.infrastructure.services.prro.xml_builder",
            fromlist=["XmlBuilder"],
        ).XmlBuilder("4538765845", "ПН 345612052809", "АА57506761")

        sale_xml = builder.build_receipt_xml(
            check_type="0",
            items=[{"code": "1", "name": "Товар", "quantity": 3,
                    "price": 100, "total": 300, "tax_rate": "0"}],
            payments=[{"code": "0", "name": "ГОТІВКА", "amount": 300}],
            totals={"total": 300, "fiscal_number": 1, "se": 250,
                    "tax_groups": [{"tax": "0", "tax_percent": 20,
                                    "tax_total": 50, "dtpr": 0, "dtsm": 0,
                                    "tax_type": "0", "tax_algorithm": "0"}]},
        )
        return_xml = builder.build_receipt_xml(
            check_type="1",
            items=[{"code": "2", "name": "Повернення", "quantity": 1,
                    "price": 100, "total": 100, "tax_rate": "0"}],
            payments=[{"code": "0", "name": "ГОТІВКА", "amount": 100}],
            totals={"total": 100, "fiscal_number": 2, "se": 83.33,
                    "tax_groups": [{"tax": "0", "tax_percent": 20,
                                    "tax_total": 16.67, "dtpr": 0, "dtsm": 0,
                                    "tax_type": "0", "tax_algorithm": "0"}]},
        )

        # Додаємо у чергу як успішно передані (sent)
        from app.infrastructure.services.prro.offline_queue import (
            CHECK_TYPE_CHK,
            PrroOfflineQueue,
        )

        offline_queue = PrroOfflineQueue(prro_repo)
        for i, xml in enumerate([sale_xml, return_xml], start=1):
            item = await offline_queue.add_document(
                receipt_id=None, shift_id=shift.id, local_number=i,
                check_type=CHECK_TYPE_CHK, xml_body=xml,
            )
            await offline_queue.mark_sent(item.id)

        # Закриваємо зміну
        dto = await use_case.close_shift(comment="Касир")
        assert dto.status == "closed"

        # Перевіряємо XML Z-звіту у черзі
        queue_items = await prro_repo.list_by_shift(shift.id)
        z_items = [i for i in queue_items if i.check_type == "ZREPORT"]
        assert len(z_items) == 1
        z_xml = z_items[0].xml_body

        # NC: 1 чек продажу, 1 повернення
        assert '<NC NI="1" NO="1"></NC>' in z_xml
        # Обіг за готівкою: 300 отримано, 100 видано
        assert 'SMI="30000"' in z_xml
        assert 'SMO="10000"' in z_xml
        # ПДВ: TXI=50.00 (300/1.2*0.2), TXO=16.67 (100/1.2*0.2)
        assert 'TXI="5000"' in z_xml
        assert 'TXO="1667"' in z_xml

    async def test_zreport_fallback_to_shift_counters(self, build_use_case):
        """Без чеків у черзі — використовуються лічильники зміни (fallback)."""
        use_case, prro_repo, _, _ = build_use_case()
        await _create_open_shift(prro_repo, receipt_count=3, total_amount=900.00)

        dto = await use_case.close_shift()

        assert dto.status == "closed"
        assert dto.receipt_count == 3
        assert float(dto.total_amount) == 900.00
