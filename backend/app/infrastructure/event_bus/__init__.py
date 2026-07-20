"""
Infrastructure Layer: Event Bus.

Реалізація IEventBus для публікації та підписки на доменні події.
"""

from .local_event_bus import LocalEventBus

__all__ = [
    "LocalEventBus",
]
