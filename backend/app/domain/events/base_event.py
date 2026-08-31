"""
Domain Event: Base Domain Event.

Базовий клас для всіх доменних подій в Torgashka POS.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime
from uuid import UUID, uuid4


@dataclass(kw_only=True)
class BaseDomainEvent:
    """Базовий клас для всіх доменних подій."""

    event_id: UUID = field(default_factory=uuid4)
    occurred_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    event_name: str = ""

    def __post_init__(self) -> None:
        if not self.event_name:
            self.event_name = self.__class__.__name__
