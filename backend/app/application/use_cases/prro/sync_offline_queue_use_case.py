"""
Application Layer: SyncOfflineQueueUseCase — повторна передача офлайн-документів.

Проходить по prro_queue (status=pending/failed), надсилає документи
по порядку (з урахуванням локальних номерів) та оновлює статуси.
"""

from __future__ import annotations

import logging

from sqlalchemy.ext.asyncio import AsyncSession

from app.application.use_cases.prro.context import PrroContextFactory
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.offline_queue import PrroOfflineQueue

logger = logging.getLogger(__name__)


class SyncOfflineQueueUseCase:
    """
    Use Case для синхронізації офлайн-черги ПРРО.

    Args:
        session: асинхронна сесія БД.
        prro_repo: репозиторій змін/черги ПРРО.
        settings_repo: репозиторій налаштувань ПРРО.
        context_factory: фабрика компонентів ПРРО.
        offline_queue: черга офлайн-документів ПРРО.
    """

    def __init__(
        self,
        session: AsyncSession,
        prro_repo: PrroRepository,
        settings_repo: PrroSettingsRepository,
        context_factory: PrroContextFactory,
        offline_queue: PrroOfflineQueue,
    ) -> None:
        self._session = session
        self._prro_repo = prro_repo
        self._settings_repo = settings_repo
        self._context = context_factory
        self._offline_queue = offline_queue

    async def sync(self, limit: int = 100) -> dict:
        """
        Надсилає всі документи черги (pending/failed) по порядку.

        Args:
            limit: максимальна кількість документів за одну синхронізацію.

        Returns:
            dict: {"synced", "failed", "skipped", "total", "results"}.
        """
        pending = await self._offline_queue.get_pending(limit=limit)
        if not pending:
            return {"synced": 0, "failed": 0, "skipped": 0, "total": 0, "results": []}

        try:
            xml_builder = await self._context.build_xml_builder()
            crypto = await self._context.build_crypto_signer()
            grpc_client = await self._context.grpc_client()
        except Exception as exc:
            # Позначаємо ВСІ pending як failed: sync не має падати 500,
            # коли ключ КЕП/сервер ПРРО недоступний (контракт: sync → 200 + failed).
            logger.warning("PRRO_SYNC | компоненти ПРРО недоступні: %s", exc)
            error = str(exc)
            for item in pending:
                await self._offline_queue.mark_failed(item.id, error)
            return {
                "synced": 0,
                "failed": len(pending),
                "skipped": 0,
                "total": len(pending),
                "results": [
                    {
                        "id": str(item.id),
                        "local_number": int(item.local_number),
                        "check_type": item.check_type,
                        "status": "failed",
                        "error": error,
                    }
                    for item in pending
                ],
            }

        synced = 0
        failed = 0
        results: list[dict] = []

        for item in pending:
            try:
                # B2: відправляємо ПОВНИЙ підписаний check_sign as-is (ідемпотентність).
                # Документи, додані до B2 (check_sign=None), формуються рівно 1 раз
                # і фіксуються у черзі — повторні sync не переформовують
                # (build_message ≤ 1 разу на документ, NT/MAC не змінюються).
                if getattr(item, "check_sign", None):
                    signed = item.check_sign.encode("utf-8")
                else:
                    message = xml_builder.build_message(item.xml_body)
                    signed = crypto.sign(message.encode("utf-8"))
                    await self._offline_queue.update_check_sign(
                        item.id, signed.decode("utf-8")
                    )
                check = await self._context.build_check(
                    check_sign=signed,
                    local_number=int(item.local_number),
                    check_type=item.check_type,
                )
                response = await grpc_client.send_chk(check)

                if int(response.status) == 1:
                    await self._offline_queue.mark_sent(item.id)
                    # B1: оновлюємо last_mac зміни — наступний Check посилатиметься
                    # на хеш цього успішно відправленого документа (hash-ланцюжок).
                    # getattr: документи без shift_id/mac (тестові стаби) — пропускаємо.
                    shift_id = getattr(item, "shift_id", None)
                    mac = getattr(item, "mac", None)
                    if shift_id is not None and mac is not None:
                        await self._prro_repo.update_shift_last_mac(shift_id, mac)
                    synced += 1
                    results.append({
                        "id": str(item.id),
                        "local_number": int(item.local_number),
                        "check_type": item.check_type,
                        "status": "sent",
                    })
                else:
                    error = response.error_message or f"status={response.status}"
                    await self._offline_queue.mark_failed(item.id, error)
                    failed += 1
                    results.append({
                        "id": str(item.id),
                        "local_number": int(item.local_number),
                        "check_type": item.check_type,
                        "status": "failed",
                        "error": error,
                    })
            except Exception as exc:
                logger.warning(
                    "PRRO_SYNC | документ %s не передано: %s", item.id, exc
                )
                await self._offline_queue.mark_failed(item.id, str(exc))
                failed += 1
                results.append({
                    "id": str(item.id),
                    "local_number": int(item.local_number),
                    "check_type": item.check_type,
                    "status": "failed",
                    "error": str(exc),
                })

        await self._context.persist_builder_counters(xml_builder)
        await self._session.commit()

        logger.info(
            "PRRO_SYNC | синхронізовано: %d успішно, %d помилок із %d",
            synced, failed, len(pending),
        )
        return {
            "synced": synced,
            "failed": failed,
            "skipped": 0,
            "total": len(pending),
            "results": results,
        }


__all__ = ["SyncOfflineQueueUseCase"]
