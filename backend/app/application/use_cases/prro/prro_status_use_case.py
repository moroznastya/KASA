"""
Application Layer: PrroStatusUseCase — статус ПРРО (statusRro/infoRro).

Комбінує дані фіскального сервера (statusRro/infoRro через gRPC)
з локальним станом (відкрита зміна, офлайн-черга).
"""

from __future__ import annotations

import logging
from typing import Optional

from app.application.dto.prro_dto import PrroQueueItemDTO, PrroStatusDTO
from app.application.use_cases.prro.context import PrroContextFactory
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)

logger = logging.getLogger(__name__)


class PrroStatusUseCase:
    """
    Use Case для отримання статусу ПРРО.

    Args:
        prro_repo: репозиторій змін/черги ПРРО.
        settings_repo: репозиторій налаштувань ПРРО.
        context_factory: фабрика компонентів ПРРО.
    """

    def __init__(
        self,
        prro_repo: PrroRepository,
        settings_repo: PrroSettingsRepository,
        context_factory: PrroContextFactory,
    ) -> None:
        self._prro_repo = prro_repo
        self._settings_repo = settings_repo
        self._context = context_factory

    # ─── Статус ────────────────────────────────────────────────────────────

    async def get_status(self) -> PrroStatusDTO:
        """
        Повертає статус ПРРО (gRPC + локальний стан).

        При недоступності фіскального сервера повертається локальний стан
        (online=False, open_shift з БД).

        Returns:
            PrroStatusDTO.
        """
        open_shift = await self._prro_repo.get_open_shift()
        local_open = open_shift is not None
        fn = await self._settings_repo.get("prro_fn") or None

        dto = PrroStatusDTO(
            open_shift=local_open,
            online=False,
            last_signer=None,
            name=None,
            addr=None,
            fn=fn,
        )

        # Дані з фіскального сервера (best-effort)
        try:
            client = await self._context.grpc_client()

            status_resp = await client.status(timeout=5)
            dto.open_shift = bool(getattr(status_resp, "open_shift", local_open))
            dto.online = bool(getattr(status_resp, "online", False))
            dto.last_signer = getattr(status_resp, "last_signer", None) or None

            info_resp = await client.info(timeout=5)
            dto.name = getattr(info_resp, "name", None) or None
            dto.addr = getattr(info_resp, "addr", None) or None
            dto.fn = getattr(info_resp, "fn", None) or dto.fn
        except Exception as exc:
            logger.warning("PRRO_STATUS | сервер недоступний: %s", exc)

        return dto

    # ─── Черга ─────────────────────────────────────────────────────────────

    async def get_queue(
        self,
        page: int = 1,
        size: int = 20,
        status_filter: Optional[str] = None,
    ) -> dict:
        """
        Повертає журнал офлайн-черги ПРРО.

        Args:
            page: номер сторінки.
            size: кількість на сторінці.
            status_filter: фільтр за статусом (pending/sent/failed).

        Returns:
            dict: {"items": [...], "total": int, "pending": int}.
        """
        # Простий журнал: використовуємо list_pending + count_pending.
        # Повноцінний пошук з фільтрами — наступні фази.
        items = await self._prro_repo.list_pending(limit=size)
        pending_count = await self._prro_repo.count_pending()

        if status_filter:
            items = [i for i in items if i.status.value == status_filter]

        start = (page - 1) * size
        page_items = items[start : start + size] if start < len(items) else []

        return {
            "items": [self._queue_to_dto(i) for i in page_items],
            "total": len(items),
            "pending": pending_count,
            "page": page,
            "size": size,
        }

    @staticmethod
    def _queue_to_dto(item) -> PrroQueueItemDTO:
        """Конвертує PrroQueueItem у DTO."""
        return PrroQueueItemDTO(
            id=item.id,
            receipt_id=item.receipt_id,
            shift_id=item.shift_id,
            local_number=item.local_number,
            check_type=item.check_type,
            status=item.status.value
            if hasattr(item.status, "value") else str(item.status),
            error=item.error,
            created_at=item.created_at,
            sent_at=item.sent_at,
        )


__all__ = ["PrroStatusUseCase"]
