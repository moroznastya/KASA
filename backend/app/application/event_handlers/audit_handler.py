"""Event Handler: Аудит доменних подій."""

from __future__ import annotations

import logging
from datetime import datetime
from typing import Any

from app.domain.events import (
    BaseDomainEvent,
)

logger = logging.getLogger(__name__)


class AuditHandler:
    """Аудит — запис всіх важливих подій для історії."""

    def __init__(self, audit_repository: Any = None):
        self._repo = audit_repository

    async def handle(self, event: BaseDomainEvent) -> None:
        """Записати подію в аудит-журнал."""

        audit_entry = {
            "event_id": str(event.event_id),
            "event_name": event.event_name,
            "occurred_at": event.occurred_at.isoformat(),
            "data": self._serialize_event(event),
        }

        if self._repo:
            await self._repo.save(audit_entry)

        logger.debug(f"📋 Аудит: {event.event_name}")

    def _serialize_event(self, event: BaseDomainEvent) -> dict:
        """Серіалізувати подію в словник."""
        data = {}
        for field_name in event.__dataclass_fields__:
            if field_name in ("event_id", "occurred_at", "event_name"):
                continue
            value = getattr(event, field_name)
            if isinstance(value, datetime):
                data[field_name] = value.isoformat()
            else:
                data[field_name] = str(value) if not isinstance(value, (str, int, float, bool, type(None))) else value
        return data
