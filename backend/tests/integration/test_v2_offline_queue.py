"""
Інтеграційні тести офлайн-черги ПРРО через API v2.

Покриває флоу (HTTP → PrroUseCases → БД):
  - Журнал черги: порожня черга → GET /api/v2/prro/queue → 200, total=0
  - Синхронізація порожньої черги → POST /api/v2/prro/sync → 200, synced=0
  - Додавання документа в чергу (PrroQueueItem) → видно в журналі
  - Синхронізація з помилкою передачі → item позначено failed
  - Синхронізація з успішною передачею (mocked gRPC) → item позначено sent
  - Доступ до sync лише для admin → cashier → 403
"""

from uuid import uuid4

import pytest
from httpx import AsyncClient

from app.infrastructure.persistence.models.prro import PrroQueueItem

pytestmark = [
    pytest.mark.integration,
    pytest.mark.prro,
    pytest.mark.v2,
]


def _make_item(**overrides) -> PrroQueueItem:
    """Створює PrroQueueItem для прямої вставки в БД (як робить PrroOfflineQueue)."""
    from app.infrastructure.persistence.models.prro import PrroQueueStatus

    data = dict(
        id=uuid4(),
        local_number=1,
        check_type="CHK",
        xml_body="<DAT><body><n>1</n></body></DAT>",
        status=PrroQueueStatus.PENDING,
    )
    data.update(overrides)
    return PrroQueueItem(**data)


class TestV2OfflineQueue:
    """Офлайн-черга ПРРО: журнал + синхронізація."""

    async def test_queue_empty(self, client: AsyncClient, auth_headers: dict):
        """Порожня черга → 200, total=0, pending=0."""
        response = await client.get("/api/v2/prro/queue", headers=auth_headers)
        assert response.status_code == 200
        data = response.json()
        assert data["items"] == []
        assert data["total"] == 0
        assert data["pending"] == 0

    async def test_sync_empty_queue(self, client: AsyncClient, auth_headers: dict):
        """Синхронізація порожньої черги → 200, synced=0."""
        response = await client.post("/api/v2/prro/sync", headers=auth_headers)
        assert response.status_code == 200
        data = response.json()
        assert data["synced"] == 0
        assert data["failed"] == 0
        assert data["total"] == 0

    async def test_queue_shows_pending_item(
        self, client: AsyncClient, session, auth_headers: dict
    ):
        """Документ у черзі → видно в журналі зі статусом pending."""
        item = _make_item()
        session.add(item)
        await session.commit()

        response = await client.get("/api/v2/prro/queue", headers=auth_headers)
        assert response.status_code == 200
        data = response.json()
        assert data["total"] == 1
        assert data["pending"] == 1
        assert data["items"][0]["id"] == str(item.id)
        assert data["items"][0]["status"] == "pending"
        assert data["items"][0]["check_type"] == "CHK"

    async def test_sync_marks_item_failed(
        self, client: AsyncClient, session, auth_headers: dict
    ):
        """
        Синхронізація з помилкою формування/передачі → item позначено failed.

        (xml_body без атрибута DI — формування повідомлення падає,
        помилка перехоплюється в use case → 200 з failed=1)
        """
        item = _make_item()  # xml_body без DI → build_message кидає ValueError
        session.add(item)
        await session.commit()

        response = await client.post("/api/v2/prro/sync", headers=auth_headers)
        assert response.status_code == 200
        data = response.json()
        assert data["synced"] == 0
        assert data["failed"] == 1
        assert data["total"] == 1
        assert data["results"][0]["id"] == str(item.id)
        assert data["results"][0]["status"] == "failed"

        # Статус оновлено в БД
        check = await client.get("/api/v2/prro/queue", headers=auth_headers)
        assert check.json()["items"][0]["status"] == "failed"

    async def test_sync_marks_item_sent(
        self, client: AsyncClient, session, auth_headers: dict, monkeypatch
    ):
        """
        Синхронізація з успішною передачею (mocked gRPC/крипто) →
        item позначено sent, synced=1.

        Мокаємо лише зовнішній кордон (gRPC-клієнт ПРРО та КЕП-підписант);
        весь шлях API → SyncOfflineQueueUseCase → PrroOfflineQueue → БД реальний.
        """
        from app.application.use_cases.prro.context import PrroContextFactory

        item = _make_item(
            xml_body='<DAT V="1" DI="1" NT="1"><body><n>1</n></body></DAT>'
        )
        session.add(item)
        await session.commit()

        async def fake_crypto(self):
            # build_crypto_signer() АВАТИТЬСЯ; sign() викликається синхронно
            class FakeCrypto:
                def sign(self, data):
                    return b"fake-signature"

            return FakeCrypto()

        async def fake_grpc(self):
            class FakeResponse:
                status = 1
                error_message = ""

            class FakeClient:
                async def send_chk(self, check):
                    return FakeResponse()

            return FakeClient()

        monkeypatch.setattr(PrroContextFactory, "build_crypto_signer", fake_crypto)
        monkeypatch.setattr(PrroContextFactory, "grpc_client", fake_grpc)

        response = await client.post("/api/v2/prro/sync", headers=auth_headers)
        assert response.status_code == 200
        data = response.json()
        assert data["synced"] == 1
        assert data["failed"] == 0
        assert data["total"] == 1
        assert data["results"][0]["status"] == "sent"

        # Статус оновлено в БД (після sync item виходить з pending-вибірки,
        # тому перевіряємо безпосередньо в БД)
        from sqlalchemy import select

        from app.infrastructure.persistence.models.prro import PrroQueueItem

        result = await session.execute(
            select(PrroQueueItem).where(PrroQueueItem.id == item.id)
        )
        db_item = result.scalar_one()
        assert db_item.status.value == "sent"
        assert db_item.sent_at is not None

    async def test_sync_requires_admin(
        self, client: AsyncClient, cashier_headers: dict
    ):
        """Синхронізація черги доступна лише адміністратору → cashier → 403."""
        response = await client.post("/api/v2/prro/sync", headers=cashier_headers)
        assert response.status_code == 403
