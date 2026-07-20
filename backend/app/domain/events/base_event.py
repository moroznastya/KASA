"""
Базовий клас для всіх доменних подій.

Всі доменні події мають наслідуватись від DomainEvent.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from uuid import UUID, uuid4


@dataclass(frozen=True)
class DomainEvent:
    """
    Базовий клас доменної події.

    Всі події в системі мають наслідуватись від цього класу.
    Події є immutable (frozen=True) після створення.

    Атрибути:
        event_id: Унікальний ідентифікатор події.
        created_at: Час створення події (UTC).
        aggregate_id: ID агрегату, який згенерував подію.
    """

    event_id: UUID = field(default_factory=uuid4)
    created_at: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    aggregate_id: UUID = field(default_factory=uuid4)

    def __str__(self) -> str:
        return (
            f"{self.__class__.__name__}("
            f"event_id={self.event_id}, "
            f"aggregate_id={self.aggregate_id}, "
            f"created_at={self.created_at.isoformat()})"
        )

    def __repr__(self) -> str:
        return self.__str__()
