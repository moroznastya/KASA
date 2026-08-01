"""
Infrastructure Layer: LocalEventBus — реалізація IEventBus.

In-memory Event Bus для синхронної публікації доменних подій.
Використовує словник підписок {event_type: [handlers]}.
"""

from __future__ import annotations

import asyncio
import logging
from collections import defaultdict
from typing import Any, Callable, Coroutine

from app.domain.events.base_event import BaseDomainEvent
from app.application.interfaces.i_event_bus import IEventBus

logger = logging.getLogger(__name__)

# Тип для обробника події
EventHandler = Callable[[BaseDomainEvent], Coroutine[Any, Any, None]]


class LocalEventBus(IEventBus):
    """
    Локальний in-memory Event Bus.

    Реалізує IEventBus для синхронної публікації подій.
    Всі підписники викликаються послідовно в межах одного потоку.

    Підтримує:
    - Підписку/відписку обробників
    - Публікацію однієї події
    - Публікацію списку подій
    - Історію подій для аудиту
    """

    def __init__(self) -> None:
        """Ініціалізує Event Bus з порожнім словником підписок."""
        self._handlers: dict[type[BaseDomainEvent], list[EventHandler]] = defaultdict(list)
        self._history: list[BaseDomainEvent] = []

    # ─── Публікація ─────────────────────────────────────────────────────────

    async def publish(self, event: BaseDomainEvent) -> None:
        """
        Публікує доменну подію — викликає всіх підписаних обробників.

        Args:
            event: Доменна подія для публікації.
        """
        event_type = type(event)

        # Шукаємо хендлери за MRO: підписка на BaseDomainEvent покриває всі події.
        handlers: list[EventHandler] = []
        for cls in event_type.__mro__:
            handlers.extend(self._handlers.get(cls, []))
        # Дедуплікація (хендлер може бути підписаний і на батьківський тип)
        seen: set[int] = set()
        unique: list[EventHandler] = []
        for h in handlers:
            if id(h) not in seen:
                seen.add(id(h))
                unique.append(h)
        handlers = unique

        if not handlers:
            logger.debug(f"Event {event_type.__name__} published, no handlers registered")
            self._history.append(event)
            return

        logger.info(
            f"Publishing event {event_type.__name__} "
            f"(id={event.event_id}) to {len(handlers)} handler(s)"
        )

        for handler in handlers:
            try:
                await handler(event)
            except Exception as e:
                logger.error(
                    f"Handler {handler.__name__} failed for event "
                    f"{event_type.__name__}: {e}",
                    exc_info=True,
                )

        self._history.append(event)

    async def publish_many(self, events: list[BaseDomainEvent]) -> None:
        """
        Публікує декілька доменних подій одночасно.

        Args:
            events: Список доменних подій для публікації.
        """
        for event in events:
            await self.publish(event)

    # ─── Підписка ───────────────────────────────────────────────────────────

    def subscribe(
        self,
        event_type: type[BaseDomainEvent],
        handler: EventHandler,
    ) -> None:
        """
        Підписує обробник на тип події.

        Args:
            event_type: Тип події (клас, успадкований від BaseDomainEvent).
            handler: Асинхронний обробник події.
        """
        if handler not in self._handlers[event_type]:
            self._handlers[event_type].append(handler)
            logger.info(
                f"Handler {handler.__name__} subscribed to {event_type.__name__}"
            )

    def unsubscribe(
        self,
        event_type: type[BaseDomainEvent],
        handler: EventHandler,
    ) -> None:
        """
        Відписує обробник від типу події.

        Args:
            event_type: Тип події.
            handler: Обробник для видалення.
        """
        if event_type in self._handlers:
            self._handlers[event_type].remove(handler)
            logger.info(
                f"Handler {handler.__name__} unsubscribed from {event_type.__name__}"
            )
            if not self._handlers[event_type]:
                del self._handlers[event_type]

    # ─── Допоміжні методи ───────────────────────────────────────────────────

    def get_history(
        self,
        event_type: type[BaseDomainEvent] | None = None,
    ) -> list[BaseDomainEvent]:
        """
        Повертає історію подій (для аудиту).

        Args:
            event_type: Фільтр за типом події (опціонально).

        Returns:
            Список подій.
        """
        if event_type is not None:
            return [e for e in self._history if isinstance(e, event_type)]
        return list(self._history)

    def clear_history(self) -> None:
        """Очищає історію подій."""
        self._history.clear()

    @property
    def handler_count(self) -> int:
        """Кількість зареєстрованих обробників."""
        return sum(len(h) for h in self._handlers.values())

    @property
    def event_count(self) -> int:
        """Кількість опублікованих подій."""
        return len(self._history)
