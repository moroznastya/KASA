"""Application Interfaces (Ports) для Application Layer."""

from .i_event_bus import IEventBus
from .i_unit_of_work import IUnitOfWork

__all__ = [
    "IEventBus",
    "IUnitOfWork",
]
