"""
Unit tests: режим заглушки (stub) фіскалізації ПРРО.

Перевіряємо:
  - визначення активності stub (налаштування prro_stub_mode / env PRRO_STUB);
  - ручну фіскалізацію через stub (успіх, fiscalized, STUB-номер);
  - авто-фіскалізацію через stub (при auto_fiscalize=true);
  - що при вимкненій авто-фіскалізації чек НЕ фіскалізується автоматично.
"""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.dto.prro_dto import FiscalizeResponseDTO
from app.application.use_cases.prro.context import (
    KEY_AUTO_FISCALIZE,
    KEY_PRRO_STUB_MODE,
)
from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    FiscalizeReceiptUseCase,
)
from app.infrastructure.persistence.models.receipt import FiscalStatus


def _build_use_case(*, stub_value: str | None, auto_value: str = "true"):
    """FiscalizeReceiptUseCase з моками (без реальної БД і ПРРО)."""
    session = AsyncMock()
    result = MagicMock()

    receipt = MagicMock()
    receipt.id = uuid4()
    receipt.receipt_number = "RCPT-1"
    receipt.fiscal_status = FiscalStatus.PENDING
    receipt.is_fiscal = False
    result.scalar_one_or_none.return_value = receipt

    session.execute = AsyncMock(return_value=result)
    session.commit = AsyncMock()

    settings_repo = AsyncMock()
    settings_repo.get = AsyncMock(
        side_effect=lambda key: {
            KEY_PRRO_STUB_MODE: stub_value,
            KEY_AUTO_FISCALIZE: auto_value,
        }.get(key)
    )

    use_case = FiscalizeReceiptUseCase(
        session=session,
        prro_repo=AsyncMock(),
        settings_repo=settings_repo,
        context_factory=MagicMock(),
        offline_queue=AsyncMock(),
    )
    return use_case, session, receipt


class TestStubModeDetection:
    """Визначення активності режиму заглушки."""

    async def test_enabled_from_settings_true(self):
        use_case, _, _ = _build_use_case(stub_value="true")
        assert await use_case._stub_mode_enabled() is True

    async def test_enabled_from_settings_one(self):
        use_case, _, _ = _build_use_case(stub_value="1")
        assert await use_case._stub_mode_enabled() is True

    async def test_enabled_from_env(self, monkeypatch):
        monkeypatch.setenv("PRRO_STUB", "true")
        use_case, _, _ = _build_use_case(stub_value=None)
        assert await use_case._stub_mode_enabled() is True

    async def test_disabled(self, monkeypatch):
        monkeypatch.delenv("PRRO_STUB", raising=False)
        use_case, _, _ = _build_use_case(stub_value="false")
        assert await use_case._stub_mode_enabled() is False

    async def test_disabled_when_no_sources(self, monkeypatch):
        monkeypatch.delenv("PRRO_STUB", raising=False)
        use_case, _, _ = _build_use_case(stub_value=None)
        assert await use_case._stub_mode_enabled() is False


class TestFiscalizeStub:
    """Ручна фіскалізація через заглушку."""

    async def test_manual_fiscalize_success(self):
        use_case, session, receipt = _build_use_case(
            stub_value="true", auto_value="false"
        )
        dto = await use_case.fiscalize_receipt(receipt.id, manual=True)

        assert isinstance(dto, FiscalizeResponseDTO)
        assert dto.status == "success"
        assert dto.fiscal_status == "fiscalized"
        assert dto.fiscal_number.startswith(f"STUB-{receipt.receipt_number}-")
        assert dto.fiscal_serial == "STUB"
        assert dto.message == "Фіскалізацію виконано (заглушка)"
        assert dto.fiscal_date is not None

        # Чек позначено фіскалізованим у БД
        assert receipt.fiscal_status == FiscalStatus.FISCALIZED
        assert receipt.is_fiscal is True
        assert receipt.fiscal_error is None
        session.commit.assert_awaited_once()

    async def test_auto_fiscalize_stub_success(self):
        """Авто-фіскалізація через stub при auto_fiscalize=true."""
        use_case, session, receipt = _build_use_case(
            stub_value="true", auto_value="true"
        )
        dto = await use_case.fiscalize_receipt(receipt.id, manual=False)

        assert dto.status == "success"
        assert dto.fiscal_status == "fiscalized"
        assert receipt.fiscal_status == FiscalStatus.FISCALIZED
        session.commit.assert_awaited_once()

    async def test_auto_fiscalize_skipped_when_disabled(self):
        """Авто-фіскалізація вимкнена → чек не фіскалізується (навіть у stub)."""
        use_case, session, receipt = _build_use_case(
            stub_value="true", auto_value="false"
        )
        dto = await use_case.fiscalize_receipt(receipt.id, manual=False)

        assert dto.status == "success"
        assert dto.fiscal_status == "none"
        assert dto.error == "Авто-фіскалізація вимкнена (auto_fiscalize=false)"
        assert receipt.fiscal_status != FiscalStatus.FISCALIZED
        session.commit.assert_not_awaited()

    async def test_manual_fiscalize_without_stub_uses_real_path(self):
        """Без stub ручний виклик іде реальним шляхом (stub-фіскалізація НЕ викликається)."""
        use_case, _, receipt = _build_use_case(
            stub_value="false", auto_value="false"
        )
        use_case._fiscalize_stub = AsyncMock(
            return_value=FiscalizeResponseDTO(
                receipt_id=receipt.id, fiscal_status="fiscalized", status="success"
            )
        )
        dto = await use_case.fiscalize_receipt(receipt.id, manual=True)
        # Реальний шлях не викликає stub (немає фіскальних позицій у mock-чека)
        use_case._fiscalize_stub.assert_not_awaited()
        assert dto.fiscal_status != "fiscalized"
