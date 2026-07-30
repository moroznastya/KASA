"""Domain Events: User."""

from __future__ import annotations

from dataclasses import dataclass
from uuid import UUID

from .base_event import BaseDomainEvent


@dataclass(kw_only=True)
class UserLoggedIn(BaseDomainEvent):
    """Користувач увійшов у систему."""
    user_id: UUID
    login_method: str  # "password", "pin"


@dataclass(kw_only=True)
class UserCreated(BaseDomainEvent):
    """Створено нового користувача."""
    user_id: UUID
    login: str
    role: str
