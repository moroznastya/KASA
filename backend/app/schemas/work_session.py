"""
Pydantic схеми для моделі WorkSession (робоча сесія користувача).
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict


class WorkSessionResponse(BaseModel):
    """Відповідь з даними однієї робочої сесії."""
    id: UUID
    user_id: UUID
    login_time: datetime
    logout_time: Optional[datetime] = None
    duration_hours: Optional[float] = None

    model_config = ConfigDict(from_attributes=True)


class WorkSessionDetail(WorkSessionResponse):
    """Розширена відповідь з ім'ям користувача."""
    user_name: Optional[str] = None


class UserHoursSummary(BaseModel):
    """Підсумок відпрацьованих годин для одного користувача."""
    user_id: UUID
    user_name: str
    total_hours: float
    hourly_rate: Optional[float] = None
    salary: Optional[float] = None


class WorkSessionReportResponse(BaseModel):
    """Звіт за місяць: список користувачів з годинами та зарплатою."""
    month: int
    year: int
    items: list[UserHoursSummary]
