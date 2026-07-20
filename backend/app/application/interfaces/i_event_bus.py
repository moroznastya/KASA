"""
Application Interface: IEventBus (Protocol).

Визначає контракт для Event Bus в Application Layer.
Реалізація знаходиться в Infrastructure Layer.
"""

from __future__ import annotations

from typing import Protocol

from app.domain.events.base_event import DomainEvent


class IEventBus(Protocol):
    """
    Інтерфейс Event Bus.

    Відповідає за публікацію доменних подій та виклик відповідних обробників.
    Використовується Use Cases для публікації подій після виконання команд.
    """

    async def publish(self, event: DomainEvent) -> None:
        """
        Публікує доменну подію.

    Args:
            event: Доменна подія для публікації.

        Всі зареєстровані обробники для цього типу події будуть викликані.
        """
        ...

    async def publish_many(self, events: list[DomainEvent]) -> None:
        """
        Публікує декілька доменних подій одночасно.

        Args:
            events: Список доменних подій для публікації.
        """
        ...

    def subscribe(self, event_type: type[DomainEvent], handler) -> None:
        """
        Підписує обробник на тип події.

        Args:
            event_type: Тип події (клас, успадкований від DomainEvent).
            handler: Асинхронний обробник події.
        """
        ...

    def unsubscribe(self, event_type: type[DomainEvent], handler) -> None:
        """
        Відписує обробник від типу події.

        Args:
            event_type: Тип події.
            handler: Обробник для видалення.
        """
        ...
