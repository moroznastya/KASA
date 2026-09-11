"""
Pydantic схеми для моделі WorkSession (робоча сесія користувача).
"""

from datetime import UTC, datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, field_serializer


class WorkSessionResponse(BaseModel):
    """Відповідь з даними однієї робочої сесії."""
    id: UUID
    user_id: UUID
    login_time: datetime
    logout_time: Optional[datetime] = None
    duration_hours: Optional[float] = None
    is_active: bool = False  # True — сесія ще триває (logout_time IS NULL)

    model_config = ConfigDict(from_attributes=True)

    @field_serializer('login_time', 'logout_time')
    def _serialize_utc(self, dt, _info):
        """Серіалізація datetime як UTC з маркером 'Z'.

        Модель зберігає naive UTC (datetime.utcnow без tzinfo). Без маркера
        фронтенд (JavaScript) інтерпретує рядок без зсуву як ЛОКАЛЬНИЙ час
        (UTC+3) → помилка рівно на 3 години. 'Z' змушує JS трактувати час
        як UTC і коректно конвертувати в локальний пояс користувача.
        """
        if dt is None:
            return None
        if dt.tzinfo is None:
            # naive datetime вважаємо UTC
            dt = dt.replace(tzinfo=UTC)
        return dt.astimezone(UTC).isoformat().replace('+00:00', 'Z')


class WorkSessionDetail(WorkSessionResponse):
    """Розширена відповідь з ім'ям користувача.

    Успадковує серіалізатори login_time/logout_time від WorkSessionResponse.
    """
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
