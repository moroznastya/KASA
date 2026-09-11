"""Unit tests: PRRO API v2 (mock use cases)."""

from __future__ import annotations

from datetime import datetime
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest
from httpx import AsyncClient

from app.api.v2.deps import get_prro_use_cases
from app.application.dto.prro_dto import (
    FiscalizeResponseDTO,
    PrroSettingsDTO,
    PrroShiftDTO,
    PrroStatusDTO,
)
from app.application.use_cases.prro import (
    PrroFiscalizeError,
    PrroShiftError,
)


@pytest.fixture
def prro_mock():
    """Мок фасаду PrroUseCases."""
    m = MagicMock()
    m.get_settings = AsyncMock(
        return_value=PrroSettingsDTO(
            key_file="/secure/Key-6.pfx",
            key_password_masked="••••",
            key_format="pfx",
            prro_fn="4538765845",
            prro_tn="345612052809",
            prro_zn="АА57506761",
            mode="test",
            url="cabinet.tax.gov.ua:9443",
            shift_open=True,
            online=True,
        )
    )
    m.save_settings = AsyncMock(
        return_value=PrroSettingsDTO(
            key_file="/certs/prro-test/Key-6.pfx",
            key_password_masked="••••",
            key_format="pfx",
            prro_fn="4538765845",
            prro_tn="345612052809",
            prro_zn="АА57506761",
            mode="test",
            url="cabinet.tax.gov.ua:9443",
            shift_open=False,
            online=False,
        )
    )
    m.test_connection = AsyncMock(
        return_value={"status": 1, "ok": True, "error": None}
    )
    m.get_status = AsyncMock(
        return_value=PrroStatusDTO(
            open_shift=True,
            online=True,
            last_signer="3F2A9C01",
            name="Тест ПРРО",
            addr="м. Київ, вул. Тестова 1",
            fn="4538765845",
        )
    )
    m.open_shift = AsyncMock(
        return_value=PrroShiftDTO(
            id=uuid4(),
            shift_number=1,
            opened_at=datetime.utcnow(),
            status="open",
            signer_name="Підписант",
            receipt_count=0,
            total_amount=0,
        )
    )
    m.close_shift = AsyncMock(
        return_value=PrroShiftDTO(
            id=uuid4(),
            shift_number=1,
            opened_at=datetime.utcnow(),
            closed_at=datetime.utcnow(),
            status="closed",
            signer_name="Підписант",
            receipt_count=5,
            total_amount=1500.00,
            zreport_number="Z-001",
        )
    )
    m.list_shifts = AsyncMock(
        return_value=(
            [
                PrroShiftDTO(
                    id=uuid4(),
                    shift_number=1,
                    opened_at=datetime.utcnow(),
                    status="closed",
                    receipt_count=5,
                    total_amount=1500.00,
                    zreport_number="Z-001",
                )
            ],
            1,
        )
    )
    m.fiscalize_receipt = AsyncMock(
        return_value=FiscalizeResponseDTO(
            receipt_id=uuid4(),
            fiscal_status="sent",
            fiscal_number="FISCAL-100",
            fiscal_serial="id-sign-100",
            fiscal_sent_at=datetime.utcnow(),
        )
    )
    m.sync_offline_queue = AsyncMock(
        return_value={"synced": 2, "failed": 0, "skipped": 0, "total": 2, "results": []}
    )
    m.get_queue = AsyncMock(
        return_value={"items": [], "total": 0, "pending": 0, "page": 1, "size": 20}
    )
    return m


@pytest.fixture
def prro_api_client(client: AsyncClient, prro_mock, auth_headers: dict):
    """AsyncClient з підміненою залежністю get_prro_use_cases + auth."""

    async def _override():
        return prro_mock

    from app.main import app
    app.dependency_overrides[get_prro_use_cases] = _override
    return client, auth_headers


# ─── Налаштування ───────────────────────────────────────────────────────────

class TestSettingsApi:
    async def test_get_settings(self, prro_api_client, prro_mock):
        """GET /api/v2/prro/settings → PrroSettingsDTO."""
        client, headers = prro_api_client
        resp = await client.get("/api/v2/prro/settings", headers=headers)
        assert resp.status_code == 200
        body = resp.json()
        assert body["prro_fn"] == "4538765845"
        assert body["mode"] == "test"
        assert body["key_password_masked"] == "••••"
        assert "key_password" not in body  # пароль НЕ повертається

    async def test_put_settings(self, prro_api_client, prro_mock):
        """PUT /api/v2/prro/settings (multipart form-data)."""
        client, headers = prro_api_client
        resp = await client.put(
            "/api/v2/prro/settings",
            files={"key_file": ("Key-6.pfx", b"pfx-binary-content")},
            data={
                "key_password": "secret",
                "prro_fn": "4538765845",
                "mode": "test",
                "auto_fiscalize": "true",
            },
            headers=headers,
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["key_format"] == "pfx"
        assert "secret" not in str(body)  # пароль не у відповіді

        # Перевіряємо, що save_settings викликано з вмістом файлу
        prro_mock.save_settings.assert_awaited_once()
        kwargs = prro_mock.save_settings.call_args.kwargs
        assert kwargs["key_file_content"] == b"pfx-binary-content"
        assert kwargs["key_file_name"] == "Key-6.pfx"
        assert kwargs["key_password"] == "secret"
        assert kwargs["auto_fiscalize"] is True

    async def test_test_connection(self, prro_api_client):
        """POST /api/v2/prro/test-connection → ping."""
        client, headers = prro_api_client
        resp = await client.post("/api/v2/prro/test-connection", headers=headers)
        assert resp.status_code == 200
        assert resp.json() == {"status": 1, "ok": True, "error": None}


# ─── Статус ─────────────────────────────────────────────────────────────────

class TestStatusApi:
    async def test_get_status(self, prro_api_client):
        """GET /api/v2/prro/status → PrroStatusDTO."""
        client, headers = prro_api_client
        resp = await client.get("/api/v2/prro/status", headers=headers)
        assert resp.status_code == 200
        body = resp.json()
        assert body["open_shift"] is True
        assert body["online"] is True
        assert body["fn"] == "4538765845"


# ─── Зміни ──────────────────────────────────────────────────────────────────

class TestShiftApi:
    async def test_open_shift(self, prro_api_client):
        """POST /api/v2/prro/shift/open."""
        client, headers = prro_api_client
        resp = await client.post(
            "/api/v2/prro/shift/open", json={"comment": "Касир №1"}, headers=headers
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["shift_number"] == 1
        assert body["status"] == "open"

    async def test_close_shift(self, prro_api_client):
        """POST /api/v2/prro/shift/close."""
        client, headers = prro_api_client
        resp = await client.post(
            "/api/v2/prro/shift/close", json={"comment": "Касир №1"}, headers=headers
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["status"] == "closed"
        assert body["zreport_number"] == "Z-001"

    async def test_open_shift_error_400(self, prro_api_client, prro_mock):
        """Помилка shift use case → HTTP 400."""
        client, headers = prro_api_client
        prro_mock.open_shift.side_effect = PrroShiftError(
            "Зміна вже відкрита", code="SHIFT_ALREADY_OPEN"
        )
        resp = await client.post("/api/v2/prro/shift/open", headers=headers)
        assert resp.status_code == 400
        assert "Зміна вже відкрита" in resp.json()["detail"]

    async def test_list_shifts(self, prro_api_client):
        """GET /api/v2/prro/shifts."""
        client, headers = prro_api_client
        resp = await client.get("/api/v2/prro/shifts?page=1&size=10", headers=headers)
        assert resp.status_code == 200
        body = resp.json()
        assert body["total"] == 1
        assert body["items"][0]["zreport_number"] == "Z-001"


# ─── Фіскалізація ───────────────────────────────────────────────────────────

class TestFiscalizeApi:
    async def test_fiscalize_receipt(self, prro_api_client, prro_mock):
        """POST /api/v2/prro/receipts/{id}/fiscalize."""
        client, headers = prro_api_client
        receipt_id = uuid4()
        resp = await client.post(
            f"/api/v2/prro/receipts/{receipt_id}/fiscalize",
            json={"manual": True},
            headers=headers,
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["fiscal_status"] == "sent"
        assert body["fiscal_number"] == "FISCAL-100"

        prro_mock.fiscalize_receipt.assert_awaited_once()
        assert prro_mock.fiscalize_receipt.call_args.args[0] == receipt_id

    async def test_fiscalize_receipt_error_400(self, prro_api_client, prro_mock):
        """Помилка фіскалізації → HTTP 400."""
        client, headers = prro_api_client
        prro_mock.fiscalize_receipt.side_effect = PrroFiscalizeError(
            "Зміна ПРРО не відкрита", code="ERROR_NOT_OPEN_SHIFT"
        )
        resp = await client.post(
            f"/api/v2/prro/receipts/{uuid4()}/fiscalize", headers=headers
        )
        assert resp.status_code == 400
        assert "не відкрита" in resp.json()["detail"]


# ─── Синхронізація та черга ─────────────────────────────────────────────────

class TestSyncAndQueueApi:
    async def test_sync(self, prro_api_client):
        """POST /api/v2/prro/sync."""
        client, headers = prro_api_client
        resp = await client.post("/api/v2/prro/sync?limit=50", headers=headers)
        assert resp.status_code == 200
        body = resp.json()
        assert body["synced"] == 2
        assert body["total"] == 2

    async def test_get_queue(self, prro_api_client):
        """GET /api/v2/prro/queue."""
        client, headers = prro_api_client
        resp = await client.get("/api/v2/prro/queue?status=pending", headers=headers)
        assert resp.status_code == 200
        body = resp.json()
        assert body["total"] == 0
        assert body["pending"] == 0


# ─── RBAC: захист чутливих операцій (Фаза 4, аудит безпеки) ──────────────────

class TestPrroRbac:
    """Касир не має права змінювати налаштування / закривати зміну / синхронізувати."""

    async def test_cashier_forbidden_put_settings(self, client, prro_mock, cashier_headers):
        from app.api.v2.deps import get_prro_use_cases
        from app.main import app

        async def _override():
            return prro_mock
        app.dependency_overrides[get_prro_use_cases] = _override
        try:
            resp = await client.put(
                "/api/v2/prro/settings",
                data={"prro_fn": "4538765845"},
                headers=cashier_headers,
            )
            assert resp.status_code == 403, resp.text
        finally:
            app.dependency_overrides.pop(get_prro_use_cases, None)

    async def test_cashier_forbidden_close_shift(self, client, prro_mock, cashier_headers):
        from app.api.v2.deps import get_prro_use_cases
        from app.main import app

        async def _override():
            return prro_mock
        app.dependency_overrides[get_prro_use_cases] = _override
        try:
            resp = await client.post(
                "/api/v2/prro/shift/close",
                json={},
                headers=cashier_headers,
            )
            assert resp.status_code == 403, resp.text
        finally:
            app.dependency_overrides.pop(get_prro_use_cases, None)

    async def test_cashier_allowed_get_status(self, client, prro_mock, cashier_headers):
        from app.api.v2.deps import get_prro_use_cases
        from app.main import app

        async def _override():
            return prro_mock
        app.dependency_overrides[get_prro_use_cases] = _override
        try:
            resp = await client.get("/api/v2/prro/status", headers=cashier_headers)
            assert resp.status_code == 200, resp.text
        finally:
            app.dependency_overrides.pop(get_prro_use_cases, None)
