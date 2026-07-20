"""
DTO для User (Користувач).

Використовуються для передачі даних між Application та Presentation шарами.
"""

from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class UserDTO:
    """Повний DTO користувача для відповіді клієнту."""
    id: UUID
    name: str
    login: str
    role: str = "cashier"
    is_active: bool = True
    email: str = ""
    phone: str = ""
    created_at: Optional[datetime] = None
    last_login_at: Optional[datetime] = None


@dataclass
class UserCreateDTO:
    """DTO для створення нового користувача."""
    name: str
    login: str
    password: str
    role: str = "cashier"
    is_active: bool = True
    email: str = ""
    phone: str = ""
    pin_code: Optional[str] = None
